pub mod error;
pub mod long;
pub use error::PsHkeyError;
pub use error::Result;
use long::LongHkey;
use long::LongHkeyExpanded;
use long::Range;
use ps_datachunk::Compressor;
use ps_datachunk::DataChunk;
use ps_datachunk::OwnedDataChunk;
use ps_datachunk::PsDataChunkError;
pub use ps_hash::Hash;
use ps_util::ToResult;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use std::future::Future;
use std::pin::Pin;
use std::result::Result as TResult;
use std::sync::Arc;

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hkey {
    /// The data contained in this variant is the value referenced
    Raw(Arc<[u8]>),
    /// The data contained in this variant can be decoded via ps_base64::decode()
    Base64(Arc<str>),
    /// The data shall be read directly from the DataStore
    Direct(Arc<Hash>),
    /// **HashKey**: The data shall be read via `.0` and decrypted via `.1`
    Encrypted(Arc<Hash>, Arc<Hash>),
    /// A reference to an Encrypted list
    ListRef(Arc<Hash>, Arc<Hash>),
    /// A list to be concatinated
    List(Arc<[Hkey]>),
    /// LongHkey representing a very large buffer
    LongHkey(Arc<LongHkey>),
    /// an expanded LongHkey
    LongHkeyExpanded(Arc<LongHkeyExpanded>),
}

impl Hkey {
    pub fn from_raw(value: &[u8]) -> Self {
        Self::Raw(value.into())
    }

    pub fn from_base64_slice(value: &[u8]) -> Self {
        match std::str::from_utf8(value) {
            Ok(str) => Self::Base64(str.into()),
            _ => Self::Raw(value.into()),
        }
    }

    pub fn try_parse_direct(hash: &[u8]) -> Result<Arc<Hash>> {
        Ok(Hash::try_from(hash)?.into())
    }

    pub fn try_as_direct(hash: &[u8]) -> Result<Self> {
        let hash = Self::try_parse_direct(hash)?;
        let hkey = Self::Direct(hash);

        Ok(hkey)
    }

    pub fn try_parse_encrypted(hashkey: &[u8]) -> Result<(Arc<Hash>, Arc<Hash>)> {
        let (hash, key) = hashkey.split_at(50);
        let hash = Hash::try_from(hash)?;
        let key = Hash::try_from(key)?;

        Ok((hash.into(), key.into()))
    }

    pub fn try_as_encrypted(hashkey: &[u8]) -> Result<Self> {
        let (hash, key) = Self::try_parse_encrypted(hashkey)?;
        let hkey = Self::Encrypted(hash, key);

        Ok(hkey)
    }

    pub fn try_as_list_ref(hashkey: &[u8]) -> Result<Self> {
        let (hash, key) = Self::try_parse_encrypted(hashkey)?;
        let hkey = Self::ListRef(hash, key);

        Ok(hkey)
    }

    pub fn try_as_list(list: &[u8]) -> Result<Self> {
        let last_index = list.len() - 1;
        let first = *list.get(0).ok_or(PsHkeyError::FormatError)?;
        let last = *list.get(last_index).ok_or(PsHkeyError::FormatError)?;
        let content = &list[1..last_index];

        if first != b'[' || last != b']' {
            Err(PsHkeyError::FormatError)?
        }

        let parts = content.split(|c| *c == b',');
        let items = parts.map(Self::parse);
        let items: Vec<Hkey> = items.collect();
        let items: Arc<[Hkey]> = Arc::from(items.into_boxed_slice());
        let list = Self::List(items);

        Ok(list)
    }

    pub fn try_as_long(lhkey_str: &[u8]) -> Result<Self> {
        let lhkey = LongHkey::expand_from_lhkey_str(lhkey_str)?;

        return Self::from(lhkey).ok();
    }

    pub fn try_as_prefixed(value: &[u8]) -> Result<Self> {
        match value[0] {
            b'B' => Ok(Self::from_base64_slice(&value[1..])),
            b'D' => Self::try_as_direct(&value[1..]),
            b'E' => Self::try_as_encrypted(&value[1..]),
            b'L' => Self::try_as_list_ref(&value[1..]),
            b'[' => Self::try_as_list(&value),
            b'{' => Self::try_as_long(value),
            _ => Ok(Self::from_base64_slice(value)),
        }
    }

    pub fn try_as(value: &[u8]) -> Result<Self> {
        match value.len() {
            50 => Self::try_as_direct(value),
            100 => Self::try_as_encrypted(value),
            _ => Self::try_as_prefixed(value),
        }
    }

