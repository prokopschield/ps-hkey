pub mod helpers;

#[cfg(test)]
mod tests;

use std::{
    cmp::Ordering::{Equal, Greater, Less},
    ops::{Add, Mul, Sub},
    sync::Arc,
};

use helpers::{calculate_depth, calculate_segment_length};
use ps_buffer::Buffer;
use ps_datachunk::{Bytes, DataChunk, DataChunkError};
use ps_util::ToResult;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{
    long::{long_hkey_expanded::constants::LHKEY_SEGMENT_MAX_LENGTH, LongHkeyExpanded},
    Hkey, HkeyBug, HkeyError, Range, Store,
};

impl LongHkeyExpanded {
    /// Rewrites the receiver as a depth-0 node whose parts sit on the
    /// `LHKEY_SEGMENT_MAX_LENGTH` grid. Only valid on depth-0 receivers; [`Self::update`]
    /// dispatches here iff the effective depth is 0.
    pub fn update_flat<'a, C, E, S>(
        &self,
        store: &'a S,
        data: &[u8],
        range: &Range,
    ) -> Result<Self, E>
    where
        C: DataChunk,
        E: From<HkeyError> + From<DataChunkError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        if range.start > range.end {
            HkeyError::InvalidRange(range.clone()).err()?;
        }

        let length = data.len().min(range.end - range.start);

        // Writing no bytes is the identity
        if length == 0 {
            return Ok(self.clone());
        }

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

                    let slice = self.resolve_slice_exact(store, part_start..part_end)?;

                    return (part_start..part_end, store.put(&slice)?).ok();
                }

                // part is intirely within range
                if part_start >= range.start && part_end <= range.end {
                    let slice = &data[part_start - range.start..part_end - range.start];

                    return (part_start..part_end, store.put(slice)?).ok();
                }

                // range is entirely within part
                if range.start >= part_start && range.end <= part_end {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    let original = self.resolve_slice_exact(store, part_start..part_end)?;

                    let data_start = range.start - part_start;
                    let data_end = data_start + data.len();

                    buffer.extend_from_slice(&original[..data_start]);
                    buffer.extend_from_slice(data);
                    buffer.extend_from_slice(&original[data_end..]);

                    return (part_start..part_end, store.put(&buffer)?).ok();
                }

                // part begins with original data
                if range.start > part_start {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    buffer.extend_from_slice(
                        &self.resolve_slice_exact(store, part_start..range.start)?,
                    );
                    buffer.extend_from_slice(&data[..part_end - range.start]);

                    return (part_start..part_end, store.put(&buffer)?).ok();
                }

                // part begins with new data
                if part_start >= range.start {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    let data_start = part_start - range.start;

                    buffer.extend_from_slice(&data[data_start..]);
                    buffer.extend_from_slice(&self.resolve_slice(store, range.end..part_end)?);

                    return (part_start..part_end, store.put(&buffer)?).ok();
                }

                // all variants have been exhausted
                Err(HkeyError::Bug(HkeyBug::UpdateFlatAllVariantsExhausted))?
            })
            .collect();

        let lhkey = Self::new(0, new_size, Arc::from(parts?.into_boxed_slice()));

        Ok(lhkey)
    }

    pub fn update<'a, C, E, S>(&self, store: &'a S, data: &[u8], range: Range) -> Result<Self, E>
    where
        C: DataChunk,
        E: From<HkeyError> + From<DataChunkError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        if range.start > range.end {
            HkeyError::InvalidRange(range.clone()).err()?;
        }

        let range = range.start..range.end.min(range.start + data.len());

        // Writing no bytes is the identity. Returning early also avoids renormalizing and
        // re-storing every segment of the receiver for a write that changes nothing.
        if range.is_empty() {
            return Ok(self.clone());
        }

        let length = range.end.max(self.size);
        let depth = calculate_depth(self.depth, range.end);
        let segment_length = calculate_segment_length(depth);

        if depth == 0 {
            return self.update_flat(store, data, &range);
        }

        let iterator = (0..length.div_ceil(segment_length)).into_par_iter();

        let transformer = |lhkey: &Self| Ok::<_, E>(lhkey.store(store)?.into());

        let parts: Result<Vec<_>, E> = iterator
            .map(|index| {
                let start = index * segment_length;
                let end = (index + 1).mul(segment_length).min(length);
                let segment_range = start.min(self.size)..end.min(self.size);
                let segment = self.normalize_segment(store, depth - 1, segment_range)?;

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

                let segment = segment.update(store, data_slice, offset_range)?;

                Ok((start..end, transformer(&segment)?))
            })
            .collect();

        let parts = Arc::from(parts?.into_boxed_slice());

        let lhkey = Self::new(depth, length, parts);

        Ok(lhkey)
    }

    /// Resolves `range` in full, zero-filling whatever lies past the end of the receiver.
    ///
    /// A write past the current size grows the buffer, so the parts spanning the gap declare more
    /// bytes than the receiver holds.
    fn resolve_slice_exact<'a, C, E, S>(&self, store: &'a S, range: Range) -> Result<Bytes, E>
    where
        C: DataChunk,
        E: From<HkeyError> + From<DataChunkError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let resolved = self.resolve_slice(store, range.clone())?;
        let length = range.end - range.start;

        match resolved.len().cmp(&length) {
            Greater => Err(HkeyError::Bug(HkeyBug::ResolvedSliceTooLong))?,
            Equal => return Ok(resolved),
            Less => {}
        }

        let mut buffer = Buffer::with_capacity(length).map_err(HkeyError::from)?;

        buffer
            .extend_from_slice(&resolved)
            .map_err(HkeyError::from)?;

        buffer.resize(length, 0).map_err(HkeyError::from)?;

        Ok(buffer.into())
    }
}
