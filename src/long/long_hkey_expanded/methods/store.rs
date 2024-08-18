use ps_util::ToResult;

use crate::{Hkey, LongHkey, LongHkeyExpanded, PsHkeyError};

impl LongHkeyExpanded {
    pub fn store<'lt, E, F>(&self, store: &F) -> Result<LongHkey, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> Result<Hkey, E> + Sync,
    {
        match store(self.to_string().as_bytes())? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => Err(PsHkeyError::StorageError)?,
        }
        .ok()
    }
}
