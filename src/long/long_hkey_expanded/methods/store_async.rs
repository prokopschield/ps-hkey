use ps_datachunk::{Bytes, DataChunk};
use ps_promise::PromiseRejection;
use ps_util::ToResult;

use crate::{AsyncStore, Hkey, HkeyError, LongHkey, LongHkeyExpanded};

impl LongHkeyExpanded {
    pub async fn store_async<C, E, S>(&self, store: &S) -> Result<LongHkey, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        match store.put(Bytes::from_owner(self.to_string())).await? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => Err(HkeyError::Storage)?,
        }
        .ok()
    }
}
