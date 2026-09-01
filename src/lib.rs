#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::type_complexity)]
mod async_store;
mod constants;
mod error;
mod long;
mod methods;
mod store;
use arrayvec::ArrayString;
use arrayvec::ArrayVec;
pub use async_store::AsyncStore;
pub use constants::*;
pub use error::HkeyBug;
pub use error::HkeyConstructionError;
pub use error::HkeyError;
pub use error::HkeyFromCompactError;
pub use error::Result;
pub use long::LongHkey;
pub use long::LongHkeyExpanded;
use ps_buffer::Buffer;
use ps_datachunk::Bytes;
use ps_datachunk::DataChunk;
use ps_datachunk::DataChunkError;
use ps_datachunk::OwnedDataChunk;
pub use ps_hash::Hash;
use ps_promise::PromiseRejection;
use ps_util::ToResult;
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use std::future::Future;
use std::pin::Pin;
use std::result::Result as TResult;
use std::sync::Arc;
pub use store::Store;

pub use crate::async_store::in_memory::InMemoryAsyncStore;
pub use crate::async_store::in_memory::InMemoryAsyncStoreError;
pub use crate::async_store::mixed::MixedStore;
pub use crate::async_store::mixed::MixedStoreError;
pub use crate::store::combined::CombinedStore;
pub use crate::store::combined::CombinedStoreError;
pub use crate::store::in_memory::InMemoryStore;
pub use crate::store::in_memory::InMemoryStoreError;

pub type Range = std::ops::Range<usize>;

#[derive(Clone, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hkey {
    /// This variant represents the empty string.
    #[default]
    Empty,
    /// The data contained in this variant is the value referenced
    Raw(ArrayVec<u8, BUF_SIZE_RAW>),
    /// The data contained in this variant can be decoded via [`ps_base64::decode()`]
    Base64(ArrayString<BUF_SIZE_BASE64>),
    /// The data shall be read directly from the [`DataStore`]
    Direct(Hash),
    /// **`HashKey`**: The data shall be read via `.0` and decrypted via `.1`
    Encrypted(Hash, Hash),
    /// A reference to an Encrypted list
    ListRef(Hash, Hash),
    /// A list to be concatinated
    List(Arc<[Self]>),
    /// [`LongHkey`] representing a very large buffer
    LongHkey(LongHkey),
    /// an expanded [`LongHkey`]
    LongHkeyExpanded(LongHkeyExpanded),
}

impl Hkey {
    pub fn from_raw(value: &[u8]) -> Result<Self, HkeyConstructionError> {
        let mut v = ArrayVec::new();

        v.try_extend_from_slice(value)
            .map_err(|_| HkeyConstructionError::TooLong)?;

        Ok(Self::Raw(v))
    }

    pub fn from_base64_slice(value: &str) -> Result<Self, HkeyConstructionError> {
        let mut v = ArrayString::new();

        v.try_push_str(value)
            .map_err(|_| HkeyConstructionError::TooLong)?;

        Ok(Self::Base64(v))
    }

    pub fn try_as_direct(hash: &[u8]) -> Result<Self> {
        let hash = Hash::try_from(hash)?;
        let hkey = Self::Direct(hash);

        Ok(hkey)
    }

    pub fn try_parse_encrypted(hashkey: &[u8]) -> Result<(Hash, Hash)> {
        let (hash, key) = hashkey.split_at(HASH_SIZE);
        let hash = Hash::try_from(hash)?;
        let key = Hash::try_from(key)?;

        Ok((hash, key))
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
        let first_byte = *list.first().ok_or(HkeyError::Format)?;
        let last_byte = *list.get(last_index).ok_or(HkeyError::Format)?;
        let content = &list[1..last_index];

        if first_byte != b'[' || last_byte != b']' {
            Err(HkeyError::Format)?;
        }

        let parts = content.split(|c| *c == b',');
        let items = parts.map(|item| Self::parse(item).map_err(Into::into));
        let items: Result<Vec<Self>> = items.collect();
        let items: Vec<Self> = items?;
        let items: Arc<[Self]> = Arc::from(items.into_boxed_slice());
        let list = Self::List(items);

        Ok(list)
    }

    pub fn try_as_long(lhkey_str: &[u8]) -> Result<Self> {
        let lhkey = LongHkey::expand_from_lhkey_str(lhkey_str)?;

        Self::from(lhkey).ok()
    }

