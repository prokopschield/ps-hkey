#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::type_complexity)]
mod constants;
mod error;
mod long;
mod resolved;
use constants::DOUBLE_HASH_SIZE;
use constants::HASH_SIZE;
use constants::MAX_SIZE_BASE64;
use constants::MAX_SIZE_RAW;
pub use error::PsHkeyError;
pub use error::Result;
pub use long::LongHkey;
pub use long::LongHkeyExpanded;
use ps_datachunk::DataChunk;
use ps_datachunk::OwnedDataChunk;
use ps_datachunk::PsDataChunkError;
use ps_datachunk::SerializedDataChunk;
pub use ps_hash::Hash;
use ps_util::ToResult;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
pub use resolved::Resolved;
use std::future::Future;
use std::pin::Pin;
use std::result::Result as TResult;
use std::sync::Arc;

pub type Range = std::ops::Range<usize>;

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hkey {
    /// The data contained in this variant is the value referenced
    Raw(Arc<[u8]>),
    /// The data contained in this variant can be decoded via [`ps_base64::decode()`]
    Base64(Arc<str>),
    /// The data shall be read directly from the [`DataStore`]
    Direct(Arc<Hash>),
    /// **`HashKey`**: The data shall be read via `.0` and decrypted via `.1`
    Encrypted(Arc<Hash>, Arc<Hash>),
    /// A reference to an Encrypted list
    ListRef(Arc<Hash>, Arc<Hash>),
    /// A list to be concatinated
    List(Arc<[Hkey]>),
    /// [`LongHkey`] representing a very large buffer
    LongHkey(Arc<LongHkey>),
    /// an expanded [`LongHkey`]
    LongHkeyExpanded(Arc<LongHkeyExpanded>),
}

impl Hkey {
    #[must_use]
    pub fn from_raw(value: &[u8]) -> Self {
        Self::Raw(value.into())
    }

    #[must_use]
    pub fn from_base64_slice(value: &[u8]) -> Self {
        std::str::from_utf8(value)
            .map_or_else(|_| Self::Raw(value.into()), |str| Self::Base64(str.into()))
    }

    pub fn try_as_direct(hash: &[u8]) -> Result<Self> {
        let hash = Hash::try_from(hash)?.into();
        let hkey = Self::Direct(hash);

        Ok(hkey)
    }

    pub fn try_parse_encrypted(hashkey: &[u8]) -> Result<(Arc<Hash>, Arc<Hash>)> {
        let (hash, key) = hashkey.split_at(HASH_SIZE);
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
        let first_byte = *list.first().ok_or(PsHkeyError::FormatError)?;
        let last_byte = *list.get(last_index).ok_or(PsHkeyError::FormatError)?;
        let content = &list[1..last_index];

        if first_byte != b'[' || last_byte != b']' {
            Err(PsHkeyError::FormatError)?;
        }

        let parts = content.split(|c| *c == b',');
        let items = parts.map(Self::parse);
        let items: Vec<Self> = items.collect();
        let items: Arc<[Self]> = Arc::from(items.into_boxed_slice());
        let list = Self::List(items);

        Ok(list)
    }

    pub fn try_as_long(lhkey_str: &[u8]) -> Result<Self> {
        let lhkey = LongHkey::expand_from_lhkey_str(lhkey_str)?;

        Self::from(lhkey).ok()
    }

    pub fn try_as_prefixed(value: &[u8]) -> Result<Self> {
        match value[0] {
            b'B' => Ok(Self::from_base64_slice(&value[1..])),
            b'D' => Self::try_as_direct(&value[1..]),
            b'E' => Self::try_as_encrypted(&value[1..]),
            b'L' => Self::try_as_list_ref(&value[1..]),
            b'[' => Self::try_as_list(value),
            b'{' => Self::try_as_long(value),
            _ => Ok(Self::from_base64_slice(value)),
        }
    }

    pub fn try_as(value: &[u8]) -> Result<Self> {
        match value.len() {
            HASH_SIZE => Self::try_as_direct(value),
            DOUBLE_HASH_SIZE => Self::try_as_encrypted(value),
            _ => Self::try_as_prefixed(value),
        }
    }

    #[must_use]
    pub fn parse(value: &[u8]) -> Self {
        Self::try_as(value).unwrap_or_else(|_| Self::from_base64_slice(value))
    }

