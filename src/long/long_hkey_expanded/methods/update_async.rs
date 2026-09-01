use std::{
    cmp::Ordering::{Equal, Greater, Less},
    future::Future,
    ops::{Add, Mul, Sub},
    pin::Pin,
    sync::Arc,
};

use futures::future::try_join_all;
use ps_buffer::Buffer;
use ps_datachunk::{Bytes, DataChunk, DataChunkError};
use ps_promise::PromiseRejection;
use ps_util::ToResult;

use crate::{
    long::{long_hkey_expanded::constants::LHKEY_SEGMENT_MAX_LENGTH, LongHkeyExpanded},
    AsyncStore, HkeyBug, HkeyError, Range,
};

use super::update::helpers::{calculate_depth, calculate_segment_length};

impl LongHkeyExpanded {
    /// Asynchronous variant of [`Self::update_flat`]. Only valid on depth-0 receivers;
    /// [`Self::update_async`] dispatches here iff the effective depth is 0.
    pub async fn update_flat_async<C, E, S>(
        &self,
        store: S,
        data: &[u8],
        range: &Range,
    ) -> Result<Self, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + From<DataChunkError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
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

        let futures = (0..new_size.div_ceil(LHKEY_SEGMENT_MAX_LENGTH)).map(|index| {
            let store = store.clone();
            let range = range.clone();

            async move {
                let part_start = index.mul(LHKEY_SEGMENT_MAX_LENGTH);
                let part_end = index.add(1).mul(LHKEY_SEGMENT_MAX_LENGTH).min(new_size);

                // part is entirely outside of range
                if range.end <= part_start || range.start >= part_end {
                    if let Some(segment) = self.parts.get(index) {
                        if segment.0.start == part_start && segment.0.end == part_end {
                            return Ok::<_, E>(segment.clone());
                        }
                    }

                    let slice = self
                        .resolve_slice_exact_async(store.clone(), part_start..part_end)
                        .await?;

                    return (part_start..part_end, store.put(slice).await?).ok();
                }

                // part is intirely within range
                if part_start >= range.start && part_end <= range.end {
                    let slice = &data[part_start - range.start..part_end - range.start];
                    let hkey = store.put(Bytes::copy_from_slice(slice)).await?;

                    return (part_start..part_end, hkey).ok();
                }

                // range is entirely within part
                if range.start >= part_start && range.end <= part_end {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    let original = self
                        .resolve_slice_exact_async(store.clone(), part_start..part_end)
                        .await?;

                    let data_start = range.start - part_start;
                    let data_end = data_start + data.len();

                    buffer.extend_from_slice(&original[..data_start]);
                    buffer.extend_from_slice(data);
                    buffer.extend_from_slice(&original[data_end..]);

                    return (part_start..part_end, store.put(Bytes::from(buffer)).await?).ok();
                }

                // part begins with original data
                if range.start > part_start {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    buffer.extend_from_slice(
                        &self
                            .resolve_slice_exact_async(store.clone(), part_start..range.start)
                            .await?,
                    );
                    buffer.extend_from_slice(&data[..part_end - range.start]);

                    return (part_start..part_end, store.put(Bytes::from(buffer)).await?).ok();
                }

                // part begins with new data
                if part_start >= range.start {
                    let mut buffer = Vec::with_capacity(part_end - part_start);

                    let data_start = part_start - range.start;

                    buffer.extend_from_slice(&data[data_start..]);
                    buffer.extend_from_slice(
                        &self
                            .resolve_slice_async(store.clone(), range.end..part_end)
                            .await?,
                    );

                    return (part_start..part_end, store.put(Bytes::from(buffer)).await?).ok();
                }

                // all variants have been exhausted
                Err(HkeyError::Bug(HkeyBug::UpdateFlatAllVariantsExhausted))?
            }
        });

        let parts = try_join_all(futures).await?;

        let lhkey = Self::new(0, new_size, Arc::from(parts.into_boxed_slice()));

        Ok(lhkey)
    }

    /// Boxed [`Self::update_async`]; breaks the cycle in the recursive future type.
    pub fn update_async_box<'a, C, E, S>(
        &'a self,
        store: S,
        data: &'a [u8],
        range: Range,
    ) -> Pin<Box<dyn Future<Output = Result<Self, E>> + Send + 'a>>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + From<DataChunkError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        Box::pin(async move { self.update_async(store, data, range).await })
    }

    /// Asynchronous variant of [`Self::update`].
    pub async fn update_async<C, E, S>(
        &self,
        store: S,
        data: &[u8],
        range: Range,
    ) -> Result<Self, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + From<DataChunkError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
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
            return self.update_flat_async(store, data, &range).await;
        }

        let futures = (0..length.div_ceil(segment_length)).map(|index| {
            let store = store.clone();
            let range = range.clone();

            async move {
                let start = index * segment_length;
                let end = (index + 1).mul(segment_length).min(length);
                let segment_range = start.min(self.size)..end.min(self.size);
                let segment = self
                    .normalize_segment_async_box(store.clone(), depth - 1, segment_range)
                    .await?;

                if start >= range.end || end <= range.start {
                    // outside of modified range
                    return Ok((start..end, segment.store_async(store).await?));
                }

                let offset_start = start.max(range.start);
                let offset_end = end.min(range.end);
                // The segment is a local node; translate the write range into its coordinates.
                let segment_local_range = offset_start.sub(start)..offset_end.sub(start);
                let data_slice_start = offset_start.sub(range.start);
                let data_slice_end = offset_end.sub(range.start);
                let data_slice_range = data_slice_start..data_slice_end;
                let data_slice = &data[data_slice_range];

                let segment = segment
                    .update_async_box(store.clone(), data_slice, segment_local_range)
                    .await?;

                Ok::<_, E>((start..end, segment.store_async(store).await?))
            }
        });

        let parts = Arc::from(try_join_all(futures).await?.into_boxed_slice());

        let lhkey = Self::new(depth, length, parts);

        Ok(lhkey)
    }

    /// Asynchronous variant of [`Self::resolve_slice_exact`]: resolves `range` in full,
    /// zero-filling whatever lies past the end of the receiver.
    async fn resolve_slice_exact_async<C, E, S>(&self, store: S, range: Range) -> Result<Bytes, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + From<DataChunkError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        let resolved = self.resolve_slice_async(store, range.clone()).await?;
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