    #[must_use]
    pub fn format_list(list: &[Self]) -> String {
        let mut accumulator = String::new();

        // Writing into a `String` cannot fail.
        let _ = Self::fmt_list(list, &mut accumulator);

        accumulator
    }

    /// Writes the textual form of `list` into `sink`.
    fn fmt_list<W: std::fmt::Write>(list: &[Self], sink: &mut W) -> std::fmt::Result {
        sink.write_char('[')?;

        let mut items = list.iter();

        if let Some(first) = items.next() {
            write!(sink, "{first}")?;
        }

        for item in items {
            write!(sink, ",{item}")?;
        }

        sink.write_char(']')
    }

    /// Transmutates Encrypted(Hash,Key) into ListRef(Hash,Key)
    /// # Errors
    /// - [`HkeyError::EncryptedIntoListRef`] if a different variant of [`Hkey`] is supplied
    pub fn encrypted_into_list_ref(self) -> Result<Self> {
        match self {
            Self::Encrypted(hash, key) => Self::ListRef(hash, key).ok(),
            hkey => HkeyError::EncryptedIntoListRef(hkey).err(),
        }
    }

    pub fn resolve<'a, C, E, S>(&self, store: &'a S) -> TResult<Bytes, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let chunk = match self {
            Self::Empty => Bytes::new(),
            Self::Raw(raw) => Bytes::from_owner(raw.clone()),
            Self::Base64(base64) => decode_base64(base64.as_bytes())?.into(),
            Self::Direct(hash) => store.get(hash)?.into_bytes(),
            Self::Encrypted(hash, key) => Self::resolve_encrypted(hash, key, store)?.into_bytes(),
            Self::ListRef(hash, key) => Self::resolve_list_ref(hash, key, store)?,
            Self::List(list) => Self::resolve_list(list, store)?.into_bytes(),
            Self::LongHkey(lhkey) => lhkey.expand(store)?.resolve(store)?,
            Self::LongHkeyExpanded(lhkey) => lhkey.resolve(store)?,
        };

        Ok(chunk)
    }

    pub fn resolve_encrypted<'a, C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: &'a S,
    ) -> TResult<OwnedDataChunk, E>
    where
        C: DataChunk,
        E: From<DataChunkError>,
        S: Store<Chunk<'a> = C, Error = E>,
    {
        let encrypted = store.get(hash)?;
        let decrypted = encrypted.decrypt(key)?;

        Ok(decrypted)
    }

    pub fn resolve_list_ref<'a, C, E, S>(hash: &Hash, key: &Hash, store: &'a S) -> TResult<Bytes, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let list_bytes = Self::resolve_encrypted(hash, key, store)?;

        Self::parse(list_bytes.data_ref())
            .map_err(HkeyError::Construction)?
            .resolve(store)
    }

    pub fn resolve_list<'a, C, E, S>(list: &[Self], store: &'a S) -> TResult<OwnedDataChunk, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        // Parallel iterator over the list
        let hkey_iter = list.into_par_iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &Self| hkey.resolve(store);

        // Apply the closure to each item in the iterator
        let results: TResult<Vec<Bytes>, E> = hkey_iter.map(closure).collect();

        let mut data = Vec::new();

        for result in results? {
            data.extend_from_slice(result.as_ref());
        }

        Ok(OwnedDataChunk::from_data(data)?)
    }

    pub fn resolve_list_slice<'a, C, E, S>(
        list: &[Self],
        store: &'a S,
        range: Range,
    ) -> TResult<Bytes, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        if range.start > range.end {
            HkeyError::InvalidRange(range.clone()).err()?;
        }

        let mut to_skip = range.start;
        let mut to_take = range.end - range.start;
        let mut buffer = Buffer::with_capacity(to_take).map_err(HkeyError::from)?;

        for hkey in list {
            if to_skip == 0 && to_take == 0 {
                break;
            }

            let data = hkey.resolve(store)?;
            let len = data.len();

            let skip = len.min(to_skip);
            let take = (len - skip).min(to_take);

            buffer
                .extend_from_slice(&data[skip..skip + take])
                .map_err(HkeyError::from)?;

            to_skip -= skip;
            to_take -= take;
        }

        if to_skip > 0 || to_take > 0 {
            HkeyError::Range(range.start - to_skip + buffer.len()).err()?;
        }

        Ok(buffer.into())
    }

    pub fn resolve_list_ref_slice<'a, C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: &'a S,
        range: Range,
    ) -> TResult<Bytes, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        let chunk = store.get(hash)?;
        let decrypted = chunk.decrypt(key)?;
        let hkey = Self::parse(decrypted.data_ref()).map_err(HkeyError::Construction)?;

        hkey.resolve_slice(store, range)
    }

    pub fn resolve_slice<'a, C, E, S>(&self, store: &'a S, range: Range) -> TResult<Bytes, E>
    where
        C: DataChunk,
        E: From<DataChunkError> + From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        if range.start > range.end {
            HkeyError::InvalidRange(range.clone()).err()?;
        }

        match self {
            Self::List(list) => Self::resolve_list_slice(list, store, range),

            Self::ListRef(hash, key) => Self::resolve_list_ref_slice(hash, key, store, range),

            Self::LongHkey(lhkey) => lhkey.expand(store)?.resolve_slice(store, range),

            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_slice(store, range),

            _ => {
                let bytes = self.resolve(store)?;

                if bytes.len() >= range.end {
                    return Ok(bytes.slice(range));
                }

                HkeyError::Range(bytes.len()).err()?
            }
        }
    }

    pub fn resolve_async_box<'a, C, E, S>(
        &'a self,
        store: S,
    ) -> Pin<Box<dyn Future<Output = TResult<Bytes, E>> + Send + 'a>>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send + 'a,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        Box::pin(async move { self.resolve_async(store).await })
    }

    pub async fn resolve_async<C, E, S>(&self, store: S) -> TResult<Bytes, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        let chunk = match self {
            Self::Empty => Bytes::new(),
            Self::Raw(raw) => Bytes::from_owner(raw.clone()),
            Self::Base64(base64) => decode_base64(base64.as_bytes())?.into(),
            Self::Direct(hash) => store.get(hash).await?.into_bytes(),
            Self::Encrypted(hash, key) => Self::resolve_encrypted_async(hash, key, store)
                .await?
                .into_bytes(),
            Self::ListRef(hash, key) => Self::resolve_list_ref_async(hash, key, store).await?,
            Self::List(list) => Self::resolve_list_async(list, store).await?,
            Self::LongHkey(lhkey) => {
                lhkey
                    .expand_async(store.clone())
                    .await?
                    .resolve_async(store)
                    .await?
            }
            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_async(store).await?,
        };

        Ok(chunk)
    }

    pub async fn resolve_encrypted_async<C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: S,
    ) -> TResult<OwnedDataChunk, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        let encrypted = store.get(hash).await?;
        let decrypted = encrypted.decrypt(key)?;

        Ok(decrypted)
    }

    pub async fn resolve_list_ref_async<C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: S,
    ) -> TResult<Bytes, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        let list_bytes = Self::resolve_encrypted_async(hash, key, store.clone()).await?;

        Self::parse(list_bytes.data_ref())
            .map_err(HkeyError::Construction)?
            .resolve_async_box(store)
            .await
    }

    pub async fn resolve_list_async<'k, C, E, S>(list: &'k [Self], store: S) -> TResult<Bytes, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        // Iterator over the list
        let hkey_iter = list.iter();

        // Closure to resolve each Hkey
        let closure = |hkey: &'k Self| hkey.resolve_async_box(store.clone());

        // Apply the closure to each item in the iterator
        let futures = hkey_iter.map(closure).collect();
        let futures: Vec<Pin<Box<dyn Future<Output = TResult<Bytes, E>> + Send>>> = futures;

        // Join futures into a single Future, then await it
        let joined = futures::future::join_all(futures).await;

        let mut data = Vec::new();

        for result in joined {
            data.extend_from_slice(result?.as_ref());
        }

        Ok(data.into())
    }

    pub async fn resolve_list_ref_slice_async<C, E, S>(
        hash: &Hash,
        key: &Hash,
        store: S,
        range: Range,
    ) -> TResult<Bytes, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        let chunk = store.get(hash).await?;
        let decrypted = chunk.decrypt(key)?;
        let hkey = Self::parse(decrypted.data_ref()).map_err(HkeyError::Construction)?;

        hkey.resolve_slice_async_box(store, range).await
    }

    pub async fn resolve_list_slice_async<C, E, S>(
        list: &[Self],
        store: S,
        range: Range,
    ) -> TResult<Bytes, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        if range.start > range.end {
            HkeyError::InvalidRange(range.clone()).err()?;
        }

        let mut to_skip = range.start;
        let mut to_take = range.end - range.start;
        let mut buffer = Buffer::with_capacity(to_take).map_err(HkeyError::from)?;

        for hkey in list {
            if to_skip == 0 && to_take == 0 {
                break;
            }

            let data = hkey.resolve_async(store.clone()).await?;
            let len = data.len();

            let skip = len.min(to_skip);
            let take = (len - skip).min(to_take);

            buffer
                .extend_from_slice(&data[skip..skip + take])
                .map_err(HkeyError::from)?;

            to_skip -= skip;
            to_take -= take;
        }

        if to_skip > 0 || to_take > 0 {
            HkeyError::Range(range.start - to_skip + buffer.len()).err()?;
        }

        Ok(buffer.into())
    }

    pub fn resolve_slice_async_box<'a, C, E, S>(
        &'a self,
        store: S,
        range: Range,
    ) -> Pin<Box<dyn Future<Output = TResult<Bytes, E>> + Send + 'a>>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        Box::pin(async move { self.resolve_slice_async(store, range).await })
    }

    pub async fn resolve_slice_async<C, E, S>(&self, store: S, range: Range) -> TResult<Bytes, E>
    where
        C: DataChunk + Send,
        E: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        if range.start > range.end {
            HkeyError::InvalidRange(range.clone()).err()?;
        }

        match self {
            Self::List(list) => Self::resolve_list_slice_async(list, store, range).await,

            Self::ListRef(hash, key) => {
                Self::resolve_list_ref_slice_async(hash, key, store, range).await
            }

            Self::LongHkey(lhkey) => {
                lhkey
                    .expand_async(store.clone())
                    .await?
                    .resolve_slice_async(store, range)
                    .await
            }

            Self::LongHkeyExpanded(lhkey) => lhkey.resolve_slice_async(store, range).await,

            _ => {
                let bytes = self.resolve_async(store).await?;

                if bytes.len() >= range.end {
                    return Ok(bytes.slice(range));
                }

                HkeyError::Range(bytes.len()).err()?
            }
        }
    }

    pub fn shrink_or_not<'a, C, E, S>(&self, store: &S) -> TResult<Option<Self>, E>
    where
        C: DataChunk,
        E: From<HkeyError> + Send,
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
                        .put(&decode_base64(base64.as_bytes())?)?
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
            Self::LongHkeyExpanded(lhkey) => lhkey.store(store)?.some(),
            _ => None,
        }
        .ok()
    }

    pub async fn shrink_or_not_async<C, E, S>(&self, store: S) -> TResult<Option<Self>, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + PromiseRejection,
        S: AsyncStore<Chunk = C, Error = E>,
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
                        .put(decode_base64(base64.as_bytes())?.into())
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
                    _ => Err(HkeyError::Storage)?,
                }
            }
            _ => None,
        }
        .ok()
    }

    pub fn shrink_into<'a, C, E, S>(self, store: &S) -> TResult<Self, E>
    where
        C: DataChunk,
        E: From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        (self.shrink_or_not(store)?).map_or_else(|| Ok(self), Ok)
    }

    pub async fn shrink_into_async<C, E, S>(self, store: S) -> TResult<Self, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        (self.shrink_or_not_async(store).await?).map_or_else(|| Ok(self), Ok)
    }

    pub fn shrink<'a, C, E, S>(&self, store: &S) -> TResult<Self, E>
    where
        C: DataChunk,
        E: From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        (self.shrink_or_not(store)?).map_or_else(|| Ok(self.clone()), Ok)
    }

    pub async fn shrink_async<C, E, S>(&self, store: S) -> TResult<Self, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        (self.shrink_or_not_async(store).await?).map_or_else(|| Ok(self.clone()), Ok)
    }

    pub fn shrink_to_string<'a, C, E, S>(&self, store: &S) -> TResult<String, E>
    where
        C: DataChunk,
        E: From<HkeyError> + Send,
        S: Store<Chunk<'a> = C, Error = E> + Sync + 'a,
    {
        self.shrink(store)?.to_string().ok()
    }

    pub async fn shrink_to_string_async<C, E, S>(&self, store: S) -> TResult<String, E>
    where
        C: DataChunk + Send,
        E: From<HkeyError> + PromiseRejection + Send,
        S: AsyncStore<Chunk = C, Error = E>,
    {
        self.shrink_async(store).await?.to_string().ok()
    }
}

