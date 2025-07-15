use std::sync::Arc;

use ps_datachunk::DataChunk;
use rayon::{
    iter::{IndexedParallelIterator, ParallelIterator},
    slice::ParallelSlice,
};

use crate::{
    long::{
        long_hkey_expanded::{
            constants::{LHKEY_LEVEL_MAX_LENGTH, LHKEY_SEGMENT_MAX_LENGTH},
            methods::update::helpers::{calculate_depth, calculate_segment_length},
        },
        LongHkeyExpanded,
    },
    Hkey, PsHkeyError, Range, Store,
};

impl LongHkeyExpanded {
    pub fn from_blob<C, E, S>(store: &S, data: &[u8]) -> Result<Self, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk = C, Error = E> + Sync,
    {
        let depth = calculate_depth(0, data.len());

        let parts: Result<Vec<(Range, Hkey)>, E> = if data.len() > LHKEY_LEVEL_MAX_LENGTH {
            let segment_length = calculate_segment_length(depth);

            let chunks = data.par_chunks(segment_length);

            chunks
                .enumerate()
                .map(|(index, chunk)| {
                    let start = index * segment_length;
                    let end = start + chunk.len();
                    let hkey = Self::from_blob(store, chunk)?.shrink(store)?;

                    Ok((start..end, hkey))
                })
                .collect()
        } else {
            let chunks = data.par_chunks(LHKEY_SEGMENT_MAX_LENGTH);

            chunks
                .enumerate()
                .map(|(index, chunk)| {
                    let start = index * LHKEY_SEGMENT_MAX_LENGTH;
                    let end = start + chunk.len();
                    let hkey = store.put(chunk)?;

                    Ok((start..end, hkey))
                })
                .collect()
        };

        let parts = Arc::from(parts?.into_boxed_slice());
        let lhkey = Self::new(depth, data.len(), parts);

        Ok(lhkey)
    }
}
