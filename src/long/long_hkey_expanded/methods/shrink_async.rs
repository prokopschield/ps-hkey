use ps_datachunk::DataChunk;
use ps_promise::PromiseRejection;

use crate::{long::LongHkeyExpanded, AsyncStore, Hkey, PsHkeyError};

impl LongHkeyExpanded {
    /// transforms this [`LongHkey`] into a [`Hkey::ListRef`]
    pub async fn shrink_async<C, E, S>(&self, store: &S) -> Result<Hkey, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        Ok(self.store_async(store).await?.into())
    }
}
