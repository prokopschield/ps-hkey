use ps_base64::base64;

use crate::{methods::compact::compact_dhash, AsyncStore, Hkey};

impl Hkey {
    pub async fn compact_async<S: AsyncStore>(&self, store: &S) -> Result<Vec<u8>, S::Error> {
        match self.shrink_async(store).await? {
            Self::Raw(value) => Ok(value.to_vec()),
            Self::Base64(value) => Ok(base64::decode(value.as_bytes())),
            Self::Direct(hash) => Ok(hash.compact().to_vec()),
            Self::Encrypted(hash, key) => Ok(compact_dhash(&hash, &key, 0)),
            Self::ListRef(hash, key) => Ok(compact_dhash(&hash, &key, 1)),
            Self::LongHkey(lhkey) => Ok(compact_dhash(lhkey.hash_ref(), lhkey.key_ref(), 1)),
            hkey => hkey.shrink_async(store).await?.compact_async(store).await,
        }
    }
}
