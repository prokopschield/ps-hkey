use std::{future::Future, pin::Pin, sync::Arc};

use ps_datachunk::{Bytes, DataChunk};
use ps_promise::PromiseRejection;

use crate::{
    long::{
        long_hkey_expanded::{
            constants::{LHKEY_LEVEL_MAX_LENGTH, LHKEY_SEGMENT_MAX_LENGTH},
            methods::update::helpers::{calculate_depth, calculate_segment_length},
        },
        LongHkeyExpanded,
    },
    AsyncStore, Hkey, PsHkeyError, Range,
};

impl LongHkeyExpanded {
    pub fn from_blob_async_box<'a, C, E, S>(
        store: &'a S,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<Self, E>> + Send + Sync + 'a>>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        Box::pin(async move { Self::from_blob_async(store, data).await })
    }

    pub async fn from_blob_async<C, E, S>(store: &S, data: &[u8]) -> Result<Self, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        let depth = calculate_depth(0, data.len());

        let parts: Result<Vec<(Range, Hkey)>, E> = if data.len() > LHKEY_LEVEL_MAX_LENGTH {
            let segment_length = calculate_segment_length(depth);

            let mut chunks = Vec::new();

            for (index, chunk) in data.chunks(segment_length).enumerate() {
                let start = index * segment_length;
                let end = start + chunk.len();
                let hkey = Self::from_blob_async_box(store, chunk)
                    .await?
                    .shrink_async(store)
                    .await?;

                chunks.push((start..end, hkey));
            }

            Ok(chunks)
        } else {
            let mut chunks = Vec::new();

            for (index, chunk) in data.chunks(LHKEY_SEGMENT_MAX_LENGTH).enumerate() {
                let start = index * LHKEY_SEGMENT_MAX_LENGTH;
                let end = start + chunk.len();
                let hkey = store.put(Bytes::copy_from_slice(chunk)).await?;

                chunks.push((start..end, hkey));
            }

            Ok(chunks)
        };

        let parts = Arc::from(parts?.into_boxed_slice());
        let lhkey = Self::new(depth, data.len(), parts);

        Ok(lhkey)
    }
}
