use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use ps_datachunk::{DataChunk, OwnedDataChunk, PsDataChunkError};
use ps_hash::{Hash, HashError};

use crate::PsHkeyError;

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
    type Chunk<'c> = OwnedDataChunk;
    type Error = InMemoryStoreError;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error> {
        self.hashmap
            .lock()?
            .get(hash)
            .cloned()
            .ok_or(InMemoryStoreError::NotFound)
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Result<(), Self::Error> {
        let chunk = chunk.into_owned();
        let hash = *chunk.hash_ref();

        self.hashmap.lock()?.insert(hash, chunk);

        Ok(())
    }
}
