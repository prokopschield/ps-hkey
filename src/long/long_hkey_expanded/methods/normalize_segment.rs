use std::sync::Arc;

use ps_datachunk::{DataChunk, PsDataChunkError};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{
    long::{long_hkey_expanded::constants::LHKEY_SEGMENT_MAX_LENGTH, LongHkeyExpanded},
    Hkey, PsHkeyError, Range, Store,
};

use super::update::helpers::{calculate_depth, calculate_segment_length};

impl LongHkeyExpanded {
    pub fn normalize_segment<C, E, S>(
        &self,
        store: &S,
        depth: u32,
        range: Range,
    ) -> Result<Arc<Self>, E>
    where
        C: DataChunk + Send,
        E: From<PsHkeyError> + From<PsDataChunkError> + Send,
        S: Store<Chunk = C, Error = E> + Sync,
    {
        if range.end == range.start {
            return Ok(Arc::from(Self::default()));
        }

        for part in self.parts.iter() {
            if part.0 == range {
                match &part.1 {
                    Hkey::LongHkeyExpanded(lhkey) => return Ok(lhkey.clone()),
                    Hkey::LongHkey(lhkey) => {
                        let lhkey = lhkey.expand(store)?;

                        return Ok(Arc::from(lhkey));
                    }
                    _ => (),
                }
            }
        }

        let length = range.end - range.start;
        let depth = calculate_depth(depth, length);

        if depth == 0 && length <= LHKEY_SEGMENT_MAX_LENGTH {
            let data = self.resolve_slice(store, range)?;
            let parts = Arc::from([(0..length, store.put(&data)?)]);
            let lhkey = Self::new(0, data.len(), parts);

            return Ok(Arc::from(lhkey));
        }

        if depth == 0 {
            let iterator = (0..length.div_ceil(LHKEY_SEGMENT_MAX_LENGTH)).into_par_iter();

            let parts: Result<Vec<_>, E> = iterator
                .map(|index| {
                    let begin = range.start + index * LHKEY_SEGMENT_MAX_LENGTH;
                    let end = range
                        .end
                        .min(range.start + (index + 1) * LHKEY_SEGMENT_MAX_LENGTH);
                    let data = self.resolve_slice(store, begin..end)?;
                    let hkey = store.put(&data)?;

                    Ok::<_, E>((
                        index * LHKEY_SEGMENT_MAX_LENGTH..(index + 1) * LHKEY_SEGMENT_MAX_LENGTH,
                        hkey,
                    ))
                })
                .collect();

            let parts = Arc::from(parts?.into_boxed_slice());

            let lhkey = Self::new(1, length, parts);

            return Ok(Arc::from(lhkey));
        }

        // if depth >= 1, resolve recursively

        let segment_length = calculate_segment_length(depth);

        let iterator = (0..length.div_ceil(segment_length)).into_par_iter();

        let parts: Result<Vec<_>, E> = iterator
            .map(|index| {
                let begin = range.start + index * segment_length;
                let end = range.end.min(range.start + (index + 1) * segment_length);
                let lhkey = self.normalize_segment(store, depth - 1, begin..end)?;
                let hkey = Hkey::LongHkey(Arc::from(lhkey.store(store)?));

                Ok::<_, E>((index * segment_length..(index + 1) * segment_length, hkey))
            })
            .collect();

        let parts = Arc::from(parts?.into_boxed_slice());

        let lhkey = Self::new(depth, length, parts);

        Ok(Arc::from(lhkey))
    }
}
