use ps_util::ToResult;

use crate::{Hkey, LongHkey, LongHkeyExpanded, PsHkeyError};

impl LongHkeyExpanded {
    pub fn store<E, Ef, F>(&self, store: &F) -> Result<LongHkey, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> Result<Hkey, Ef> + Sync,
    {
        match store(self.to_string().as_bytes())? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => Err(PsHkeyError::StorageError)?,
        }
        .ok()
    }
}