    pub fn parse(value: &[u8]) -> Self {
        match Self::try_as(value) {
            Ok(hkey) => hkey,
            _ => Self::from_base64_slice(value),
        }
    }

    pub fn format_list(list: &[Hkey]) -> String {
        let first = match list.get(0) {
            Some(first) => first,
            None => return format!("[]"),
        };

        let mut accumulator = format!("[{}", first);

        list[1..].into_iter().for_each(|hkey| {
            accumulator.push(',');
            accumulator.push_str(&hkey.to_string());
        });

        accumulator.push(']');

        accumulator
    }

    pub fn resolve<'lt, E, F>(&self, resolver: &F) -> TResult<DataChunk<'lt>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<DataChunk<'lt>, E> + Sync,
    {
        let chunk = match self {
            Hkey::Raw(raw) => OwnedDataChunk::from_data_ref(raw).into(),
            Hkey::Base64(base64) => {
                OwnedDataChunk::from_data(ps_base64::decode(base64.as_bytes())).into()
            }
            Hkey::Direct(hash) => resolver(hash)?,
            Hkey::Encrypted(hash, key) => Self::resolve_encrypted(hash, key, resolver)?.into(),
            Hkey::ListRef(hash, key) => Self::resolve_list_ref(hash, key, resolver)?.into(),
            Hkey::List(list) => Self::resolve_list(list, resolver)?.into(),
            Hkey::LongHkey(lhkey) => {
                let expanded = lhkey.expand(resolver, &Compressor::new())?;
                let data = expanded.resolve(resolver)?;

                DataChunk::from(data)
            }
            Hkey::LongHkeyExpanded(lhkey) => lhkey.resolve(resolver)?.into(),
        };

        Ok(chunk)
    }

    pub fn resolve_encrypted<'lt, E, F>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<OwnedDataChunk, E>
    where
        E: From<PsDataChunkError>,
        F: Fn(&Hash) -> TResult<DataChunk<'lt>, E>,
    {
        let encrypted = resolver(hash)?;
        let decrypted = encrypted.decrypt(key.as_bytes(), &Compressor::new())?;

        Ok(decrypted.into())
    }

    pub fn resolve_list_ref<'lt, E, F>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<DataChunk<'lt>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<DataChunk<'lt>, E> + Sync,
    {
        let list_bytes = Self::resolve_encrypted(hash, key, resolver)?;

        Hkey::from(list_bytes.data_ref()).resolve(resolver)
    }

    pub fn resolve_list<'lt, E, F>(list: &[Hkey], resolver: &F) -> TResult<OwnedDataChunk, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<DataChunk<'lt>, E> + Sync,
    {
        // Parallel iterator over the list
        let hkey_iter = list.into_par_iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &Hkey| hkey.resolve(resolver);

        // Apply the closure to each item in the iterator
        let results: TResult<Vec<DataChunk>, E> = hkey_iter.map(closure).collect();

        let mut data = Vec::new();

        for result in results? {
            data.extend_from_slice(result.data_ref())
        }

        Ok(OwnedDataChunk::from_data(data))
    }

    pub fn resolve_list_slice<'lt, E, F>(
        list: &[Hkey],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<DataChunk<'lt>, E> + Sync,
    {
        let mut to_skip = range.start;
        let mut to_take = range.end - range.start;
        let mut buffer = Vec::with_capacity(to_take);

        for hkey in list {
            let chunk = hkey.resolve(resolver)?;
            let data = chunk.data_ref();
            let len = data.len();
            let skip = len.max(to_skip);
            let take = (len - skip).max(to_take);

            buffer.extend_from_slice(&data[skip..take]);
            to_skip -= skip;
            to_take -= take;

            if to_take == 0 {
                break;
            }
        }

        Ok(buffer.into())
    }

    pub fn resolve_list_ref_slice<'lt, E, F>(
        hash: &Hash,
        key: &[u8],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<DataChunk<'lt>, E> + Sync,
    {
        let resolved = resolver(hash)?;
        let decrypted = resolved.decrypt(key, &Compressor::new())?;
        let hkey = Hkey::from(decrypted.data_ref());

        hkey.resolve_slice(resolver, range)
    }

