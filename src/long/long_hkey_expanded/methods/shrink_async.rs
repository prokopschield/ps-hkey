use ps_datachunk::DataChunk;
use ps_promise::PromiseRejection;

use crate::{long::LongHkeyExpanded, AsyncStore, Hkey, PsHkeyError};

impl LongHkeyExpanded {
    /// transforms this [`LongHkey`] into a [`Hkey::ListRef`]
    pub async fn shrink_async<C, E, Es, S>(&self, store: &S) -> Result<Hkey, E>
    where
        C: DataChunk + Unpin,
        E: From<Es> + From<PsHkeyError> + Send,
        Es: Into<E> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = Es> + Sync + ?Sized,
    {
        Ok(self.store_async(store).await?.into())
    }
}
