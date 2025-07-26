use ps_datachunk::{DataChunk, OwnedDataChunk, PsDataChunkError};
use ps_hash::Hash;
use ps_promise::{Promise, PromiseRejection};

use crate::{
    store::in_memory::{InMemoryStore, InMemoryStoreError},
    PsHkeyError, Store,
};

use super::AsyncStore;

#[derive(Clone, Debug, Default)]
pub struct InMemoryAsyncStore {
    store: InMemoryStore,
}

impl AsyncStore for InMemoryAsyncStore {
    type Chunk = OwnedDataChunk;
    type Error = InMemoryAsyncStoreError;

    fn get(&self, hash: &Hash) -> Promise<Self::Chunk, Self::Error> {
        match self.store.get(hash) {
            Ok(chunk) => Promise::Resolved(chunk),
            Err(err) => Promise::Rejected(err.into()),
        }
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Promise<(), Self::Error> {
        match self.store.put_encrypted(chunk) {
            Ok(chunk) => Promise::Resolved(chunk),
            Err(err) => Promise::Rejected(err.into()),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum InMemoryAsyncStoreError {
    #[error(transparent)]
    DataChunk(#[from] PsDataChunkError),
    #[error(transparent)]
    Hkey(#[from] PsHkeyError),
    #[error("The Promise was consumed more than once.")]
    PromiseConsumedAlready,
    #[error(transparent)]
    StoreError(#[from] InMemoryStoreError),
}

impl PromiseRejection for InMemoryAsyncStoreError {
    fn already_consumed() -> Self {
        Self::PromiseConsumedAlready
    }
}