/// Decodes base64url `input` into a freshly allocated [`Buffer`].
pub(crate) fn decode_base64(input: &[u8]) -> Result<Buffer> {
    let mut buffer = Buffer::alloc_uninit(ps_base64::decoded_len(input.len()))?;

    let decoded_bytes = ps_base64::decode_into(input, &mut buffer);

    buffer.truncate(decoded_bytes);

    Ok(buffer)
}

impl From<&Hkey> for String {
    fn from(value: &Hkey) -> Self {
        value.to_string()
    }
}

impl std::fmt::Display for Hkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => Ok(()),
            Self::Raw(raw) => ps_base64::encode_into(raw, f),
            Self::Base64(base64) => f.write_str(base64),
            Self::Direct(hash) => write!(f, "{}", hash.display_base64()),
            Self::Encrypted(hash, key) => {
                write!(f, "E{}{}", hash.display_base64(), key.display_base64())
            }
            Self::ListRef(hash, key) => {
                write!(f, "L{}{}", hash.display_base64(), key.display_base64())
            }
            Self::List(list) => Self::fmt_list(list, f),
            Self::LongHkey(lhkey) => write!(f, "{lhkey}"),
            Self::LongHkeyExpanded(lhkey) => write!(f, "{lhkey}"),
        }
    }
}

