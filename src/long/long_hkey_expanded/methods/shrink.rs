use ps_datachunk::DataChunk;

use crate::{long::LongHkeyExpanded, Hkey, HkeyError, Store};

impl LongHkeyExpanded {
    /// Stores this [`LongHkeyExpanded`] and returns the resulting [`Hkey::LongHkey`].
    pub fn shrink<'a, C, E, S>(&self, store: &S) -> Result<Hkey, E>
    where
        C: DataChunk,
        E: From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        self.store(store)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        long::LongHkeyExpanded,
        store::in_memory::{InMemoryStore, InMemoryStoreError},
        Store,
    };

    #[test]
    fn valid() -> Result<(), InMemoryStoreError> {
        let store = InMemoryStore::default();

        let orig_data = [18u8; 10000];

        assert_eq!(store.put(&orig_data)?.to_string(), "LnPrRVAzZYr-TMwLv8fpYEMLTPgIn7twlHeDa-tr44LV6AdDmMPbz2FWRzvdHjhzdb6GQPFAT31Z-0oMIrHXGJM5f35HaCdfmTBaqPRz5x46qAbNTxv1PzkKVuBivXxi3");

        let lhkey = LongHkeyExpanded::default().update(&store, &orig_data, 0..orig_data.len())?;

        let hkey = lhkey.shrink(&store)?;

        assert_eq!(hkey.to_string(), "LnPrRVAzZYr-TMwLv8fpYEMLTPgIn7twlHeDa-tr44LV6AdDmMPbz2FWRzvdHjhzdb6GQPFAT31Z-0oMIrHXGJM5f35HaCdfmTBaqPRz5x46qAbNTxv1PzkKVuBivXxi3");

        let data = hkey.resolve_slice(&store, 0..10000)?;

        assert_eq!(
            &data[..],
            &orig_data[..],
            "Fetched data should match stored data"
        );

        Ok(())
    }
}
