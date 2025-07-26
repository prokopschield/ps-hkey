use ps_datachunk::{Bytes, DataChunk};
use ps_promise::PromiseRejection;
use ps_util::ToResult;

use crate::{AsyncStore, Hkey, LongHkey, LongHkeyExpanded, PsHkeyError};

impl LongHkeyExpanded {
    pub async fn store_async<C, E, S>(&self, store: &S) -> Result<LongHkey, E>
    where
        C: DataChunk + Unpin,
        E: From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        match store.put(Bytes::from_owner(self.to_string())).await? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => Err(PsHkeyError::StorageError)?,
        }
        .ok()
    }
}
