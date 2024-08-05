pub mod helpers;

use std::{
    ops::{Mul, Sub},
    sync::Arc,
};

use helpers::{calculate_depth, calculate_segment_length};
use ps_datachunk::{DataChunk, PsDataChunkError};
use ps_hash::Hash;
use ps_util::ToResult;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{
    long::{long_hkey_expanded::constants::LHKEY_SEGMENT_MAX_LENGTH, LongHkeyExpanded},
    Hkey, PsHkeyError, Range,
};

impl LongHkeyExpanded {
    /// only to be used with depth=0
    pub fn update_flat<'lt, E, Ef, Es, F, S>(
        &self,
        fetch: &F,
        store: &S,
        data: &[u8],
        range: Range,
    ) -> Result<Arc<Self>, E>
    where
        E: From<Ef> + From<Es> + From<PsHkeyError> + From<PsDataChunkError> + Send,
        Ef: Into<E> + Send,
        Es: Into<E> + Send,
        F: Fn(&Hash) -> Result<DataChunk<'lt>, Ef> + Sync,
        S: Fn(&[u8]) -> Result<Hkey, Es> + Sync,
    {
        let range = range.start..range.end.min(range.start + data.len());
        let length = self.size.max(range.end);

        let iterator = (0..length.div_ceil(LHKEY_SEGMENT_MAX_LENGTH)).into_par_iter();
        let resolver = |hash: &Hash| Ok::<_, E>(fetch(hash)?);

        let parts: Result<Vec<(Range, Hkey)>, E> = iterator
            .map(|index| {
                let start = index.mul(LHKEY_SEGMENT_MAX_LENGTH);
                let end = (index + 1).mul(LHKEY_SEGMENT_MAX_LENGTH).min(length);

                match self.parts.get(index) {
                    Some(part) => {
                        if part.0.start == start && part.0.end == end {
                            return part.clone().ok();
                        }
                    }
                    None => (),
                }

                if range.start <= start && range.end >= end {
                    // only new bytes
                    return Ok((start..end, store(&data[start..end])?));
                }

                let bytes = self.resolve_slice(&resolver, start..end.min(self.size))?;

                if start >= range.end || end <= range.start {
                    // only old bytes
                    return Ok((start..end, store(&bytes)?));
                }

                let mut vector = Vec::with_capacity(end - start);

                if start < range.start {
                    // begin with old bytes
                    vector.extend_from_slice(&bytes[0..range.start]);
                    vector.extend_from_slice(&data[range.start..end]);
                } else {
                    // end with old bytes
                    vector.extend_from_slice(&data[end..range.end]);
                    vector.extend_from_slice(&bytes[range.end..end]);
                }

                Ok::<_, E>((start..end, store(&vector)?))
            })
            .collect();

        let lhkey = LongHkeyExpanded::new(0, length, Arc::from(parts?.into_boxed_slice()));

        Ok(Arc::from(lhkey))
    }

    pub fn update<'lt, E, Ef, Es, F, S>(
        &self,
        fetch: &F,
        store: &S,
        data: &[u8],
        range: Range,
    ) -> Result<Arc<Self>, E>
    where
        E: From<Ef> + From<Es> + From<PsHkeyError> + From<PsDataChunkError> + Send,
        Ef: Into<E> + Send,
        Es: Into<E> + Send,
        F: Fn(&Hash) -> Result<DataChunk<'lt>, Ef> + Sync,
        S: Fn(&[u8]) -> Result<Hkey, Es> + Sync,
    {
        let range = range.start..range.end.min(range.start + data.len());
        let length = range.end.max(self.size);
        let depth = calculate_depth(self.depth, range.end);
        let segment_length = calculate_segment_length(depth);

        if depth == 0 {
            return self.update_flat(fetch, store, data, range);
        }

        let iterator = (0..length.div_ceil(segment_length)).into_par_iter();

        let transformer = |lhkey: &LongHkeyExpanded| {
            Ok::<_, E>(Hkey::LongHkey(Arc::from(
                lhkey.store(&|bytes| Ok::<Hkey, E>(store(bytes)?))?,
            )))
        };

        let parts: Result<Vec<_>, E> = iterator
            .map(|index| {
                let start = index * segment_length;
                let end = (index + 1).mul(segment_length).min(length);
                let segment_range = start.min(self.size)..end.min(self.size);
                let segment = self.normalize_segment(fetch, store, depth - 1, segment_range)?;

                if start >= range.end || end <= range.start {
                    // outside of modified range
                    return Ok((start..end, transformer(&segment)?));
                }

                let offset_start = start.max(range.start);
                let offset_end = end.min(range.end);
                let offset_range = offset_start..offset_end;
                let data_slice_start = offset_start.sub(range.start);
                let data_slice_end = offset_end.sub(range.start);
                let data_slice_range = data_slice_start..data_slice_end;
                let data_slice = &data[data_slice_range];

                let segment = segment.update(fetch, store, data_slice, offset_range)?;

                Ok((start..end, transformer(&segment)?))
            })
            .collect();

        let parts = Arc::from(parts?.into_boxed_slice());

        let lhkey = LongHkeyExpanded::new(depth, length, parts);

        Ok(Arc::from(lhkey))
    }
}