impl TryFrom<&[u8]> for Hkey {
    type Error = HkeyConstructionError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for Hkey {
    type Error = HkeyConstructionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.as_bytes().try_into()
    }
}

impl From<Hash> for Hkey {
    fn from(hash: Hash) -> Self {
        Self::Direct(hash)
    }
}

impl From<&Hash> for Hkey {
    fn from(hash: &Hash) -> Self {
        Self::from(*hash)
    }
}

impl<A, B> From<(A, B)> for Hkey
where
    A: Into<Hash>,
    B: Into<Hash>,
{
    fn from(value: (A, B)) -> Self {
        Self::Encrypted(value.0.into(), value.1.into())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn raw(bytes: &[u8]) -> Hkey {
        Hkey::from_raw(bytes).expect("Failed to allocate Hkey::Raw")
    }

    /// `[1, 2, 3], [4, 5]`, five bytes across two items.
    fn five_byte_list() -> Hkey {
        Hkey::List(vec![raw(&[1, 2, 3]), raw(&[4, 5])].into())
    }

    /// `[1, 2, 3], [4, 5, 6], [7, 8, 9]`, nine bytes across three items.
    fn nine_byte_list() -> Hkey {
        Hkey::List(vec![raw(&[1, 2, 3]), raw(&[4, 5, 6]), raw(&[7, 8, 9])].into())
    }

    #[test]
    fn resolve_list_slice_full_range() {
        let store = InMemoryStore::default();

        let slice = five_byte_list()
            .resolve_slice(&store, 0..5)
            .expect("Failed to resolve slice");

        assert_eq!(&slice[..], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn resolve_list_slice_spans_items() {
        let store = InMemoryStore::default();

        let slice = five_byte_list()
            .resolve_slice(&store, 2..5)
            .expect("Failed to resolve slice");

        assert_eq!(&slice[..], &[3, 4, 5]);
    }

    #[test]
    fn resolve_list_slice_within_single_item() {
        let store = InMemoryStore::default();
        let list = Hkey::List(vec![raw(&[1, 2, 3])].into());

        let slice = list
            .resolve_slice(&store, 1..3)
            .expect("Failed to resolve slice");

        assert_eq!(&slice[..], &[2, 3]);
    }

    #[test]
    fn resolve_list_slice_skip_spans_multiple_items() {
        let store = InMemoryStore::default();

        let slice = nine_byte_list()
            .resolve_slice(&store, 4..6)
            .expect("Failed to resolve slice");

        assert_eq!(&slice[..], &[5, 6]);
    }

    #[test]
    fn resolve_list_slice_skips_empty_items() {
        let store = InMemoryStore::default();
        let list = Hkey::List(vec![Hkey::Empty, raw(&[1, 2, 3]), Hkey::Empty].into());

        let slice = list
            .resolve_slice(&store, 1..3)
            .expect("Failed to resolve slice");

        assert_eq!(&slice[..], &[2, 3]);
    }

    #[test]
    fn resolve_list_slice_beyond_list_errors() {
        let store = InMemoryStore::default();

        let result = five_byte_list().resolve_slice(&store, 2..7);

        assert!(matches!(
            result,
            Err(InMemoryStoreError::Hkey(HkeyError::Range(5)))
        ));
    }

    #[test]
    fn resolve_list_slice_empty_range_at_exact_end_is_empty() {
        let store = InMemoryStore::default();

        let slice = five_byte_list()
            .resolve_slice(&store, 5..5)
            .expect("Failed to resolve slice");

        assert!(slice.is_empty());
    }

    #[test]
    fn resolve_list_slice_empty_range_past_end_errors() {
        let store = InMemoryStore::default();

        let result = five_byte_list().resolve_slice(&store, 9..9);

        assert!(matches!(
            result,
            Err(InMemoryStoreError::Hkey(HkeyError::Range(5)))
        ));
    }

    #[test]
    fn resolve_list_slice_async_spans_items() {
        let store = InMemoryAsyncStore::default();

        let slice = futures::executor::block_on(five_byte_list().resolve_slice_async(store, 2..5))
            .expect("Failed to resolve slice");

        assert_eq!(&slice[..], &[3, 4, 5]);
    }

    #[test]
    fn resolve_list_slice_async_skip_spans_multiple_items() {
        let store = InMemoryAsyncStore::default();

        let slice = futures::executor::block_on(nine_byte_list().resolve_slice_async(store, 4..6))
            .expect("Failed to resolve slice");

        assert_eq!(&slice[..], &[5, 6]);
    }

    #[test]
    fn resolve_list_slice_async_beyond_list_errors() {
        let store = InMemoryAsyncStore::default();

        let result = futures::executor::block_on(five_byte_list().resolve_slice_async(store, 2..7));

        assert!(matches!(
            result,
            Err(InMemoryAsyncStoreError::Hkey(HkeyError::Range(5)))
        ));
    }

    #[test]
    fn resolve_list_slice_async_empty_range_past_end_errors() {
        let store = InMemoryAsyncStore::default();

        let result = futures::executor::block_on(five_byte_list().resolve_slice_async(store, 9..9));

        assert!(matches!(
            result,
            Err(InMemoryAsyncStoreError::Hkey(HkeyError::Range(5)))
        ));
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn resolve_slice_inverted_range_errors() {
        let store = InMemoryStore::default();

        let result = raw(&[1, 2, 3]).resolve_slice(&store, 3..1);

        assert!(matches!(
            result,
            Err(InMemoryStoreError::Hkey(HkeyError::InvalidRange(_)))
        ));
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn resolve_list_slice_inverted_range_errors() {
        let store = InMemoryStore::default();

        let result = five_byte_list().resolve_slice(&store, 3..1);

        assert!(matches!(
            result,
            Err(InMemoryStoreError::Hkey(HkeyError::InvalidRange(_)))
        ));
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn resolve_list_slice_called_directly_with_inverted_range_errors() {
        let store = InMemoryStore::default();
        let items = [raw(&[1, 2, 3]), raw(&[4, 5])];

        let result = Hkey::resolve_list_slice(&items, &store, 3..1);

        assert!(matches!(
            result,
            Err(InMemoryStoreError::Hkey(HkeyError::InvalidRange(_)))
        ));
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn resolve_slice_async_inverted_range_errors() {
        let store = InMemoryAsyncStore::default();

        let result = futures::executor::block_on(raw(&[1, 2, 3]).resolve_slice_async(store, 3..1));

        assert!(matches!(
            result,
            Err(InMemoryAsyncStoreError::Hkey(HkeyError::InvalidRange(_)))
        ));
    }

    #[test]
    #[allow(clippy::reversed_empty_ranges)]
    fn resolve_list_slice_async_called_directly_with_inverted_range_errors() {
        let store = InMemoryAsyncStore::default();
        let items = [raw(&[1, 2, 3]), raw(&[4, 5])];

        let result =
            futures::executor::block_on(Hkey::resolve_list_slice_async(&items, store, 3..1));

        assert!(matches!(
            result,
            Err(InMemoryAsyncStoreError::Hkey(HkeyError::InvalidRange(_)))
        ));
    }
}
