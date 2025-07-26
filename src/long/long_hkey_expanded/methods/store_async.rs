use ps_datachunk::DataChunk;
use ps_promise::PromiseRejection;
use ps_util::ToResult;

use crate::{AsyncStore, Hkey, LongHkey, LongHkeyExpanded, PsHkeyError};

impl LongHkeyExpanded {
    pub async fn store_async<C, E, Es, S>(&self, store: &S) -> Result<LongHkey, E>
    where
        C: DataChunk + Unpin,
        E: From<Es> + From<PsHkeyError> + Send,
        Es: Into<E> + PromiseRejection + Send + Unpin,
        S: AsyncStore<Chunk = C, Error = Es> + Sync + ?Sized,
    {
        match store.put(self.to_string().as_bytes()).await? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => Err(PsHkeyError::StorageError)?,
        }
        .ok()
    }
}
