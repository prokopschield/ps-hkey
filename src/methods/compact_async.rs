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
                hkey.shrink_async(store.clone())
                    .await?
                    .compact_async(store)
                    .await
            }
        }
    }
}
