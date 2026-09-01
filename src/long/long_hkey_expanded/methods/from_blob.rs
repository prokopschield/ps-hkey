use std::sync::Arc;

use ps_datachunk::{DataChunk, DataChunkError};
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
    Hkey, HkeyError, Range, Store,
};

impl LongHkeyExpanded {
    pub fn from_blob<'a, C, E, S>(store: &'a S, data: &[u8]) -> Result<Self, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use crate::long::long_hkey_expanded::constants::LHKEY_LEVEL_MAX_LENGTH;
    use crate::{InMemoryStore, Store};

    /// A blob longer than one level whose length is 1 to 21 bytes past a
    /// multiple of [`LHKEY_LEVEL_MAX_LENGTH`] gets a trailing sub-node whose
    /// manifest is at most 40 bytes, which `put` inlines as [`Hkey::Raw`];
    /// storing such a blob used to fail with [`HkeyError::Storage`].
    #[test]
    fn stores_blobs_with_tiny_trailing_segments() {
        let store = InMemoryStore::default();

        for size in [
            LHKEY_LEVEL_MAX_LENGTH + 1,
            LHKEY_LEVEL_MAX_LENGTH + 21,
            2 * LHKEY_LEVEL_MAX_LENGTH + 5,
        ] {
            let data = vec![129u8; size];

            let hkey = store.put(&data).expect("Failed to store blob");
            let resolved = hkey.resolve(&store).expect("Failed to resolve blob");

            assert_eq!(&resolved[..], &data[..]);
        }
    }
}
