use crate::{long::LongHkeyExpanded, Hkey, PsHkeyError};

impl LongHkeyExpanded {
    /// transforms this [`LongHkey`] into a [`Hkey::ListRef`]
    pub fn shrink<E, Ef, F>(&self, store: &F) -> Result<Hkey, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> Result<Hkey, Ef> + Sync,
    {
        Ok(self.store::<E, _, _>(store)?.into())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use ps_datachunk::{BorrowedDataChunk, DataChunk, OwnedDataChunk};
    use ps_hash::Hash;

    use crate::{long::LongHkeyExpanded, Hkey, PsHkeyError};

    #[test]
    fn valid() -> Result<(), PsHkeyError> {
        let hashmap_mutex = Arc::from(Mutex::from(HashMap::new()));
        let hashmap_mutex: Arc<Mutex<HashMap<Hash, OwnedDataChunk>>> = hashmap_mutex;

        let hashmap = || match hashmap_mutex.lock() {
            Ok(guard) => guard,
            Err(err) => err.into_inner(),
        };

        let fetch = |hash: &Hash| {
            hashmap().get(hash).map_or_else(
                || Err(PsHkeyError::StorageError),
                |chunk| Ok(chunk.to_owned()),
            )
        };

        let store = |bytes: &[u8]| {
            let chunk = BorrowedDataChunk::from_data(bytes)?;
            let encrypted = chunk.encrypt()?;
            let hkey = Hkey::Encrypted(encrypted.hash(), encrypted.key());
            let owned = OwnedDataChunk::from_data(encrypted.data_ref().to_vec())?;

            hashmap().insert(*encrypted.hash(), owned);

            Ok::<Hkey, PsHkeyError>(hkey)
        };

        let orig_data = [18u8; 10000];

        assert_eq!(store(&orig_data)?.to_string(), "EdU0ij2fjyx5sgfyFmASwcFrNBi_2vGwMX9lJKIjdIhKKAItc6lXhvQ2f_FPaXlx~_AeUWOwg56a32S7BGedG3247YEHGzZMexcaZsWX6GzU7BrZpjXy7mt7HhK0ak335");

        let lhkey = LongHkeyExpanded::default().update::<OwnedDataChunk, PsHkeyError, _, _, _, _>(
            &fetch,
            &store,
            &orig_data,
            0..orig_data.len(),
        )?;

        let hkey = lhkey.shrink::<PsHkeyError, _, _>(&store)?;

        assert_eq!(hkey.to_string(), "LGD4YPKEIeEu_aMyTKPfqE7BQ_i0sM~NfoRNON8EV0fSEAdsnv_niR0MHg0dzsgQHqdi11LESmbilfMKsqy~0Lq4uCbxv6S7a4cCT2ULyZ1vqAU9QGYD2pU6uX4x7edGe");

        let data = hkey.resolve_slice(&fetch, 0..10000)?;

        assert_eq!(
            &data[..],
            &orig_data[..],
            "Fetched data should match stored data"
        );

        Ok(())
    }
}
