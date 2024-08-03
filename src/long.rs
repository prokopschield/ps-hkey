use crate::Hkey;
use crate::PsHkeyError;
use ps_datachunk::Compressor;
use ps_datachunk::DataChunk;
use ps_datachunk::OwnedDataChunk;
use ps_datachunk::PsDataChunkError;
use ps_hash::Hash;
use ps_util::ToResult;
use std::fmt::Display;
use std::fmt::Write;
use std::sync::Arc;

pub type Range = std::ops::Range<usize>;

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
    pub fn expand_from_lhkey_str<'lt, E>(expanded_data: &[u8]) -> Result<LongHkeyExpanded, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
    {
        if expanded_data.len() < 6 {
            // empty array: {0;0;}
            Err(PsHkeyError::FormatError)?
        }

        if expanded_data[0] != b'{' || expanded_data[expanded_data.len() - 1] != b'}' {
            Err(PsHkeyError::FormatError)?
        }

        let parts_data = &expanded_data[1..expanded_data.len() - 1];
        let parts_data = std::str::from_utf8(parts_data);
        let parts_data = parts_data.map_err(|err| PsHkeyError::from(err))?;

        let parts: Vec<&str> = parts_data.split(';').collect();

        if parts.len() != 3 {
            Err(PsHkeyError::FormatError)?
        }

        let depth: usize = parts[0].parse().map_err(|err| PsHkeyError::from(err))?;
        let size: usize = parts[1].parse().map_err(|err| PsHkeyError::from(err))?;

        let parts = parts[2].split(',').map(|part| {
            let (range, hkey) = part.split_once(':').ok_or(PsHkeyError::FormatError)?;
            let (start, end) = range.split_once('-').ok_or(PsHkeyError::FormatError)?;
            let start: usize = start.parse()?;
            let end: usize = end.parse()?;
            let hkey: Hkey = Hkey::from(hkey);
            Ok((start..end + 1, Arc::from(hkey)))
        });

        let parts: Result<Vec<_>, PsHkeyError> = parts.collect();
        let parts = parts?.into_boxed_slice().into();

        LongHkeyExpanded::new(depth, size, parts).ok()
    }

    #[inline(always)]
    pub fn expand_from_lhkey_encrypted_str<'lt, E>(
        &self,
        encrypted: &[u8],
        compressor: &Compressor,
    ) -> Result<LongHkeyExpanded, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
    {
        let lhkey_str = OwnedDataChunk::decrypt_bytes(encrypted, self.key.as_bytes(), compressor)?;

        Self::expand_from_lhkey_str(lhkey_str.data_ref())
    }

    #[inline(always)]
    pub fn expand<'lt, E, F>(
        &self,
        resolver: &F,
        compressor: &Compressor,
    ) -> Result<LongHkeyExpanded, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Result<DataChunk<'lt>, E> + Sync,
    {
        Self::expand_from_lhkey_encrypted_str(self, resolver(&self.hash)?.data_ref(), compressor)
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct LongHkeyExpanded {
    depth: usize,
    size: usize,
    parts: Arc<[(Range, Arc<Hkey>)]>,
}

impl LongHkeyExpanded {
    pub fn new(depth: usize, size: usize, parts: Arc<[(Range, Arc<Hkey>)]>) -> Self {
        Self { depth, size, parts }
    }

    pub fn store<'lt, E, F>(&self, store: &F) -> Result<LongHkey, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> Result<Hkey, E> + Sync,
    {
        match store(format!("{}", self).as_bytes())? {
            Hkey::Encrypted(hash, key) => LongHkey { hash, key },
            _ => PsHkeyError::StorageError.err()?,
        }
        .ok()
    }
}

impl Display for LongHkeyExpanded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}{};{};", '{', self.depth, self.size))?;

        for i in 0..self.parts.len() {
            let part = &self.parts[i];

            f.write_fmt(format_args!(
                "{}{}-{}:{}",
                if i == 0 { "" } else { "," },
                part.0.start,
                part.0.end - 1,
                part.1
            ))?;
        }

        f.write_char('}')
    }
}
