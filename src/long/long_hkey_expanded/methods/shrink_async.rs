use std::future::Future;

use crate::{long::LongHkeyExpanded, Hkey, PsHkeyError};

impl LongHkeyExpanded {
    /// transforms this [`LongHkey`] into a [`Hkey::ListRef`]
    pub async fn shrink_async<E, Ef, F, Ff>(&self, store: &F) -> Result<Hkey, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> Ff,
        Ff: Future<Output = Result<Hkey, Ef>> + Sync,
    {
        Ok(self.store_async(store).await?.into())
    }
}
