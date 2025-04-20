pub mod helpers;

use std::{
    ops::{Add, Mul, Sub},
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
        range: &Range,
    ) -> Result<Arc<Self>, E>
    where
        E: From<Ef> + From<Es> + From<PsHkeyError> + From<PsDataChunkError> + Send,
        Ef: Into<E> + Send,
        Es: Into<E> + Send,
        F: Fn(&Hash) -> Result<DataChunk<'lt>, Ef> + Sync,
        S: Fn(&[u8]) -> Result<Hkey, Es> + Sync,
    {
        let fetch = &|hash: &Hash| Ok::<_, E>(fetch(hash)?);

        let length = data.len().min(range.end - range.start);

        let range = range.start..range.start + length;
        let data = &data[..length];

        let new_size = range.end.max(self.size);

        let parts: Result<Vec<(Range, Hkey)>, E> = (0..new_size.div_ceil(LHKEY_SEGMENT_MAX_LENGTH))
            .into_par_iter()
            .map(|index| {
                let part_start = index.mul(LHKEY_SEGMENT_MAX_LENGTH);
                let part_end = index.add(1).mul(LHKEY_SEGMENT_MAX_LENGTH).min(new_size);

                // part is entirely outside of range
                if range.end <= part_start || range.start >= part_end {
                    if let Some(segment) = self.parts.get(index) {
                        if segment.0.start == part_start && segment.0.end == part_end {
                            return segment.clone().ok();
                        }
                    }

                    let slice = &self.resolve_slice(fetch, part_start..part_end)?[..];

                    return (part_start..part_end, store(slice)?).ok();
                }

                // part is intirely within range
                if part_start >= range.start && part_end <= range.end {
                    let slice = &data[part_start - range.start..part_end - range.start];

                    return (part_start..part_end, store(slice)?).ok();
                }

                // range is entirely within part
                if range.start >= part_start && range.end <= part_end {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    let original = self.resolve_slice(fetch, part_start..part_end)?;

                    let data_start = range.start - part_start;
                    let data_end = data_start + data.len();

                    let orig_start = data_end.min(original.len());

                    buffer.extend_from_slice(&original[..data_start]);
                    buffer.extend_from_slice(data);
                    buffer.extend_from_slice(&original[orig_start..]);

                    return (part_start..part_end, store(&buffer)?).ok();
                }

                // part begins with original data
                if range.start > part_start {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    buffer.extend_from_slice(&self.resolve_slice(fetch, part_start..range.start)?);
                    buffer.extend_from_slice(&data[..part_end - range.start]);

                    return (part_start..part_end, store(&buffer)?).ok();
                }

                // part begins with new data
                if part_start >= range.start {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    let data_start = part_start - range.start;
                    let orig_start = data.len() - data_start;

                    buffer.extend_from_slice(&data[data_start..]);
                    buffer.extend_from_slice(&self.resolve_slice(fetch, orig_start..part_end)?);

                    return (part_start..part_end, store(&buffer)?).ok();
                }

                // all variants have been exhausted
                Err(PsHkeyError::UnreachableCodeReached)?
            })
            .collect();

        let lhkey = Self::new(0, length, Arc::from(parts?.into_boxed_slice()));

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
            return self.update_flat(fetch, store, data, &range);
        }

        let iterator = (0..length.div_ceil(segment_length)).into_par_iter();

        let transformer = |lhkey: &Self| Ok::<_, E>(lhkey.store(store)?.into());

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

        let lhkey = Self::new(depth, length, parts);

        Ok(Arc::from(lhkey))
    }
}
