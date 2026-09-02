use ps_datachunk::{DataChunk, DataChunkError, OwnedDataChunk};
use ps_hash::Hash;
use ps_promise::{Promise, PromiseRejection, TaskFailure};

use crate::{
    store::in_memory::{InMemoryStore, InMemoryStoreError},
    HkeyError, Store,
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
            Ok(chunk) => Promise::resolve(chunk),
            Err(err) => Promise::reject(err.into()),
        }
    }

    fn put_verbatim<C: DataChunk>(&self, chunk: C) -> Promise<(), Self::Error> {
        match self.store.put_verbatim(chunk) {
            Ok(chunk) => Promise::resolve(chunk),
            Err(err) => Promise::reject(err.into()),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum InMemoryAsyncStoreError {
    #[error(transparent)]
    DataChunk(#[from] DataChunkError),
    #[error(transparent)]
    Hkey(#[from] HkeyError),
    #[error("The Promise was consumed more than once.")]
    PromiseConsumedAlready,
    #[error(transparent)]
    StoreError(#[from] InMemoryStoreError),
    #[error(transparent)]
    TaskFailed(#[from] TaskFailure),
}

impl PromiseRejection for InMemoryAsyncStoreError {
    fn already_consumed() -> Self {
        Self::PromiseConsumedAlready
    }

    fn task_failed(failure: TaskFailure) -> Self {
        Self::TaskFailed(failure)
    }
}
