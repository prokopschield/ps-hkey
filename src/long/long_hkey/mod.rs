use std::fmt::Display;

use ps_datachunk::{utils::decrypt, DataChunk};
use ps_hash::Hash;
use ps_promise::PromiseRejection;
use ps_util::ToResult;

use crate::{AsyncStore, Hkey, HkeyError, Store};

use super::LongHkeyExpanded;

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct LongHkey {
    hash: Hash,
    key: Hash,
}

impl Display for LongHkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "L{}{}",
            self.hash.display_base64(),
            self.key.display_base64()
        ))
    }
}

impl LongHkey {
    #[must_use]
    pub const fn from_hash_and_key(hash: Hash, key: Hash) -> Self {
        Self { hash, key }
    }

    #[must_use]
    pub const fn hash(&self) -> Hash {
        self.hash
    }

    #[must_use]
    pub const fn hash_ref(&self) -> &Hash {
        &self.hash
    }

    #[must_use]
    pub const fn key(&self) -> Hash {
        self.key
    }

    #[must_use]
    pub const fn key_ref(&self) -> &Hash {
        &self.key
    }

    pub fn expand_from_lhkey_str(expanded_data: &[u8]) -> Result<LongHkeyExpanded, HkeyError> {
        if expanded_data.len() < 6 {
            // empty array: {0;0;}
            Err(HkeyError::Format)?;
        }

        if expanded_data[0] != b'{' || expanded_data[expanded_data.len() - 1] != b'}' {
            Err(HkeyError::Format)?;
        }

        let parts_data = &expanded_data[1..expanded_data.len() - 1];
        let parts_data = std::str::from_utf8(parts_data);
        let parts_data = parts_data.map_err(HkeyError::from)?;

        let parts: Vec<&str> = parts_data.split(';').collect();

        if parts.len() != 3 {
            Err(HkeyError::Format)?;
        }

        let depth: u32 = parts[0].parse().map_err(HkeyError::from)?;
        let size: usize = parts[1].parse().map_err(HkeyError::from)?;

        let parts = parts[2].split(',').filter(|part| !part.is_empty());
        let parts = parts.map(|part| {
            let (range, hkey) = part.split_once(':').ok_or(HkeyError::Format)?;
            let (start, end) = range.split_once('-').ok_or(HkeyError::Format)?;
            let start: usize = start.parse()?;
            let end: usize = end.parse()?;
            let hkey: Hkey = Hkey::parse(hkey).map_err(HkeyError::Construction)?;
            #[allow(clippy::range_plus_one)]
            Ok((start..end + 1, hkey))
        });

        let parts: Result<Vec<_>, HkeyError> = parts.collect();
        let parts = parts?.into_boxed_slice().into();

        LongHkeyExpanded::new(depth, size, parts).ok()
    }

    #[inline]
    pub fn expand_from_lhkey_encrypted_str(
        &self,
        encrypted: &[u8],
    ) -> Result<LongHkeyExpanded, HkeyError> {
        let lhkey_str = decrypt(encrypted, &self.key)?;

        Self::expand_from_lhkey_str(lhkey_str.data_ref())
    }

