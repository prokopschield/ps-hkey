use std::{fmt::Display, sync::Arc};

use ps_datachunk::{utils::decrypt, DataChunk};
use ps_hash::Hash;
use ps_promise::PromiseRejection;
use ps_util::ToResult;

use crate::{AsyncStore, Hkey, PsHkeyError, Store};

use super::LongHkeyExpanded;

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct LongHkey {
    hash: Arc<Hash>,
    key: Arc<Hash>,
}

impl Display for LongHkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("L{}{}", self.hash, self.key))
    }
}

impl LongHkey {
    #[must_use]
    pub const fn from_hash_and_key(hash: Arc<Hash>, key: Arc<Hash>) -> Self {
        Self { hash, key }
    }

    #[must_use]
    pub fn hash(&self) -> Arc<Hash> {
        self.hash.clone()
    }

    #[must_use]
    pub fn hash_ref(&self) -> &Hash {
        &self.hash
    }

    #[must_use]
    pub fn key(&self) -> Arc<Hash> {
        self.key.clone()
    }

    #[must_use]
    pub fn key_ref(&self) -> &Hash {
        &self.key
    }

    pub fn expand_from_lhkey_str(expanded_data: &[u8]) -> Result<LongHkeyExpanded, PsHkeyError> {
        if expanded_data.len() < 6 {
            // empty array: {0;0;}
            Err(PsHkeyError::FormatError)?;
        }

        if expanded_data[0] != b'{' || expanded_data[expanded_data.len() - 1] != b'}' {
            Err(PsHkeyError::FormatError)?;
        }

        let parts_data = &expanded_data[1..expanded_data.len() - 1];
        let parts_data = std::str::from_utf8(parts_data);
        let parts_data = parts_data.map_err(PsHkeyError::from)?;

        let parts: Vec<&str> = parts_data.split(';').collect();

        if parts.len() != 3 {
            Err(PsHkeyError::FormatError)?;
        }

        let depth: u32 = parts[0].parse().map_err(PsHkeyError::from)?;
        let size: usize = parts[1].parse().map_err(PsHkeyError::from)?;

        let parts = parts[2].split(',').map(|part| {
            let (range, hkey) = part.split_once(':').ok_or(PsHkeyError::FormatError)?;
            let (start, end) = range.split_once('-').ok_or(PsHkeyError::FormatError)?;
            let start: usize = start.parse()?;
            let end: usize = end.parse()?;
            let hkey: Hkey = Hkey::from(hkey);
            #[allow(clippy::range_plus_one)]
            Ok((start..end + 1, hkey))
        });

        let parts: Result<Vec<_>, PsHkeyError> = parts.collect();
        let parts = parts?.into_boxed_slice().into();

        LongHkeyExpanded::new(depth, size, parts).ok()
    }

    #[inline]
    pub fn expand_from_lhkey_encrypted_str(
        &self,
        encrypted: &[u8],
    ) -> Result<LongHkeyExpanded, PsHkeyError> {
        let lhkey_str = decrypt(encrypted, self.key.as_bytes())?;

        Self::expand_from_lhkey_str(lhkey_str.data_ref())
    }

    #[inline]
    pub fn expand<'a, C, E, S>(&self, store: &'a S) -> Result<LongHkeyExpanded, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + ?Sized + 'a,
    {
        let encrypted = store.get(&self.hash)?;

        Self::expand_from_lhkey_encrypted_str(self, encrypted.data_ref())?.ok()
    }

    #[inline]
    pub async fn expand_async<C, E, Es, S>(&self, resolver: &S) -> Result<LongHkeyExpanded, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<Es> + From<PsHkeyError> + Send,
        Es: Into<E> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = Es> + Sync + ?Sized,
    {
        let future = resolver.get(&self.hash);
        let chunk = future.await?;
        let bytes = chunk.data_ref();

        Self::expand_from_lhkey_encrypted_str(self, bytes)?.ok()
    }
}
