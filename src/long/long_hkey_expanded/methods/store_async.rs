use ps_datachunk::{Bytes, DataChunk, DataChunkError};
use ps_promise::PromiseRejection;

use crate::{AsyncStore, Hkey, HkeyError, LongHkey, LongHkeyExpanded, MAX_SIZE_RAW};

impl LongHkeyExpanded {
    /// Stores this node's manifest, returning a [`Hkey::LongHkey`] referencing it.
    ///
    /// A node whose content fits inline is never stored as a [`LongHkey`]; its content is
    /// returned as [`Hkey::Raw`] instead.
    pub async fn store_async<C, E, S>(&self, store: S) -> Result<Hkey, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        if self.size() <= MAX_SIZE_RAW {
            let content = self.resolve_slice_async(store, 0..self.size()).await?;

            return Hkey::from_raw(&content)
                .map_err(HkeyError::Construction)
                .map_err(Into::into);
        }

        match store.put(Bytes::from_owner(self.to_string())).await? {
            Hkey::Encrypted(hash, key) => Ok(LongHkey::from_hash_and_key(hash, key).into()),
            _ => Err(HkeyError::Storage)?,
        }
    }
}
