use crate::Hkey;

impl Hkey {
    /// Returns whether `self` references empty data, without accessing a store.
    ///
    /// Returns `None` if emptiness cannot be determined locally:
    /// [`ListRef`](Self::ListRef) and [`LongHkey`](Self::LongHkey) do not carry
    /// the length of the data they reference.
    #[must_use]
    pub fn is_empty(&self) -> Option<bool> {
        match self {
            Self::Empty => Some(true),
            Self::Raw(bytes) => Some(bytes.is_empty()),
            Self::Base64(str) => Some(str.is_empty()),
            Self::Direct(hash) => Some(hash.data_max_len().to_usize() == 0),
            Self::Encrypted(_, key) => Some(key.data_max_len().to_usize() == 0),
            Self::ListRef(_, _) | Self::LongHkey(_) => None,
            Self::List(list) => {
                let mut empty = Some(true);

                for hkey in list.iter() {
                    match hkey.is_empty() {
                        Some(true) => {}
                        Some(false) => return Some(false),
                        None => empty = None,
                    }
                }

                empty
            }
            Self::LongHkeyExpanded(lhkey) => Some(lhkey.size() == 0),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use ps_hash::Hash;

    use crate::{Hkey, InMemoryStore, LongHkey, LongHkeyExpanded, Store};

    #[test]
    fn empty_variant_is_empty() {
        assert_eq!(Hkey::Empty.is_empty(), Some(true));
    }

    #[test]
    fn raw_of_empty_data_is_empty() {
        let hkey = Hkey::from_raw(b"").expect("Failed to allocate Hkey::Raw");

        assert_eq!(hkey.is_empty(), Some(true));
    }

    #[test]
    fn raw_of_non_empty_data_is_not_empty() {
        let hkey = Hkey::from_raw(b"data").expect("Failed to allocate Hkey::Raw");

        assert_eq!(hkey.is_empty(), Some(false));
    }

    #[test]
    fn base64_of_empty_string_is_empty() {
        let hkey = Hkey::from_base64_slice("").expect("Failed to allocate Hkey::Base64");

        assert_eq!(hkey.is_empty(), Some(true));
    }

    #[test]
    fn base64_of_non_empty_string_is_not_empty() {
        let hkey =
            Hkey::from_base64_slice("SGVsbG8gd29ybGQh").expect("Failed to allocate Hkey::Base64");

        assert_eq!(hkey.is_empty(), Some(false));
    }

    #[test]
    fn direct_of_empty_data_is_empty() {
        let hkey = Hkey::Direct(Hash::hash(b"").expect("Failed to hash data"));

        assert_eq!(hkey.is_empty(), Some(true));
    }

    #[test]
    fn direct_of_non_empty_data_is_not_empty() {
        let hkey = Hkey::Direct(Hash::hash(b"data").expect("Failed to hash data"));

        assert_eq!(hkey.is_empty(), Some(false));
    }

    #[test]
    fn encrypted_with_empty_plaintext_is_empty() {
        let hash = Hash::hash(b"ciphertext").expect("Failed to hash data");
        let key = Hash::hash(b"").expect("Failed to hash data");

        assert_eq!(Hkey::Encrypted(hash, key).is_empty(), Some(true));
    }

    #[test]
    fn encrypted_from_store_is_not_empty() {
        let store = InMemoryStore::default();
        let data = b"Encrypted data".repeat(20);

        let hkey = store.put(&data).expect("Failed to put data");

        assert!(matches!(hkey, Hkey::Encrypted(_, _)));
        assert_eq!(hkey.is_empty(), Some(false));
    }

    #[test]
    fn list_ref_is_indeterminate() {
        let hash = Hash::hash(b"hash").expect("Failed to hash data");
        let key = Hash::hash(b"key").expect("Failed to hash data");

        assert_eq!(Hkey::ListRef(hash, key).is_empty(), None);
    }

    #[test]
    fn long_hkey_is_indeterminate() {
        let hash = Hash::hash(b"hash").expect("Failed to hash data");
        let key = Hash::hash(b"key").expect("Failed to hash data");

        assert_eq!(
            Hkey::LongHkey(LongHkey::from_hash_and_key(hash, key)).is_empty(),
            None
        );
    }

    #[test]
    fn list_with_no_elements_is_empty() {
        let list: Arc<[Hkey]> = Arc::new([]);

        assert_eq!(Hkey::List(list).is_empty(), Some(true));
    }

    #[test]
    fn list_of_empty_elements_is_empty() {
        let raw = Hkey::from_raw(b"").expect("Failed to allocate Hkey::Raw");
        let list: Arc<[Hkey]> = Arc::new([Hkey::Empty, raw]);

        assert_eq!(Hkey::List(list).is_empty(), Some(true));
    }

    #[test]
    fn list_with_non_empty_element_is_not_empty() {
        let raw = Hkey::from_raw(b"data").expect("Failed to allocate Hkey::Raw");
        let list: Arc<[Hkey]> = Arc::new([Hkey::Empty, raw]);

        assert_eq!(Hkey::List(list).is_empty(), Some(false));
    }

    #[test]
    fn list_with_indeterminate_element_is_indeterminate() {
        let hash = Hash::hash(b"hash").expect("Failed to hash data");
        let key = Hash::hash(b"key").expect("Failed to hash data");
        let list: Arc<[Hkey]> = Arc::new([Hkey::Empty, Hkey::ListRef(hash, key)]);

        assert_eq!(Hkey::List(list).is_empty(), None);
    }

    #[test]
    fn list_with_non_empty_element_overrides_indeterminate() {
        let hash = Hash::hash(b"hash").expect("Failed to hash data");
        let key = Hash::hash(b"key").expect("Failed to hash data");
        let raw = Hkey::from_raw(b"data").expect("Failed to allocate Hkey::Raw");
        let list: Arc<[Hkey]> = Arc::new([Hkey::ListRef(hash, key), raw]);

        assert_eq!(Hkey::List(list).is_empty(), Some(false));
    }

    #[test]
    fn long_hkey_expanded_default_is_empty() {
        assert_eq!(
            Hkey::LongHkeyExpanded(LongHkeyExpanded::default()).is_empty(),
            Some(true)
        );
    }

    #[test]
    fn long_hkey_expanded_with_data_is_not_empty() {
        let store = InMemoryStore::default();
        let data = b"Hello, world".repeat(200);

        let lhkey = LongHkeyExpanded::default()
            .update(&store, &data, 0..data.len())
            .expect("Failed to update LongHkeyExpanded");

        assert_eq!(Hkey::LongHkeyExpanded(lhkey).is_empty(), Some(false));
    }
}
