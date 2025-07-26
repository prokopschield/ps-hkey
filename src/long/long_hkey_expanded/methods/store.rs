use ps_datachunk::DataChunk;
use ps_util::ToResult;

use crate::{Hkey, LongHkey, LongHkeyExpanded, PsHkeyError, Store};

impl LongHkeyExpanded {
    pub fn store<'a, C, E, S>(&self, store: &S) -> Result<LongHkey, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + ?Sized + 'a,
    {
        match store.put(self.to_string().as_bytes())? {
            Hkey::Encrypted(hash, key) => LongHkey::from_hash_and_key(hash, key),
            _ => Err(PsHkeyError::StorageError)?,
        }
        .ok()
    }
}
