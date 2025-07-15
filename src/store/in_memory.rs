use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use ps_datachunk::{BorrowedDataChunk, DataChunk, OwnedDataChunk, PsDataChunkError};
use ps_hash::{Hash, HashError};

use crate::{Hkey, PsHkeyError};

use super::Store;

#[derive(Clone, Debug, Default)]
pub struct InMemoryStore {
    hashmap: Arc<Mutex<HashMap<Hash, OwnedDataChunk>>>,
}

#[derive(thiserror::Error, Debug)]
pub enum InMemoryStoreError {
    #[error(transparent)]
    DataChunk(#[from] PsDataChunkError),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    Hkey(#[from] PsHkeyError),
    #[error("The internal mutex was poisoned.")]
    MutexPoison,
    #[error("The data with this hash was not found.")]
    NotFound,
}

impl<T> From<PoisonError<T>> for InMemoryStoreError {
    fn from(_: PoisonError<T>) -> Self {
        Self::MutexPoison
    }
}

impl Store for InMemoryStore {
    type Chunk = OwnedDataChunk;
    type Error = InMemoryStoreError;

    fn get(&self, hash: &Hash) -> Result<Self::Chunk, Self::Error> {
        self.hashmap
            .lock()?
            .get(hash)
            .cloned()
            .ok_or(InMemoryStoreError::NotFound)
    }

    fn put(&self, bytes: &[u8]) -> Result<Hkey, Self::Error> {
        let chunk = BorrowedDataChunk::from_data(bytes)?;
        let encrypted = chunk.encrypt()?;
        let hash = *encrypted.hash_ref();
        let hkey = Hkey::Encrypted(encrypted.hash(), encrypted.key());

        self.hashmap.lock()?.insert(hash, encrypted.into_owned());

        Ok(hkey)
    }
}
