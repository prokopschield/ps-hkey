use std::{
    future::Future,
    mem::{self, replace},
    ops::Deref,
    sync::Arc,
};

use futures::FutureExt;
use parking_lot::RwLock;
use ps_datachunk::{DataChunk, OwnedDataChunk, PsDataChunkError};
use ps_hash::Hash;
use ps_promise::{Promise, PromiseRejection};

use crate::{store::combined::DynStore, AsyncStore, PsHkeyError, Store};

pub trait DynAsyncStore: Send + Sync {
    type Error: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send + 'static;

    fn get(&self, hash: Arc<Hash>) -> Promise<OwnedDataChunk, Self::Error>;
    fn put_encrypted(&self, chunk: OwnedDataChunk) -> Promise<(), Self::Error>;
}

impl<T> DynAsyncStore for T
where
    T: AsyncStore + Send + Sync + 'static,
{
    type Error = T::Error;

    fn get(&self, hash: Arc<Hash>) -> Promise<OwnedDataChunk, Self::Error> {
        let store = self.clone();

        Promise::new(async move {
            let hash = hash;

            Ok(AsyncStore::get(&store, &hash).await?.into_owned())
        })
    }

    fn put_encrypted(&self, chunk: OwnedDataChunk) -> Promise<(), Self::Error> {
        AsyncStore::put_encrypted(self, chunk)
    }
}

#[derive(Default)]
pub struct MixedStoreInner<E: MixedStoreError> {
    pub async_stores: Vec<Box<dyn DynAsyncStore<Error = E>>>,
    pub stores: Vec<Box<dyn DynStore<Error = E>>>,
}

#[derive(Clone, Default)]
pub struct MixedStore<E: MixedStoreError, const WRITE_TO_ALL: bool> {
    inner: Arc<RwLock<MixedStoreInner<E>>>,
}

impl<E: MixedStoreError, const WRITE_TO_ALL: bool> Deref for MixedStore<E, WRITE_TO_ALL> {
    type Target = Arc<RwLock<MixedStoreInner<E>>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<E: MixedStoreError, const WRITE_TO_ALL: bool> MixedStore<E, WRITE_TO_ALL> {
    /// Creates a `MixedStore` from a list of Stores.
    #[must_use]
    pub fn new<S, A, IS, IA>(stores: IS, async_stores: IA) -> Self
    where
        S: Store<Error = E> + Send + Sync + 'static,
        A: AsyncStore<Error = E>,
        IS: IntoIterator<Item = S>,
        IA: IntoIterator<Item = A>,
    {
        Self {
            inner: Arc::new(RwLock::new(MixedStoreInner {
                async_stores: async_stores.into_iter().map(|s| Box::new(s) as _).collect(),
                stores: stores.into_iter().map(|s| Box::new(s) as _).collect(),
            })),
        }
    }

    pub fn push_sync<S>(&mut self, store: S)
    where
        S: Store<Error = E> + Send + Sync + 'static,
    {
        self.write().stores.push(Box::new(store));
    }

    pub fn push_async<A>(&mut self, store: A)
    where
        A: AsyncStore<Error = E>,
    {
        self.write().async_stores.push(Box::new(store));
    }

    pub fn extend_sync<S, I>(&mut self, iter: I)
    where
        S: Store<Error = E> + Send + Sync + 'static,
        I: IntoIterator<Item = S>,
    {
        self.write()
            .stores
            .extend(iter.into_iter().map(|s| Box::new(s) as _));
    }

    pub fn extend_async<A, I>(&mut self, iter: I)
    where
        A: AsyncStore<Error = E>,
        I: IntoIterator<Item = A>,
    {
        self.write()
            .async_stores
            .extend(iter.into_iter().map(|s| Box::new(s) as _));
    }

    #[must_use]
    pub fn write_to_all(self) -> MixedStore<E, true> {
        MixedStore { inner: self.inner }
    }

    #[must_use]
    pub fn write_to_one(self) -> MixedStore<E, false> {
        MixedStore { inner: self.inner }
    }

    fn get_sync(&self, hash: &Hash) -> Result<OwnedDataChunk, E> {
        let mut last_err = None;

        for s in &self.read().stores {
            match s.get(hash) {
                Ok(chunk) => return Ok(chunk),
                Err(err) => last_err = Some(err),
            }
        }

        Err(last_err.unwrap_or_else(E::no_stores))
    }

    fn get_async(&self, hash: &Arc<Hash>) -> Promise<OwnedDataChunk, E> {
        let mut last_err = E::no_stores();
        let guard = self.read();

        for s in &guard.stores {
            match s.get(hash) {
                Ok(chunk) => return Promise::Resolved(chunk),
                Err(err) => last_err = err,
            }
        }

        let promises: Vec<Promise<OwnedDataChunk, E>> = guard
            .async_stores
            .iter()
            .map(|store| store.get(hash.clone()))
            .collect();

        drop(guard);

        Promise::new(GetAsync { last_err, promises })
    }
}

struct GetAsync<E: MixedStoreError> {
    last_err: E,
    promises: Vec<Promise<OwnedDataChunk, E>>,
}

impl<E: MixedStoreError> Future for GetAsync<E> {
    type Output = Result<OwnedDataChunk, E>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::task::Poll::{Pending, Ready};

