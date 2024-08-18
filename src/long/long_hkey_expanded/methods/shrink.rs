use crate::{long::LongHkeyExpanded, Hkey, PsHkeyError};

impl LongHkeyExpanded {
    /// transforms this LongHkey into a Hkey::ListRef
    pub fn shrink<E, Ef, F>(&self, store: &F) -> Result<Hkey, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> Result<Hkey, Ef> + Sync,
    {
        Ok(self.store::<E, _>(&|data| Ok(store(data)?))?.into())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use ps_datachunk::{BorrowedDataChunk, Compressor, DataChunk, DataChunkTrait};
    use ps_hash::Hash;

    use crate::{long::LongHkeyExpanded, Hkey, PsHkeyError};

    #[test]
    fn valid() -> Result<(), PsHkeyError> {
        let hashmap_mutex = Arc::from(Mutex::from(HashMap::new()));
        let hashmap_mutex: Arc<Mutex<HashMap<Hash, DataChunk>>> = hashmap_mutex;

        let hashmap = || match hashmap_mutex.lock() {
            Ok(guard) => guard,
            Err(err) => err.into_inner(),
        };

        let fetch = |hash: &Hash| match hashmap().get(hash) {
            Some(chunk) => Ok(DataChunk::Owned(chunk.to_owned())),
            None => Err(PsHkeyError::StorageError),
        };

        let store = |bytes: &[u8]| {
            let chunk = BorrowedDataChunk::from_data(bytes);
            let encrypted = chunk.encrypt(&Compressor::new())?;
            let hkey = Hkey::Encrypted(encrypted.chunk.hash(), encrypted.key);

            hashmap().insert(*encrypted.chunk.hash(), DataChunk::Owned(encrypted.chunk));

            Ok::<Hkey, PsHkeyError>(hkey)
        };

        let orig_data = [18u8; 10000];

        assert_eq!(store(&orig_data)?.to_string(), "EzaRmmkB_vyrxmFbWnW~WDhN~jgkrelfn3XJQJ0Q87J5yJ4GAcQfL0QThpFZytTgL1_tWUtek6jq29BkamrOnDVckrTkqFAwVb~O2");

        let lhkey = LongHkeyExpanded::default().update::<PsHkeyError, _, _, _, _>(
            &fetch,
            &store,
            &orig_data,
            0..orig_data.len(),
        )?;

        let hkey = lhkey.shrink::<PsHkeyError, _, _>(&store)?;

        assert_eq!(hkey.to_string(), "LlCIB2mY72F5wc2s4LjxCG_T87SjDxgq4y6iRvamoEUHCQCMeHRxeVJgQ2iDJA~7QWJ3ZxLSpIxHE3YggY6E_eEQ~PvXy3tr8mloB");

        let data = hkey.resolve_slice(&fetch, 0..10000)?;

        assert_eq!(
            &data[..],
            &orig_data[..],
            "Fetched data should match stored data"
        );

        Ok(())
    }
}
