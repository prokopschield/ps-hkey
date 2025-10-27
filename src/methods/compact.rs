use ps_base64::base64;
use ps_hash::{Hash, HASH_SIZE_COMPACT};

use crate::{Hkey, Store};

impl Hkey {
    pub fn compact<S: Store>(&self, store: &S) -> Result<Vec<u8>, S::Error> {
        match self.shrink(store)? {
            Self::Raw(value) => Ok(value.to_vec()),
            Self::Base64(value) => Ok(base64::decode(value.as_bytes())),
            Self::Direct(hash) => Ok(hash.compact().to_vec()),
            Self::Encrypted(hash, key) => Ok(compact_dhash(&hash, &key, 0)),
            Self::ListRef(hash, key) => Ok(compact_dhash(&hash, &key, 1)),
            Self::LongHkey(lhkey) => Ok(compact_dhash(lhkey.hash_ref(), lhkey.key_ref(), 1)),
            hkey => hkey.shrink(store)?.compact(store),
        }
    }
}

pub fn compact_dhash(a: &Hash, b: &Hash, flag: u8) -> Vec<u8> {
    let mut double = Vec::with_capacity(HASH_SIZE_COMPACT * 2);

    double.extend_from_slice(a.compact());
    double.extend_from_slice(b.compact());

    double[0] &= 0xFE;
    double[0] ^= flag;

    double
}

#[cfg(test)]
mod tests {

    // Assuming InMemoryStore implements the Store trait with methods like:
    // - insert(&mut self, data: Vec<u8>) -> Hash (computes and returns the hash)
    // - get(&self, hash: &Hash) -> Option<Vec<u8>>
    // Hash is a type in the crate with a static method Hash::hash(data: &[u8]) -> Hash.

    use std::{error::Error, sync::Arc};

    use ps_hash::Hash;

    use crate::{Hkey, InMemoryStore, LongHkeyExpanded, Store};

    #[test]
    fn test_raw_variant_roundtrip() {
        let store = InMemoryStore::default();
        let data = b"Hello, world!".to_vec();
        let hkey = Hkey::Raw(data.as_slice().into());

        let compact = hkey.compact(&store).unwrap();
        let restored = Hkey::from_compact(&compact).unwrap();

        assert_eq!(hkey, restored);
        // Verify inner data.
        if let Hkey::Raw(raw_data) = &restored {
            assert_eq!(raw_data.as_ref(), data.as_slice());
        } else {
            panic!("Expected Raw variant");
        }
    }

    #[test]
    fn test_base64_variant_roundtrip() {
        let store = InMemoryStore::default();
        let data = "SGVsbG8gd29ybGQh"; // Base64 for "Hello world!"
        let hkey = Hkey::Base64(data.into());

        let compact = hkey.compact(&store).unwrap();
        let restored = Hkey::from_compact(&compact).unwrap();

        assert_eq!(hkey.to_string(), restored.to_string());
    }

    #[test]
    fn test_direct_variant_roundtrip() {
        let store = InMemoryStore::default();

        let hkey = Hkey::Direct(Hash::hash(b"Hello, world!").unwrap().into());

        let compact = hkey.compact(&store).unwrap();
        let restored = Hkey::from_compact(&compact).unwrap();

        assert_eq!(hkey, restored);
        // Verify the hash matches.
        if let Hkey::Direct(h) = &restored {
            assert_eq!(h.to_string(), hkey.to_string());
        } else {
            panic!("Expected Direct variant");
        }
    }

    #[test]
    fn test_encrypted_variant_roundtrip() -> Result<(), Box<dyn Error>> {
        let store = InMemoryStore::default();
        let data = b"Encrypted data".repeat(20);
        let hkey = store.put(&data).unwrap();

        let Hkey::Encrypted(hash, key) = &hkey else {
            panic!("Expected an Encrypted Hkey");
        };

        let compact = hkey.compact(&store).unwrap();
        let restored = Hkey::from_compact(&compact).unwrap();

        assert_eq!(hkey, restored);
        // Verify hashes.
        if let Hkey::Encrypted(data_h, key_h) = &restored {
            assert_eq!(data_h.as_ref(), hash.as_ref());
            assert_eq!(key_h.as_ref(), key.as_ref());
            Ok(())
        } else {
            panic!("Expected Encrypted variant");
        }
    }

    #[test]
    fn test_listref_variant_roundtrip() {
        let store = InMemoryStore::default();
        let data = b"List ref data".repeat(2000);
        let hkey = Hkey::parse(store.put(&data).unwrap().to_string());

        let Hkey::ListRef(data_hash, key_hash) = &hkey else {
            panic!("Expected Hkey::ListRef, got {hkey:?}");
        };

        let compact = hkey.compact(&store).unwrap();
        let restored = Hkey::from_compact(&compact).unwrap();

        assert_eq!(hkey, restored);
        // Verify hashes.
        if let Hkey::ListRef(data_h, key_h) = &restored {
            assert_eq!(data_h.as_ref(), data_hash.as_ref());
            assert_eq!(key_h.as_ref(), key_hash.as_ref());
        } else {
            panic!("Expected ListRef variant");
        }
    }

    #[test]
    fn test_longhkeyexpanded_variant_roundtrip() -> Result<(), Box<dyn Error>> {
        let store = InMemoryStore::default();

        let mock_long_expanded = Arc::new(LongHkeyExpanded::default()).update(
            &store,
            &b"Hello, world".repeat(200),
            0..5000,
        )?;

        let hkey = Hkey::LongHkeyExpanded(mock_long_expanded.clone()).shrink(&store)?;

        let compact = hkey.compact(&store).unwrap();
        let restored = Hkey::from_compact(&compact).unwrap();

        assert_eq!(hkey.to_string(), restored.to_string());

        Ok(())
    }

    #[test]
    fn test_from_compact_invalid_data() {
        let invalid_compact = b"This represents an invalid compact Hash :)".to_vec();

        let result = Hkey::from_compact(&invalid_compact);

        assert!(result.is_err());
    }

    #[test]
    fn test_compact_empty_hkey() {
        // Assuming there's no empty Hkey, but test Raw with empty data.
        let store = InMemoryStore::default();
        let empty_data = vec![];
        let hkey = Hkey::Raw(empty_data.into());

        let compact = hkey.compact(&store).unwrap();

        let restored = Hkey::from_compact(&compact).unwrap();
        assert_eq!(hkey, restored);
    }

    #[test]
    fn test_direct_missing_in_store() {
        let store = InMemoryStore::default();
        let missing_hash = Hash::hash(b"missing").unwrap();
        let hkey = Hkey::Direct(Arc::new(missing_hash));

        // compact should succeed even if not in store, as it just stores the hash.
        let compact = hkey.compact(&store).unwrap();
        let restored = Hkey::from_compact(&compact).unwrap();

        assert_eq!(hkey, restored);
        // Note: Actual data retrieval would fail if used, but compact/from_compact just handles the key.
    }

    // Additional tests for edge cases, like very large lists, but keep concise.
    // For performance, avoid actual large data in unit tests.
}
