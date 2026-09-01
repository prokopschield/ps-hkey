use ps_datachunk::DataChunk;
use ps_promise::PromiseRejection;

use crate::{long::LongHkeyExpanded, AsyncStore, Hkey, HkeyError};

impl LongHkeyExpanded {
    /// Stores this [`LongHkeyExpanded`] and returns the resulting [`Hkey::LongHkey`].
    pub async fn shrink_async<C, E, S>(&self, store: S) -> Result<Hkey, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        self.store_async(store).await
    }
}