    #[inline]
    pub fn expand<'a, C, E, S>(&self, store: &'a S) -> Result<LongHkeyExpanded, E>
    where
        C: DataChunk,
        E: From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let encrypted = store.get(&self.hash)?;

        Self::expand_from_lhkey_encrypted_str(self, encrypted.data_ref())?.ok()
    }

    #[inline]
    pub async fn expand_async<C, E, S>(&self, resolver: S) -> Result<LongHkeyExpanded, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        let future = resolver.get(&self.hash);
        let chunk = future.await?;
        let bytes = chunk.data_ref();

        Self::expand_from_lhkey_encrypted_str(self, bytes)?.ok()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use crate::{Hkey, HkeyError, InMemoryStore, LongHkey, LongHkeyExpanded};

    #[test]
    fn expand_from_lhkey_str_accepts_empty_part_list() {
        let lhkey = LongHkey::expand_from_lhkey_str(b"{0;0;}").expect("Failed to parse");

        assert_eq!(lhkey, LongHkeyExpanded::default());
    }

    #[test]
    fn empty_lhkey_display_and_parse_are_inverse() {
        let hkey = Hkey::try_parse("{0;0;}").expect("Failed to parse");

        assert!(matches!(hkey, Hkey::LongHkeyExpanded(_)));
        assert_eq!(hkey.to_string(), "{0;0;}");
    }

    #[test]
    fn expand_from_lhkey_str_accepts_empty_parts_with_nonzero_size() {
        let lhkey = LongHkey::expand_from_lhkey_str(b"{0;5;}").expect("Failed to parse");

        assert_eq!(lhkey.size(), 5);
    }

    #[test]
    fn expand_from_lhkey_str_ignores_stray_commas() {
        let clean = LongHkey::expand_from_lhkey_str(b"{0;0;}").expect("Failed to parse");
        let stray = LongHkey::expand_from_lhkey_str(b"{0;0;,,,}").expect("Failed to parse");

        assert_eq!(clean, stray);
    }

    #[test]
    fn expand_from_lhkey_str_ignores_empty_items_between_parts() {
        let clean =
            LongHkey::expand_from_lhkey_str(b"{0;8;0-3:AAAA,4-7:BBBB}").expect("Failed to parse");
        let stray = LongHkey::expand_from_lhkey_str(b"{0;8;,0-3:AAAA,,4-7:BBBB,}")
            .expect("Failed to parse");

        assert_eq!(clean, stray);
    }

    #[test]
    fn expand_from_lhkey_str_parses_part_ranges_inclusive() {
        let parsed = LongHkey::expand_from_lhkey_str(b"{0;4;0-3:AAAA}").expect("Failed to parse");

        let hkey = Hkey::parse("AAAA").expect("Failed to parse hkey");
        let expected = LongHkeyExpanded::new(0, 4, Arc::from([(0..4, hkey)]));

        assert_eq!(parsed, expected);
    }

    #[test]
    fn lhkey_str_display_and_parse_are_inverse_for_nonempty_parts() {
        let store = InMemoryStore::default();

        let node = LongHkeyExpanded::default()
            .update(&store, &[42u8; 10000], 0..10000)
            .expect("Failed to update");

        let reparsed =
            LongHkey::expand_from_lhkey_str(node.to_string().as_bytes()).expect("Failed to parse");

        assert_eq!(node, reparsed);
    }

    #[test]
    fn expand_from_lhkey_str_rejects_missing_braces() {
        let result = LongHkey::expand_from_lhkey_str(b"0;0;ab");

        assert!(matches!(result, Err(HkeyError::Format)));
    }

    #[test]
    fn expand_from_lhkey_str_rejects_wrong_field_count() {
        let two_fields = LongHkey::expand_from_lhkey_str(b"{0;123}");
        let four_fields = LongHkey::expand_from_lhkey_str(b"{0;0;0;}");

        assert!(matches!(two_fields, Err(HkeyError::Format)));
        assert!(matches!(four_fields, Err(HkeyError::Format)));
    }

    #[test]
    fn expand_from_lhkey_str_rejects_malformed_parts() {
        let missing_colon = LongHkey::expand_from_lhkey_str(b"{0;4;0-3}");
        let missing_dash = LongHkey::expand_from_lhkey_str(b"{0;4;03:AAAA}");

        assert!(matches!(missing_colon, Err(HkeyError::Format)));
        assert!(matches!(missing_dash, Err(HkeyError::Format)));
    }

    #[test]
    fn expand_from_lhkey_str_rejects_non_numeric_header() {
        let bad_depth = LongHkey::expand_from_lhkey_str(b"{a;0;}");
        let bad_size = LongHkey::expand_from_lhkey_str(b"{0;b;}");

        assert!(matches!(bad_depth, Err(HkeyError::ParseInt(_))));
        assert!(matches!(bad_size, Err(HkeyError::ParseInt(_))));
    }
}