    #[must_use]
    pub fn format_list(list: &[Self]) -> String {
        let Some(first) = list.first() else {
            return "[]".to_string();
        };

        let mut accumulator = format!("[{first}");

        list[1..].iter().for_each(|hkey| {
            accumulator.push(',');
            accumulator.push_str(&hkey.to_string());
        });

        accumulator.push(']');

        accumulator
    }

    /// Transmutates Encrypted(Hash,Key) into ListRef(Hash,Key), leaves other variants unchanged
    pub fn encrypted_into_list_ref(self) -> Result<Self> {
        match self {
            Self::Encrypted(hash, key) => Self::ListRef(hash, key).ok(),
            hkey => PsHkeyError::EncryptedIntoListRefError(hkey).err(),
        }
    }

    pub fn resolve<C, E, F>(&self, resolver: &F) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<C, E> + Sync,
    {
        let chunk = match self {
            Self::Raw(raw) => raw.clone().into(),
            Self::Base64(base64) => {
                OwnedDataChunk::from_data(ps_base64::decode(base64.as_bytes()))?.into()
            }
            Self::Direct(hash) => Resolved::Custom(resolver(hash)?),
            Self::Encrypted(hash, key) => Self::resolve_encrypted(hash, key, resolver)?.into(),
            Self::ListRef(hash, key) => Self::resolve_list_ref(hash, key, resolver)?,
            Self::List(list) => Self::resolve_list(list, resolver)?.into(),
            Self::LongHkey(lhkey) => {
                let expanded = lhkey.expand(resolver)?;
                let data = expanded.resolve(resolver)?;

                data.into()
            }
            Self::LongHkeyExpanded(lhkey) => lhkey.resolve(resolver)?.into(),
        };

        Ok(chunk)
    }

    pub fn resolve_encrypted<C, E, F>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<SerializedDataChunk, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError>,
        F: Fn(&Hash) -> TResult<C, E>,
    {
        let encrypted = resolver(hash)?;
        let decrypted = encrypted.decrypt(key.as_bytes())?;

        Ok(decrypted)
    }

    pub fn resolve_list_ref<C, E, F>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<C, E> + Sync,
    {
        let list_bytes = Self::resolve_encrypted(hash, key, resolver)?;

        Self::from(list_bytes.data_ref()).resolve(resolver)
    }

    pub fn resolve_list<C, E, F>(list: &[Self], resolver: &F) -> TResult<OwnedDataChunk, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<C, E> + Sync,
    {
        // Parallel iterator over the list
        let hkey_iter = list.into_par_iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &Self| hkey.resolve(resolver);

        // Apply the closure to each item in the iterator
        let results: TResult<Vec<Resolved<C>>, E> = hkey_iter.map(closure).collect();

        let mut data = Vec::new();

        for result in results? {
            data.extend_from_slice(result.data_ref());
        }

        Ok(OwnedDataChunk::from_data(data)?)
    }

