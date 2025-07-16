use ps_datachunk::OwnedDataChunk;
use ps_hash::Hash;
use ps_promise::{Promise, PromiseRejection};

use crate::{
    store::in_memory::{InMemoryStore, InMemoryStoreError},
    Hkey, Store,
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

    fn put(&self, bytes: &[u8]) -> Promise<Hkey, Self::Error> {
        match self.store.put(bytes) {
            Ok(chunk) => Promise::Resolved(chunk),
            Err(err) => Promise::Rejected(err.into()),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum InMemoryAsyncStoreError {
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