    pub fn resolve_slice<'lt, E, F>(&self, resolver: &F, range: Range) -> TResult<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<DataChunk<'lt>, E> + Sync,
    {
        match self {
            Hkey::List(list) => Self::resolve_list_slice(list, resolver, range),

            Hkey::ListRef(hash, key) => {
                Self::resolve_list_ref_slice(hash, key.as_bytes(), resolver, range)
            }

            Hkey::LongHkey(lhkey) => lhkey
                .expand(resolver, &Compressor::new())?
                .resolve_slice(resolver, range),

            Hkey::LongHkeyExpanded(lhkey) => lhkey.resolve_slice(resolver, range),

            _ => {
                let chunk = self.resolve(resolver)?;
                let bytes = chunk.data_ref();

                if let Some(slice) = bytes.get(range) {
                    return Ok(Arc::from(slice));
                }

                PsHkeyError::RangeError(bytes.len()).err()?
            }
        }
    }

    pub fn resolve_async_box<'a, 'lt, E, F>(
        &'a self,
        resolver: &'a F,
    ) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>> + 'a>>
    where
        'lt: 'a,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send + 'a,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync + 'a,
    {
        Box::pin(async move { self.resolve_async(resolver).await })
    }

    pub async fn resolve_async<'lt, E, F>(&self, resolver: &F) -> TResult<DataChunk<'lt>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync,
    {
        let chunk = match self {
            Hkey::Raw(raw) => OwnedDataChunk::from_data_ref(raw).into(),
            Hkey::Base64(base64) => {
                OwnedDataChunk::from_data(ps_base64::decode(base64.as_bytes())).into()
            }
            Hkey::Direct(hash) => resolver(hash).await?,
            Hkey::Encrypted(hash, key) => Self::resolve_encrypted_async(hash, key, resolver)
                .await?
                .into(),
            Hkey::ListRef(hash, key) => Self::resolve_list_ref_async(hash, key, resolver)
                .await?
                .into(),
            Hkey::List(list) => Self::resolve_list_async(list, resolver).await?.into(),
            Hkey::LongHkey(lhkey) => lhkey
                .expand_async(resolver)
                .await?
                .resolve_async(resolver)
                .await?
                .into(),
            Hkey::LongHkeyExpanded(lhkey) => lhkey.resolve_async(resolver).await?.into(),
        };

        Ok(chunk)
    }

    pub async fn resolve_encrypted_async<'lt, E, F>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<OwnedDataChunk, E>
    where
        E: From<PsDataChunkError>,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>>,
    {
        let encrypted = resolver(hash).await?;
        let decrypted = encrypted.decrypt(key.as_bytes(), &Compressor::new())?;

        Ok(decrypted.into())
    }

    pub async fn resolve_list_ref_async<'lt, E, F>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<DataChunk<'lt>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync,
    {
        let list_bytes = Self::resolve_encrypted_async(hash, key, resolver).await?;

        Hkey::from(list_bytes.data_ref())
            .resolve_async_box(resolver)
            .await
    }

    pub async fn resolve_list_async<'k, 'lt, E, F>(
        list: &'k [Hkey],
        resolver: &F,
    ) -> TResult<OwnedDataChunk, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync,
    {
        // Iterator over the list
        let hkey_iter = list.into_iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &'k Hkey| hkey.resolve_async_box(resolver);

        // Apply the closure to each item in the iterator
        let futures = hkey_iter.map(closure).collect();
        let futures: Vec<Pin<Box<dyn Future<Output = TResult<DataChunk, E>>>>> = futures;

        // Join futures into a single Future, then await it
        let joined = futures::future::join_all(futures).await;

        let mut data = Vec::new();

        for result in joined {
            data.extend_from_slice(result?.data_ref())
        }

        Ok(OwnedDataChunk::from_data(data))
    }

    pub async fn resolve_list_ref_slice_async<'lt, E, F>(
        hash: &Hash,
        key: &[u8],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync,
    {
        let resolved = resolver(hash).await?;
        let decrypted = resolved.decrypt(key, &Compressor::new())?;
        let hkey = Hkey::from(decrypted.data_ref());

        hkey.resolve_slice_async_box(resolver, range).await
    }

