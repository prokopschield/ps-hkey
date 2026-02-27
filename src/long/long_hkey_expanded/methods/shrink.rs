use ps_datachunk::DataChunk;

use crate::{long::LongHkeyExpanded, Hkey, PsHkeyError, Store};

impl LongHkeyExpanded {
    /// transforms this [`LongHkey`] into a [`Hkey::ListRef`]
    pub fn shrink<'a, C, E, S>(&self, store: &S) -> Result<Hkey, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        Ok(self.store(store)?.into())
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

        assert_eq!(store.put(&orig_data)?.to_string(), "L_BnefA0gZ2e4bYAxal_QxJ4zd2CY9MfIm2s1_5j_dESsActe_TkhKvNYZXR90l7QJvkR3NtOYRe3EiaNXcZ_KdxG1PirhJWdOZ-cjIXf44bqAUczbJkRIddyNSow4iRl");

        let lhkey = LongHkeyExpanded::default().update(&store, &orig_data, 0..orig_data.len())?;

        let hkey = lhkey.shrink(&store)?;

        assert_eq!(hkey.to_string(), "L_BnefA0gZ2e4bYAxal_QxJ4zd2CY9MfIm2s1_5j_dESsActe_TkhKvNYZXR90l7QJvkR3NtOYRe3EiaNXcZ_KdxG1PirhJWdOZ-cjIXf44bqAUczbJkRIddyNSow4iRl");

        let data = hkey.resolve_slice(&store, 0..10000)?;

        assert_eq!(
            &data[..],
            &orig_data[..],
            "Fetched data should match stored data"
        );

        Ok(())
    }
}
