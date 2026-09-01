use std::{future::Future, pin::Pin, sync::Arc};

use futures::future::try_join_all;
use ps_datachunk::{DataChunk, DataChunkError};
use ps_promise::PromiseRejection;
use ps_util::ToResult;

use crate::{
    long::{long_hkey_expanded::constants::LHKEY_SEGMENT_MAX_LENGTH, LongHkeyExpanded},
    AsyncStore, Hkey, HkeyError, Range,
};

use super::update::helpers::{calculate_depth, calculate_segment_length};

impl LongHkeyExpanded {
    /// Boxed [`Self::normalize_segment_async`]; breaks the cycle in the recursive future type.
    pub fn normalize_segment_async_box<'a, C, E, S>(
        &'a self,
        store: S,
        depth: u32,
        range: Range,
    ) -> Pin<Box<dyn Future<Output = Result<Self, E>> + Send + 'a>>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + From<DataChunkError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        Box::pin(async move { self.normalize_segment_async(store, depth, range).await })
    }

    /// Asynchronous variant of [`Self::normalize_segment`]; see there for the semantics of
    /// `depth` and `range`.
    pub async fn normalize_segment_async<C, E, S>(
        &self,
        store: S,
        depth: u32,
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

        if range.end == range.start {
            return Ok(Self::default());
        }

        // Check for existing segment
        for (part_range, hkey) in self.parts.iter() {
            if *part_range == range {
                match hkey {
                    Hkey::LongHkeyExpanded(lhkey) => return Ok(lhkey.clone()),
                    Hkey::LongHkey(lhkey) => return lhkey.expand_async(store).await,
                    _ => {}
                }
            }
        }

        let length = range.end - range.start;
        let depth = calculate_depth(depth, length);

        if depth == 0 && length <= LHKEY_SEGMENT_MAX_LENGTH {
            let data = self.resolve_slice_async(store.clone(), range).await?;
            let size = data.len();
            let segment_hkey = store.put(data).await?;
            let segment_parts = Arc::from([(0..length, segment_hkey)]);
            let lhkey = Self::new(0, size, segment_parts);

            return Ok(lhkey);
        }

        if depth == 0 {
            let count = length.div_ceil(LHKEY_SEGMENT_MAX_LENGTH);

            let futures = (0..count).map(|index| {
                let store = store.clone();
                let range = range.clone();

                async move {
                    let begin = index * LHKEY_SEGMENT_MAX_LENGTH;
                    let end = length.min(begin + LHKEY_SEGMENT_MAX_LENGTH);

                    let data = self
                        .resolve_slice_async(store.clone(), range.start + begin..range.start + end)
                        .await?;
                    let hkey = store.put(data).await?;

                    Ok::<_, E>((begin..end, hkey))
                }
            });

            let parts = Arc::from(try_join_all(futures).await?.into_boxed_slice());

            let lhkey = Self::new(0, length, parts);

            return Ok(lhkey);
        }

        // if depth >= 1, resolve recursively

        let segment_length = calculate_segment_length(depth);

        let futures = (0..length.div_ceil(segment_length)).map(|index| {
            let store = store.clone();
            let range = range.clone();

            async move {
                let begin = index * segment_length;
                let end = length.min(begin + segment_length);

                let segment = self
                    .normalize_segment_async_box(
                        store.clone(),
                        depth - 1,
                        range.start + begin..range.start + end,
                    )
                    .await?;

                let hkey = segment.store_async(store).await?;

                Ok::<_, E>((begin..end, hkey))
            }
        });

        let parts = Arc::from(try_join_all(futures).await?.into_boxed_slice());

        let lhkey = Self::new(depth, length, parts);

        Ok(lhkey)
    }
}
