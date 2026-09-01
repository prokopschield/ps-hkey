use ps_datachunk::{DataChunk, DataChunkError};

use crate::{long::LongHkeyExpanded, Hkey, HkeyError, Store};

impl LongHkeyExpanded {
    /// Stores this [`LongHkeyExpanded`] and returns the resulting [`Hkey::LongHkey`].
    ///
    /// Content that fits inline is returned as [`Hkey::Raw`] instead.
    pub fn shrink<'a, C, E, S>(&self, store: &'a S) -> Result<Hkey, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
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
        Hkey, Store,
    };

    /// A node whose content fits inline shrinks to [`Hkey::Raw`]; no [`LongHkey`] referencing it
    /// is ever constructed.
    ///
    /// [`LongHkey`]: crate::LongHkey
    #[test]
    fn empty_node_shrinks_to_raw() -> Result<(), InMemoryStoreError> {
        let store = InMemoryStore::default();

        let hkey = LongHkeyExpanded::default().shrink(&store)?;

        assert!(
            matches!(&hkey, Hkey::Raw(raw) if raw.is_empty()),
            "Expected Hkey::Raw of no bytes, got {hkey:?}"
        );

        Ok(())
    }

    /// Small content written through `update` shrinks to [`Hkey::Raw`] carrying that content.
    #[test]
    fn small_node_shrinks_to_raw() -> Result<(), InMemoryStoreError> {
        let store = InMemoryStore::default();

        let lhkey = LongHkeyExpanded::default().update(&store, &[18u8; 10], 0..10)?;

        let hkey = lhkey.shrink(&store)?;

        assert!(
            matches!(&hkey, Hkey::Raw(raw) if raw[..] == [18u8; 10]),
            "Expected Hkey::Raw of the written bytes, got {hkey:?}"
        );

        Ok(())
    }

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
