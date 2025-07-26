#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::type_complexity)]
mod async_store;
mod constants;
mod error;
mod long;
mod resolved;
mod store;
pub use async_store::AsyncStore;
use constants::DOUBLE_HASH_SIZE;
use constants::HASH_SIZE;
use constants::MAX_SIZE_BASE64;
use constants::MAX_SIZE_RAW;
pub use error::PsHkeyError;
pub use error::Result;
pub use long::LongHkey;
pub use long::LongHkeyExpanded;
use ps_datachunk::Bytes;
use ps_datachunk::DataChunk;
use ps_datachunk::OwnedDataChunk;
use ps_datachunk::PsDataChunkError;
use ps_datachunk::SerializedDataChunk;
pub use ps_hash::Hash;
use ps_promise::PromiseRejection;
use ps_util::ToResult;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
pub use resolved::Resolved;
use std::future::Future;
use std::pin::Pin;
use std::result::Result as TResult;
use std::sync::Arc;
pub use store::Store;

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

    pub fn resolve<'a, C, E, S>(&self, store: &'a S) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let chunk = match self {
            Self::Raw(raw) => raw.clone().into(),
            Self::Base64(base64) => {
                OwnedDataChunk::from_data(ps_base64::decode(base64.as_bytes()))?.into()
            }
            Self::Direct(hash) => Resolved::Custom(store.get(hash)?),
            Self::Encrypted(hash, key) => Self::resolve_encrypted(hash, key, store)?.into(),
            Self::ListRef(hash, key) => Self::resolve_list_ref(hash, key, store)?,
            Self::List(list) => Self::resolve_list(list, store)?.into(),
            Self::LongHkey(lhkey) => {
                let expanded = lhkey.expand(store)?;
                let data = expanded.resolve(store)?;

                data.into()
            }
            Self::LongHkeyExpanded(lhkey) => lhkey.resolve(store)?.into(),
        };

        Ok(chunk)
    }

    pub fn resolve_encrypted<'a, C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: &'a S,
    ) -> TResult<SerializedDataChunk, E>
    where
        C: DataChunk,
        E: From<PsDataChunkError>,
        S: Store<Chunk<'a> = C, Error = E>,
    {
        let encrypted = store.get(hash)?;
        let decrypted = encrypted.decrypt(key.as_bytes())?;

        Ok(decrypted)
    }

    pub fn resolve_list_ref<'a, C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: &'a S,
    ) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let list_bytes = Self::resolve_encrypted(hash, key, store)?;

        Self::from(list_bytes.data_ref()).resolve(store)
    }

    pub fn resolve_list<'a, C, E, S>(list: &[Self], store: &'a S) -> TResult<OwnedDataChunk, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        // Parallel iterator over the list
        let hkey_iter = list.into_par_iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &Self| hkey.resolve(store);

        // Apply the closure to each item in the iterator
        let results: TResult<Vec<Resolved<C>>, E> = hkey_iter.map(closure).collect();

        let mut data = Vec::new();

        for result in results? {
            data.extend_from_slice(result.data_ref());
        }

        Ok(OwnedDataChunk::from_data(data)?)
    }

    pub fn resolve_list_slice<'a, C, E, S>(
        list: &[Self],
        store: &'a S,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let mut to_skip = range.start;
        let mut to_take = range.end - range.start;
        let mut buffer = Vec::with_capacity(to_take);

        for hkey in list {
            let chunk = hkey.resolve(store)?;
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

    pub fn resolve_list_ref_slice<'a, C, E, S>(
        hash: &Hash,
        key: &[u8],
        store: &'a S,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let chunk = store.get(hash)?;
        let decrypted = chunk.decrypt(key)?;
        let hkey = Self::from(decrypted.data_ref());

        hkey.resolve_slice(store, range)
    }

    pub fn resolve_slice<'a, C, E, S>(&self, store: &'a S, range: Range) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send,
        E: From<PsDataChunkError> + From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        match self {
            Self::List(list) => Self::resolve_list_slice(list, store, range),

            Self::ListRef(hash, key) => {
                Self::resolve_list_ref_slice(hash, key.as_bytes(), store, range)
            }

            Self::LongHkey(lhkey) => lhkey.expand(store)?.resolve_slice(store, range),

            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_slice(store, range),

            _ => {
                let chunk = self.resolve(store)?;
                let bytes = chunk.data_ref();

                if let Some(slice) = bytes.get(range) {
                    return Ok(Arc::from(slice));
                }

                PsHkeyError::RangeError(bytes.len()).err()?
            }
        }
    }

    pub fn resolve_async_box<'a, C, E, S>(
        &'a self,
        store: &'a S,
    ) -> Pin<Box<dyn Future<Output = TResult<Resolved<C>, E>> + Send + 'a>>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send + 'a,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        Box::pin(async move { self.resolve_async(store).await })
    }

    pub async fn resolve_async<C, E, S>(&self, store: &S) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        let chunk = match self {
            Self::Raw(raw) => raw.clone().into(),
            Self::Base64(base64) => {
                OwnedDataChunk::from_data(ps_base64::decode(base64.as_bytes()))?.into()
            }
            Self::Direct(hash) => Resolved::Custom(store.get(hash).await?),
            Self::Encrypted(hash, key) => Self::resolve_encrypted_async(hash, key, store)
                .await?
                .into(),
            Self::ListRef(hash, key) => Self::resolve_list_ref_async(hash, key, store).await?,
            Self::List(list) => Self::resolve_list_async(list, store).await?.into(),
            Self::LongHkey(lhkey) => lhkey
                .expand_async(store)
                .await?
                .resolve_async(store)
                .await?
                .into(),
            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_async(store).await?.into(),
        };

        Ok(chunk)
    }

    pub async fn resolve_encrypted_async<C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: &S,
    ) -> TResult<SerializedDataChunk, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        let encrypted = store.get(hash).await?;
        let decrypted = encrypted.decrypt(key.as_bytes())?;

        Ok(decrypted)
    }

    pub async fn resolve_list_ref_async<C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: &S,
    ) -> TResult<Resolved<C>, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        let list_bytes = Self::resolve_encrypted_async(hash, key, store).await?;

        Self::from(list_bytes.data_ref())
            .resolve_async_box(store)
            .await
    }

    pub async fn resolve_list_async<'k, C, E, S>(
        list: &'k [Self],
        store: &S,
    ) -> TResult<OwnedDataChunk, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        // Iterator over the list
        let hkey_iter = list.iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &'k Self| hkey.resolve_async_box(store);

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

    pub async fn resolve_list_ref_slice_async<C, E, S>(
        hash: &Hash,
        key: &[u8],
        store: &S,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        let chunk = store.get(hash).await?;
        let decrypted = chunk.decrypt(key)?;
        let hkey = Self::from(decrypted.data_ref());

        hkey.resolve_slice_async_box(store, range).await
    }

    pub async fn resolve_list_slice_async<C, E, S>(
        list: &[Self],
        store: &S,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        let mut to_skip = range.start;
        let mut to_take = range.end - range.start;
        let mut buffer = Vec::with_capacity(to_take);

        for hkey in list {
            let chunk = hkey.resolve_async(store).await?;
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

    pub fn resolve_slice_async_box<'a, C, E, S>(
        &'a self,
        store: &'a S,
        range: Range,
    ) -> Pin<Box<dyn Future<Output = TResult<Arc<[u8]>, E>> + Send + 'a>>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        Box::pin(async move { self.resolve_slice_async(store, range).await })
    }

    pub async fn resolve_slice_async<C, E, S>(
        &self,
        store: &S,
        range: Range,
    ) -> TResult<Arc<[u8]>, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        match self {
            Self::List(list) => Self::resolve_list_slice_async(list, store, range).await,

            Self::ListRef(hash, key) => {
                Self::resolve_list_ref_slice_async(hash, key.as_bytes(), store, range).await
            }

            Self::LongHkey(lhkey) => {
                lhkey
                    .expand_async(store)
                    .await?
                    .resolve_slice_async(store, range)
                    .await
            }

            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_slice_async(store, range).await,

            _ => {
                let chunk = self.resolve_async(store).await?;
                let bytes = chunk.data_ref();

                if let Some(slice) = bytes.get(range) {
                    return Ok(Arc::from(slice));
                }

                PsHkeyError::RangeError(bytes.len()).err()?
            }
        }
    }

    pub fn shrink_or_not<'a, C, E, S>(&self, store: &S) -> TResult<Option<Self>, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        match self {
            Self::Raw(raw) => {
                if raw.len() <= MAX_SIZE_RAW {
                    None
                } else {
                    store.put(raw)?.shrink_into(store)?.some()
                }
            }
            Self::Base64(base64) => {
                if base64.len() <= MAX_SIZE_BASE64 {
                    None
                } else {
                    store
                        .put(&ps_base64::decode(base64.as_bytes()))?
                        .shrink_into(store)?
                        .some()
                }
            }
            Self::List(list) => {
                let stored = store.put(Self::format_list(list).as_bytes())?;

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

    pub async fn shrink_or_not_async<C, E, S>(&self, store: &S) -> TResult<Option<Self>, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsHkeyError> + PromiseRejection,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        match self {
            Self::Raw(raw) => {
                if raw.len() <= MAX_SIZE_RAW {
                    None
                } else {
                    store
                        .put(Bytes::from_owner(raw.clone()))
                        .await?
                        .shrink_into_async(store)
                        .await?
                        .some()
                }
            }
            Self::Base64(base64) => {
                if base64.len() <= MAX_SIZE_BASE64 {
                    None
                } else {
                    store
                        .put(Bytes::from_owner(ps_base64::decode(base64.as_bytes())))
                        .await?
                        .shrink_into_async(store)
                        .await?
                        .some()
                }
            }
            Self::List(list) => {
                let stored = store
                    .put(Bytes::from_owner(Self::format_list(list)))
                    .await?;

                match stored.encrypted_into_list_ref() {
                    Ok(hkey) => Some(hkey),
                    Err(err) => Err(err)?,
                }
            }
            Self::LongHkeyExpanded(lhkey) => {
                match store.put(Bytes::from_owner(lhkey.to_string())).await? {
                    Self::Encrypted(hash, key) => Self::ListRef(hash, key).some(),
                    _ => Err(PsHkeyError::StorageError)?,
                }
            }
            _ => None,
        }
        .ok()
    }

    pub fn shrink_into<'a, C, E, S>(self, store: &S) -> TResult<Self, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        (self.shrink_or_not(store)?).map_or_else(|| self.ok(), ps_util::ToResult::ok)
    }

    pub async fn shrink_into_async<C, E, S>(self, store: &S) -> TResult<Self, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        (self.shrink_or_not_async(store).await?).map_or_else(|| self.ok(), ps_util::ToResult::ok)
    }

    pub fn shrink<'a, C, E, S>(&self, store: &S) -> TResult<Self, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        (self.shrink_or_not(store)?).map_or_else(|| self.clone().ok(), ps_util::ToResult::ok)
    }

    pub async fn shrink_async<C, E, S>(&self, store: &S) -> TResult<Self, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
    {
        (self.shrink_or_not_async(store).await?)
            .map_or_else(|| self.clone().ok(), ps_util::ToResult::ok)
    }

    pub fn shrink_to_string<'a, C, E, S>(&self, store: &S) -> TResult<String, E>
    where
        C: DataChunk,
        E: From<PsHkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        self.shrink(store)?.to_string().ok()
    }

    pub async fn shrink_to_string_async<C, E, S>(&self, store: &S) -> TResult<String, E>
    where
        C: DataChunk + Send + Unpin,
        E: From<PsHkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E> + Sync,
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