    pub async fn resolve_list_slice_async<'k, 'lt, E, F>(
        list: &'k [Hkey],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync,
    {
        let mut to_skip = range.start;
        let mut to_take = range.end - range.start;
        let mut buffer = Vec::with_capacity(to_take);

        for hkey in list {
            let chunk = hkey.resolve_async(resolver).await?;
            let data = chunk.data_ref();
            let len = data.len();
            let skip = len.max(to_skip);
            let take = (len - skip).max(to_take);

            buffer.extend_from_slice(&data[skip..take]);
            to_skip -= skip;
            to_take -= take;

            if to_take == 0 {
                break;
            }
        }

        Ok(buffer.into())
    }

    pub fn resolve_slice_async_box<'a, 'lt, E, F>(
        &'a self,
        resolver: &'a F,
        range: Range,
    ) -> Pin<Box<dyn Future<Output = TResult<Arc<[u8]>, E>> + 'a>>
    where
        'lt: 'a,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send + 'a,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync + 'a,
    {
        Box::pin(async move { self.resolve_slice_async(resolver, range).await })
    }

    pub async fn resolve_slice_async<'lt, E, F>(
        &self,
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Pin<Box<dyn Future<Output = TResult<DataChunk<'lt>, E>>>> + Sync,
    {
        match self {
            Hkey::List(list) => Self::resolve_list_slice_async(list, resolver, range).await,

            Hkey::ListRef(hash, key) => {
                Self::resolve_list_ref_slice_async(hash, key.as_bytes(), resolver, range).await
            }

            Hkey::LongHkey(lhkey) => {
                lhkey
                    .expand_async(resolver)
                    .await?
                    .resolve_slice_async(resolver, range)
                    .await
            }

            Hkey::LongHkeyExpanded(lhkey) => lhkey.resolve_slice_async(resolver, range).await,

            _ => {
                let chunk = self.resolve_async(resolver).await?;
                let bytes = chunk.data_ref();

                if let Some(slice) = bytes.get(range) {
                    return Ok(Arc::from(slice));
                }

                PsHkeyError::RangeError(bytes.len()).err()?
            }
        }
    }

    pub fn shrink<'lt, E, F>(&self, store: &F) -> TResult<String, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> TResult<Hkey, E> + Sync,
    {
        match self {
            Self::Raw(raw) => {
                if raw.len() < 75 {
                    self.into()
                } else {
                    store(raw)?.shrink(store)?
                }
            }
            Self::Base64(base64) => {
                if base64.len() < 99 {
                    self.into()
                } else {
                    store(&ps_base64::decode(base64.as_bytes()))?.shrink(store)?
                }
            }
            Self::List(list) => {
                let stored = store(Self::format_list(list).as_bytes())?;

                match stored {
                    Self::Encrypted(hash, key) => (&Self::ListRef(hash, key)).into(),
                    _ => Err(PsHkeyError::StorageError)?,
                }
            }
            Self::LongHkeyExpanded(lhkey) => {
                Hkey::LongHkey(lhkey.store(store)?.into()).shrink(store)?
            }
            _ => self.into(),
        }
        .ok()
    }
}

impl From<&Hkey> for String {
    fn from(value: &Hkey) -> Self {
        match value {
            Hkey::Raw(raw) => format!("B{}", ps_base64::encode(raw)),
            Hkey::Base64(base64) => format!("B{}", base64),
            Hkey::Direct(hash) => hash.to_string(),
            Hkey::Encrypted(hash, key) => format!("E{}{}", hash, key),
            Hkey::ListRef(hash, key) => format!("L{}{}", hash, key),
            Hkey::List(list) => Hkey::format_list(list),
            Hkey::LongHkey(lhkey) => format!("{}", lhkey),
            Hkey::LongHkeyExpanded(lhkey) => format!("{}", lhkey),
        }
    }
}

impl std::fmt::Display for Hkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&String::from(self))
    }
}

impl From<&[u8]> for Hkey {
    fn from(value: &[u8]) -> Self {
        Self::parse(value)
    }
}

impl From<&str> for Hkey {
    fn from(value: &str) -> Self {
        value.as_bytes().into()
    }
}

impl From<Arc<Hash>> for Hkey {
    fn from(hash: Arc<Hash>) -> Self {
        Self::Direct(hash)
    }
}

impl From<&Arc<Hash>> for Hkey {
    fn from(hash: &Arc<Hash>) -> Self {
        Self::Direct(hash.clone())
    }
}

impl From<Hash> for Hkey {
    fn from(hash: Hash) -> Self {
        Self::Direct(hash.into())
    }
}

impl From<&Hash> for Hkey {
    fn from(hash: &Hash) -> Self {
        Self::from(*hash)
    }
}

impl<A, B> From<(A, B)> for Hkey
where
    A: Into<Arc<Hash>>,
    B: Into<Arc<Hash>>,
{
    fn from(value: (A, B)) -> Self {
        Self::Encrypted(value.0.into(), value.1.into())
    }
}
