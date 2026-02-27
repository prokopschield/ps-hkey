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
#[allow(clippy::expect_used)]
mod tests {

    // Assuming InMemoryStore implements the Store trait with methods like:
    // - insert(&mut self, data: Vec<u8>) -> Hash (computes and returns the hash)
    // - get(&self, hash: &Hash) -> Option<Vec<u8>>
    // Hash is a type in the crate with a static method Hash::hash(data: &[u8]) -> Hash.

    use std::sync::Arc;

    use ps_hash::Hash;

    use crate::{Hkey, InMemoryStore, LongHkeyExpanded, Store};

    #[test]
    fn test_raw_variant_roundtrip() {
        let store = InMemoryStore::default();
        let data = b"Hello, world!".to_vec();
        let hkey = Hkey::Raw(
            data.as_slice()
                .try_into()
                .expect("Failed to allocate Hkey::Raw"),
        );

        let compact = hkey.compact(&store).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        assert_eq!(hkey, restored);
        // Verify inner data.
        let raw_data = match &restored {
            Hkey::Raw(raw_data) => Some(raw_data),
            _ => None,
        }
        .expect("Expected Raw variant");
        assert_eq!(raw_data.as_ref(), data.as_slice());
    }

    #[test]
    fn test_base64_variant_roundtrip() {
        let store = InMemoryStore::default();
        let data = "SGVsbG8gd29ybGQh"; // Base64 for "Hello world!"
        let hkey = Hkey::Base64(data.try_into().expect("Failed to allocate Hkey::Base64"));

        let compact = hkey.compact(&store).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        assert_eq!(hkey.to_string(), restored.to_string());
    }

    #[test]
    fn test_direct_variant_roundtrip() {
        let store = InMemoryStore::default();

        let hkey = Hkey::Direct(Hash::hash(b"Hello, world!").expect("Failed to hash data"));

        let compact = hkey.compact(&store).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        assert_eq!(hkey, restored);
        // Verify the hash matches.
        let h = match &restored {
            Hkey::Direct(h) => Some(h),
            _ => None,
        }
        .expect("Expected Direct variant");
        assert_eq!(h.to_string(), hkey.to_string());
    }

    #[test]
    fn test_encrypted_variant_roundtrip() {
        let store = InMemoryStore::default();
        let data = b"Encrypted data".repeat(20);
        let hkey = store.put(&data).expect("Failed to put encrypted data");

        let (hash, key) = match &hkey {
            Hkey::Encrypted(hash, key) => Some((hash, key)),
            _ => None,
        }
        .expect("Expected an Encrypted Hkey");

        let compact = hkey.compact(&store).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        assert_eq!(hkey, restored);
        // Verify hashes.
        let (data_h, key_h) = match &restored {
            Hkey::Encrypted(data_h, key_h) => Some((data_h, key_h)),
            _ => None,
        }
        .expect("Expected Encrypted variant");
        assert_eq!(data_h, hash);
        assert_eq!(key_h, key);
    }

    #[test]
    fn test_listref_variant_roundtrip() {
        let store = InMemoryStore::default();
        let data = b"List ref data".repeat(2000);
        let hkey = Hkey::parse(store.put(&data).expect("Failed to put data").to_string())
            .expect("Failed to parse Hkey");

        let (data_hash, key_hash) = match &hkey {
            Hkey::ListRef(data_hash, key_hash) => Some((data_hash, key_hash)),
            _ => None,
        }
        .expect("Expected Hkey::ListRef");

        let compact = hkey.compact(&store).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        assert_eq!(hkey, restored);
        // Verify hashes.
        let (data_h, key_h) = match &restored {
            Hkey::ListRef(data_h, key_h) => Some((data_h, key_h)),
            _ => None,
        }
        .expect("Expected ListRef variant");
        assert_eq!(data_h, data_hash);
        assert_eq!(key_h, key_hash);
    }

    #[test]
    fn test_longhkeyexpanded_variant_roundtrip() {
        let store = InMemoryStore::default();

        let mock_long_expanded = Arc::new(LongHkeyExpanded::default())
            .update(&store, &b"Hello, world".repeat(200), 0..5000)
            .expect("Failed to update LongHkeyExpanded");

        let hkey = Hkey::LongHkeyExpanded(mock_long_expanded)
            .shrink(&store)
            .expect("Failed to shrink LongHkeyExpanded");

        let compact = hkey.compact(&store).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        assert_eq!(hkey.to_string(), restored.to_string());
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
        let hkey = Hkey::from_raw(&empty_data).expect("Failed to allocate Hkey::Raw");

        let compact = hkey.compact(&store).expect("Failed to compact Hkey");

        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");
        assert_eq!(hkey, restored);
    }

    #[test]
    fn test_direct_missing_in_store() {
        let store = InMemoryStore::default();
        let missing_hash = Hash::hash(b"missing").expect("Failed to hash data");
        let hkey = Hkey::Direct(missing_hash);

        // compact should succeed even if not in store, as it just stores the hash.
        let compact = hkey.compact(&store).expect("Failed to compact Hkey");
        let restored = Hkey::from_compact(&compact).expect("Failed to restore Hkey");

        assert_eq!(hkey, restored);
        // Note: Actual data retrieval would fail if used, but compact/from_compact just handles the key.
    }

    // Additional tests for edge cases, like very large lists, but keep concise.
    // For performance, avoid actual large data in unit tests.
}
