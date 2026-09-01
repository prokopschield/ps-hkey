use ps_datachunk::{DataChunk, DataChunkError};

use crate::{Hkey, HkeyError, LongHkey, LongHkeyExpanded, Store, MAX_SIZE_RAW};

impl LongHkeyExpanded {
    /// Stores this node's manifest, returning a [`Hkey::LongHkey`] referencing it.
    ///
    /// A node whose content fits inline is never stored as a [`LongHkey`]; its content is
    /// returned as [`Hkey::Raw`] instead.
    pub fn store<'a, C, E, S>(&self, store: &'a S) -> Result<Hkey, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        if self.size() <= MAX_SIZE_RAW {
            let content = self.resolve_slice(store, 0..self.size())?;

            return Hkey::from_raw(&content)
                .map_err(HkeyError::Construction)
                .map_err(Into::into);
        }

        match store.put(self.to_string().as_bytes())? {
            Hkey::Encrypted(hash, key) => Ok(LongHkey::from_hash_and_key(hash, key).into()),
            _ => Err(HkeyError::Storage)?,
        }
    }
}
