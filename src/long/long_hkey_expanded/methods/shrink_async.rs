use ps_datachunk::{DataChunk, DataChunkError};
use ps_promise::PromiseRejection;

use crate::{long::LongHkeyExpanded, AsyncStore, Hkey, HkeyError};

impl LongHkeyExpanded {
    /// Stores this [`LongHkeyExpanded`] and returns the resulting [`Hkey::LongHkey`].
    ///
    /// Content that fits inline is returned as [`Hkey::Raw`] instead.
    pub async fn shrink_async<C, E, S>(&self, store: S) -> Result<Hkey, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        self.store_async(store).await
    }
}