        let queue = mem::take(&mut self.promises);

        for promise in queue {
            match promise {
                Promise::Consumed => {}
                Promise::Pending(mut future) => match future.poll_unpin(cx) {
                    Pending => self.promises.push(Promise::Pending(future)),
                    Ready(Ok(chunk)) => return Ready(Ok(chunk)),
                    Ready(Err(err)) => self.last_err = err,
                },
                Promise::Rejected(err) => self.last_err = err,
                Promise::Resolved(chunk) => return Ready(Ok(chunk)),
            }
        }

        if self.promises.is_empty() {
            return Ready(Err(replace(&mut self.last_err, E::already_consumed())));
        }

        Pending
    }
}

impl<E: MixedStoreError> Store for MixedStore<E, true> {
    type Chunk<'c> = OwnedDataChunk;
    type Error = E;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error> {
        self.get_sync(hash)
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Result<(), Self::Error> {
        let guard = self.read();

        if guard.stores.is_empty() {
            return Err(E::no_stores());
        }

        for s in &guard.stores {
            s.put_encrypted(chunk.borrow())?;
        }

        drop(guard);

        Ok(())
    }
}

impl<E: MixedStoreError> Store for MixedStore<E, false> {
    type Chunk<'c> = OwnedDataChunk;
    type Error = E;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error> {
        self.get_sync(hash)
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Result<(), Self::Error> {
        let mut last_err = E::no_stores();

        for store in &self.read().stores {
            match store.put_encrypted(chunk.borrow()) {
                Ok(()) => return Ok(()),
                Err(err) => last_err = err,
            }
        }

        Err(last_err)
    }
}

impl<E: MixedStoreError> AsyncStore for MixedStore<E, true> {
    type Chunk = OwnedDataChunk;
    type Error = E;

    fn get(&self, hash: &Hash) -> Promise<Self::Chunk, Self::Error> {
        let store = self.clone();
        let hash = Arc::from(*hash);

        Promise::new(async move { store.get_async(&hash).await })
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Promise<(), Self::Error> {
        let this = self.clone();
        let chunk = chunk.into_owned();

        let guard = this.read();

        if guard.stores.is_empty() && guard.async_stores.is_empty() {
            return Promise::reject(E::no_stores());
        }

        let mut promises = Vec::new();

        promises.extend(guard.stores.iter().map(|store| {
            let chunk = chunk.clone();

            match store.put_encrypted(chunk.borrow()) {
                Ok(()) => Promise::resolve(()),
                Err(err) => Promise::reject(err),
            }
        }));

        promises.extend(
            guard
                .async_stores
                .iter()
                .map(|store| store.put_encrypted(chunk.clone())),
        );

        drop(guard);

        Promise::all(promises).then(async |_| Ok(()))
    }
}

impl<E: MixedStoreError> AsyncStore for MixedStore<E, false> {
    type Chunk = OwnedDataChunk;
    type Error = E;

    fn get(&self, hash: &Hash) -> Promise<Self::Chunk, Self::Error> {
        let store = self.clone();
        let hash = Arc::from(*hash);

        Promise::new(async move { store.get_async(&hash).await })
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Promise<(), Self::Error> {
        let this = self.clone();
        let chunk = chunk.into_owned();

        let guard = this.read();

        if guard.stores.is_empty() && guard.async_stores.is_empty() {
            return Promise::reject(E::no_stores());
        }

        let mut last_err = None;

        for store in &guard.stores {
            match store.put_encrypted(chunk.borrow()) {
                Ok(()) => return Promise::resolve(()),
                Err(err) => last_err = Some(err),
            }
        }

        if guard.async_stores.is_empty() {
            return Promise::reject(last_err.unwrap_or_else(E::no_stores));
        }

        let promises: Vec<Promise<(), E>> = guard
            .async_stores
            .iter()
            .map(|store| store.put_encrypted(chunk.clone()))
            .collect();

        drop(guard);

        Promise::new(async move {
            match Promise::any(promises).await {
                Ok(()) => Ok(()),
                Err(mut errors) => {
                    let err = errors
                        .pop()
                        .or(last_err)
                        .unwrap_or_else(E::already_consumed);

                    Err(err)
                }
            }
        })
    }
}

pub trait MixedStoreError:
    Clone + From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Send + 'static
{
    fn no_stores() -> Self;
}
