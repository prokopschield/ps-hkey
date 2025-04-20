use std::future::Future;

use ps_util::ToResult;

use crate::{Hkey, LongHkey, LongHkeyExpanded, PsHkeyError};

impl LongHkeyExpanded {
    pub async fn store_async<'lt, E, Ef, F, Ff>(&self, store: &F) -> Result<LongHkey, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> Ff + Sync,
        Ff: Future<Output = Result<Hkey, Ef>> + Send,
    {
        match store(self.to_string().as_bytes()).await? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => Err(PsHkeyError::StorageError)?,
        }
        .ok()
    }
}