    pub fn resolve_list_slice<C, E, F>(
        list: &[Self],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<C, E> + Sync,
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

    pub fn resolve_list_ref_slice<C, E, F>(
        hash: &Hash,
        key: &[u8],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<C, E> + Sync,
    {
        let chunk = resolver(hash)?;
        let decrypted = chunk.decrypt(key)?;
        let hkey = Self::from(decrypted.data_ref());

        hkey.resolve_slice(resolver, range)
    }

    pub fn resolve_slice<C, E, F>(&self, resolver: &F, range: Range) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + std::marker::Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> TResult<C, E> + Sync,
    {
        match self {
            Self::List(list) => Self::resolve_list_slice(list, resolver, range),

            Self::ListRef(hash, key) => {
                Self::resolve_list_ref_slice(hash, key.as_bytes(), resolver, range)
            }

            Self::LongHkey(lhkey) => lhkey.expand(resolver)?.resolve_slice(resolver, range),

            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_slice(resolver, range),

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

    pub fn resolve_async_box<'a, 'lt, C, E, F, Ff>(
        &'a self,
        resolver: &'a F,
    ) -> Pin<Box<dyn Future<Output = TResult<Resolved<C>, E>> + Send + 'a>>
    where
        'lt: 'a,
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send + 'a,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync + 'a,
    {
        Box::pin(async move { self.resolve_async(resolver).await })
    }

    pub async fn resolve_async<'lt, C, E, F, Ff>(&self, resolver: &F) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync,
    {
        let chunk = match self {
            Self::Raw(raw) => raw.clone().into(),
            Self::Base64(base64) => {
                OwnedDataChunk::from_data(ps_base64::decode(base64.as_bytes()))?.into()
            }
            Self::Direct(hash) => Resolved::Custom(resolver(hash).await?),
            Self::Encrypted(hash, key) => Self::resolve_encrypted_async(hash, key, resolver)
                .await?
                .into(),
            Self::ListRef(hash, key) => Self::resolve_list_ref_async(hash, key, resolver).await?,
            Self::List(list) => Self::resolve_list_async(list, resolver).await?.into(),
            Self::LongHkey(lhkey) => lhkey
                .expand_async(resolver)
                .await?
                .resolve_async(resolver)
                .await?
                .into(),
            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_async(resolver).await?.into(),
        };

        Ok(chunk)
    }

    pub async fn resolve_encrypted_async<'lt, C, E, F, Ff>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<SerializedDataChunk, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError>,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send,
    {
        let encrypted = resolver(hash).await?;
        let decrypted = encrypted.decrypt(key.as_bytes())?;

        Ok(decrypted)
    }

    pub async fn resolve_list_ref_async<'lt, C, E, F, Ff>(
        hash: &Hash,
        key: &Hash,
        resolver: &F,
    ) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync,
    {
        let list_bytes = Self::resolve_encrypted_async(hash, key, resolver).await?;

        Self::from(list_bytes.data_ref())
            .resolve_async_box(resolver)
            .await
    }

    pub async fn resolve_list_async<'k, 'lt, C, E, F, Ff>(
        list: &'k [Self],
        resolver: &F,
    ) -> TResult<OwnedDataChunk, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync,
    {
        // Iterator over the list
        let hkey_iter = list.iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &'k Self| hkey.resolve_async_box(resolver);

        // Apply the closure to each item in the iterator
        let futures = hkey_iter.map(closure).collect();
        let futures: Vec<Pin<Box<dyn Future<Output = TResult<Resolved<C>, E>> + Send>>> = futures;

        // Join futures into a single Future, then await it
        let joined = futures::future::join_all(futures).await;

        let mut data = Vec::new();

        for result in joined {
            data.extend_from_slice(result?.data_ref());
        }

        Ok(OwnedDataChunk::from_data(data)?)
    }

    pub async fn resolve_list_ref_slice_async<'lt, C, E, F, Ff>(
        hash: &Hash,
        key: &[u8],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync,
    {
        let chunk = resolver(hash).await?;
        let decrypted = chunk.decrypt(key)?;
        let hkey = Self::from(decrypted.data_ref());

        hkey.resolve_slice_async_box(resolver, range).await
    }

    pub async fn resolve_list_slice_async<'k, 'lt, C, E, F, Ff>(
        list: &'k [Self],
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync,
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

    pub fn resolve_slice_async_box<'a, 'lt, C, E, F, Ff>(
        &'a self,
        resolver: &'a F,
        range: Range,
    ) -> Pin<Box<dyn Future<Output = TResult<Arc<[u8]>, E>> + Send + 'a>>
    where
        'lt: 'a,
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send + 'a,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync + 'a,
    {
        Box::pin(async move { self.resolve_slice_async(resolver, range).await })
    }

    pub async fn resolve_slice_async<'lt, C, E, F, Ff>(
        &self,
        resolver: &F,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        F: Fn(&Hash) -> Ff + Sync,
        Ff: Future<Output = TResult<C, E>> + Send + Sync,
    {
        match self {
            Self::List(list) => Self::resolve_list_slice_async(list, resolver, range).await,

            Self::ListRef(hash, key) => {
                Self::resolve_list_ref_slice_async(hash, key.as_bytes(), resolver, range).await
            }

            Self::LongHkey(lhkey) => {
                lhkey
                    .expand_async(resolver)
                    .await?
                    .resolve_slice_async(resolver, range)
                    .await
            }

            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_slice_async(resolver, range).await,

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

    pub fn shrink_or_not<E, Ef, F>(&self, store: &F) -> TResult<Option<Self>, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> TResult<Self, Ef> + Sync,
    {
        match self {
            Self::Raw(raw) => {
                if raw.len() <= MAX_SIZE_RAW {
                    None
                } else {
                    store(raw)?.shrink_into(store)?.some()
                }
            }
            Self::Base64(base64) => {
                if base64.len() <= MAX_SIZE_BASE64 {
                    None
                } else {
                    store(&ps_base64::decode(base64.as_bytes()))?
                        .shrink_into(store)?
                        .some()
                }
            }
            Self::List(list) => {
                let stored = store(Self::format_list(list).as_bytes())?;

                match stored.encrypted_into_list_ref() {
                    Ok(hkey) => Some(hkey),
                    Err(err) => Err(err)?,
                }
            }
            Self::LongHkeyExpanded(lhkey) => Self::LongHkey(lhkey.store(store)?.into()).some(),
            _ => None,
        }
        .ok()
    }

    pub async fn shrink_or_not_async<'lt, E, F>(&self, store: &F) -> TResult<Option<Self>, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> Pin<Box<dyn Future<Output = TResult<Self, E>> + Send>> + Sync,
    {
        match self {
            Self::Raw(raw) => {
                if raw.len() <= MAX_SIZE_RAW {
                    None
                } else {
                    store(raw).await?.shrink_into_async(store).await?.some()
                }
            }
            Self::Base64(base64) => {
                if base64.len() <= MAX_SIZE_BASE64 {
                    None
                } else {
                    store(&ps_base64::decode(base64.as_bytes()))
                        .await?
                        .shrink_into_async(store)
                        .await?
                        .some()
                }
            }
            Self::List(list) => {
                let stored = store(Self::format_list(list).as_bytes()).await?;

                match stored.encrypted_into_list_ref() {
                    Ok(hkey) => Some(hkey),
                    Err(err) => Err(err)?,
                }
            }
            Self::LongHkeyExpanded(lhkey) => match store(format!("{lhkey}").as_bytes()).await? {
                Self::Encrypted(hash, key) => Self::ListRef(hash, key).some(),
                _ => Err(PsHkeyError::StorageError)?,
            },
            _ => None,
        }
        .ok()
    }

    pub fn shrink_into<E, Ef, F>(self, store: &F) -> TResult<Self, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> TResult<Self, Ef> + Sync,
    {
        (self.shrink_or_not(store)?).map_or_else(|| self.ok(), ps_util::ToResult::ok)
    }

    pub async fn shrink_into_async<'lt, E, F>(self, store: &F) -> TResult<Self, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> Pin<Box<dyn Future<Output = TResult<Self, E>> + Send>> + Sync,
    {
        (self.shrink_or_not_async(store).await?).map_or_else(|| self.ok(), ps_util::ToResult::ok)
    }

    pub fn shrink<E, Ef, F>(&self, store: &F) -> TResult<Self, E>
    where
        E: From<Ef> + From<PsHkeyError> + Send,
        Ef: Into<E> + Send,
        F: Fn(&[u8]) -> TResult<Self, E> + Sync,
    {
        (self.shrink_or_not::<E, _, _>(store)?)
            .map_or_else(|| self.clone().ok(), ps_util::ToResult::ok)
    }

    pub async fn shrink_async<E, F>(&self, store: &F) -> TResult<Self, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> Pin<Box<dyn Future<Output = TResult<Self, E>> + Send>> + Sync,
    {
        (self.shrink_or_not_async(store).await?)
            .map_or_else(|| self.clone().ok(), ps_util::ToResult::ok)
    }

    pub fn shrink_to_string<E, F>(&self, store: &F) -> TResult<String, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> TResult<Self, E> + Sync,
    {
        self.shrink(store)?.to_string().ok()
    }

    pub async fn shrink_to_string_async<'lt, E, F>(&self, store: &F) -> TResult<String, E>
    where
        E: From<PsHkeyError> + Send,
        F: Fn(&[u8]) -> Pin<Box<dyn Future<Output = TResult<Self, E>> + Send>> + Sync,
    {
        self.shrink_async(store).await?.to_string().ok()
    }
}

impl From<&Hkey> for String {
    fn from(value: &Hkey) -> Self {
        match value {
            Hkey::Raw(raw) => format!("B{}", ps_base64::encode(raw)),
            Hkey::Base64(base64) => format!("B{base64}"),
            Hkey::Direct(hash) => hash.to_string(),
            Hkey::Encrypted(hash, key) => format!("E{hash}{key}"),
            Hkey::ListRef(hash, key) => format!("L{hash}{key}"),
            Hkey::List(list) => Hkey::format_list(list),
            Hkey::LongHkey(lhkey) => format!("{lhkey}"),
            Hkey::LongHkeyExpanded(lhkey) => format!("{lhkey}"),
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
