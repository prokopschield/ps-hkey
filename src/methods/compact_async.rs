use ps_datachunk::Bytes;

use crate::{decode_base64, methods::compact::compact_dhash, AsyncStore, Hkey};

impl Hkey {
    pub async fn compact_async<S: AsyncStore>(&self, store: S) -> Result<Bytes, S::Error> {
        match self.shrink_async(store.clone()).await? {
            Self::Raw(value) => Ok(Bytes::copy_from_slice(&value)),
            Self::Base64(value) => Ok(decode_base64(value.as_bytes())?.into()),
            Self::Direct(hash) => Ok(Bytes::copy_from_slice(hash.compact())),
            Self::Encrypted(hash, key) => Ok(compact_dhash(&hash, &key, 0)),
            Self::ListRef(hash, key) => Ok(compact_dhash(&hash, &key, 1)),
            Self::LongHkey(lhkey) => Ok(compact_dhash(lhkey.hash_ref(), lhkey.key_ref(), 1)),
            hkey => {
                let shrunk = hkey.shrink_async(store.clone()).await?;

                // Boxed to break the cycle in the recursive future type.
                Box::pin(shrunk.compact_async(store)).await
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use futures::executor::block_on;

    use crate::{Hkey, InMemoryAsyncStore, MAX_SIZE_RAW};

    /// Instantiates the async shrink and compact chain with a concrete store;
    /// compiles only if every recursive future in the chain is boxed (E0733).
    #[test]
    fn test_oversized_raw_variant_roundtrip_async() {
        let store = InMemoryAsyncStore::default();
        let data = b"This buffer exceeds the inline maximum!!!";

        assert!(data.len() > MAX_SIZE_RAW);

        let hkey = Hkey::from_raw(data).expect("Failed to allocate Hkey::Raw");

        let compact = block_on(hkey.compact_async(store.clone())).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        let shrunk = block_on(hkey.shrink_async(store)).expect("Failed to shrink Hkey");

        assert_eq!(restored, shrunk);
    }
}
