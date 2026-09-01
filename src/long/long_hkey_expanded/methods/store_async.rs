use ps_datachunk::{Bytes, DataChunk};
use ps_promise::PromiseRejection;

use crate::{AsyncStore, Hkey, HkeyError, LongHkey, LongHkeyExpanded};

impl LongHkeyExpanded {
    pub async fn store_async<C, E, S>(&self, store: S) -> Result<Hkey, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        match store.put(Bytes::from_owner(self.to_string())).await? {
            Hkey::Encrypted(hash, key) => Ok(LongHkey::from_hash_and_key(hash, key).into()),
            _ => Err(HkeyError::Storage)?,
        }
    }
}
