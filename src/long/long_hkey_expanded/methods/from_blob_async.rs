use std::{future::Future, pin::Pin, sync::Arc};

use ps_datachunk::{Bytes, DataChunk, DataChunkError};
use ps_promise::PromiseRejection;

use crate::{
    long::{
        long_hkey_expanded::{
            constants::{LHKEY_LEVEL_MAX_LENGTH, LHKEY_SEGMENT_MAX_LENGTH},
            methods::update::helpers::{calculate_depth, calculate_segment_length},
        },
        LongHkeyExpanded,
    },
    AsyncStore, Hkey, HkeyError, Range,
};

impl LongHkeyExpanded {
    pub fn from_blob_async_box<'a, C, E, S>(
        store: S,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<Self, E>> + Send + 'a>>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        Box::pin(async move { Self::from_blob_async(store, data).await })
    }

    pub async fn from_blob_async<C, E, S>(store: S, data: &[u8]) -> Result<Self, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        let depth = calculate_depth(0, data.len());

        let parts: Result<Vec<(Range, Hkey)>, E> = if data.len() > LHKEY_LEVEL_MAX_LENGTH {
            let segment_length = calculate_segment_length(depth);

            let mut chunks = Vec::new();

            for (index, chunk) in data.chunks(segment_length).enumerate() {
                let start = index * segment_length;
                let end = start + chunk.len();
                let hkey = Self::from_blob_async_box(store.clone(), chunk)
                    .await?
                    .shrink_async(store.clone())
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_datachunk::Bytes;

    use crate::long::long_hkey_expanded::constants::LHKEY_LEVEL_MAX_LENGTH;
    use crate::{AsyncStore, InMemoryAsyncStore};

    /// A blob longer than one level whose length is 1 to 21 bytes past a
    /// multiple of [`LHKEY_LEVEL_MAX_LENGTH`] gets a trailing sub-node whose
    /// manifest is at most 40 bytes, which `put` inlines as [`Hkey::Raw`];
    /// storing such a blob used to fail with [`HkeyError::Storage`].
    #[test]
    fn stores_blobs_with_tiny_trailing_segments() {
        futures::executor::block_on(async {
            let store = InMemoryAsyncStore::default();

            for size in [
                LHKEY_LEVEL_MAX_LENGTH + 1,
                LHKEY_LEVEL_MAX_LENGTH + 21,
                2 * LHKEY_LEVEL_MAX_LENGTH + 5,
            ] {
                let data = vec![129u8; size];

                let hkey = store
                    .put(Bytes::copy_from_slice(&data))
                    .await
                    .expect("Failed to store blob");
                let resolved = hkey
                    .resolve_async(store.clone())
                    .await
                    .expect("Failed to resolve blob");

                assert_eq!(&resolved[..], &data[..]);
            }
        });
    }
}
