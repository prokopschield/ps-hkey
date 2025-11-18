use std::ops::{Deref, DerefMut};

use ps_datachunk::{BorrowedDataChunk, DataChunk, OwnedDataChunk, PsDataChunkError};
use ps_hash::Hash;

use crate::{PsHkeyError, Store};

pub trait DynStore: Send + Sync {
    type Error: From<PsDataChunkError> + From<PsHkeyError> + Send + 'static;

    fn get(&self, hash: &Hash) -> Result<OwnedDataChunk, Self::Error>;
    fn put_encrypted(&self, chunk: BorrowedDataChunk<'_>) -> Result<(), Self::Error>;
}

impl<T> DynStore for T
where
    T: Store + Send + Sync + 'static,
{
    type Error = T::Error;

    fn get(&self, hash: &Hash) -> Result<OwnedDataChunk, Self::Error> {
        Ok(Store::get(self, hash)?.into_owned())
    }

    fn put_encrypted(&self, chunk: BorrowedDataChunk<'_>) -> Result<(), Self::Error> {
        Store::put_encrypted(self, chunk)
    }
}

#[derive(Default)]
pub struct CombinedStore<E: CombinedStoreError, const WRITE_TO_ALL: bool> {
    stores: Vec<Box<dyn DynStore<Error = E>>>,
}

impl<E: CombinedStoreError, const WRITE_TO_ALL: bool> CombinedStore<E, WRITE_TO_ALL> {
    /// Creates a `CombinedStore` from a list of Stores.
    #[must_use]
    pub fn new<S, I>(stores: I) -> Self
    where
        S: Store<Error = E> + Send + Sync + 'static,
        I: IntoIterator<Item = S>,
    {
        Self {
            stores: stores.into_iter().map(|s| Box::new(s) as _).collect(),
        }
    }

    pub fn push<S>(&mut self, store: S)
    where
        S: Store<Error = E> + Send + Sync + 'static,
    {
        self.stores.push(Box::new(store));
    }

    pub fn extend<S, I>(&mut self, iter: I)
    where
        S: Store<Error = E> + Send + Sync + 'static,
        I: IntoIterator<Item = S>,
    {
        self.stores
            .extend(iter.into_iter().map(|s| Box::new(s) as _));
    }

    #[must_use]
    pub fn write_to_all(self) -> CombinedStore<E, true> {
        CombinedStore {
            stores: self.stores,
        }
    }

    #[must_use]
    pub fn write_to_one(self) -> CombinedStore<E, false> {
        CombinedStore {
            stores: self.stores,
        }
    }

    fn get(&self, hash: &Hash) -> Result<OwnedDataChunk, E> {
        let mut last_err = None;

        for s in self.iter() {
            match s.get(hash) {
                Ok(chunk) => return Ok(chunk),
                Err(err) => last_err = Some(err),
            }
        }

        Err(last_err.unwrap_or_else(E::no_stores))
    }
}

impl<E: CombinedStoreError, const WRITE_TO_ALL: bool> Deref for CombinedStore<E, WRITE_TO_ALL> {
    type Target = Vec<Box<dyn DynStore<Error = E>>>;

    fn deref(&self) -> &Self::Target {
        &self.stores
    }
}

impl<E: CombinedStoreError, const WRITE_TO_ALL: bool> DerefMut for CombinedStore<E, WRITE_TO_ALL> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stores
    }
}

impl<E: CombinedStoreError> Store for CombinedStore<E, true> {
    type Chunk<'c> = OwnedDataChunk;
    type Error = E;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error> {
        self.get(hash)
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Result<(), Self::Error> {
        if self.is_empty() {
            return Err(E::no_stores());
        }

        let mut result = Ok(());

        for s in self.iter() {
            result = result.and(s.put_encrypted(chunk.borrow()));
        }

        result
    }
}

impl<E: CombinedStoreError> Store for CombinedStore<E, false> {
    type Chunk<'c> = OwnedDataChunk;
    type Error = E;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error> {
        self.get(hash)
    }

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Result<(), Self::Error> {
        let mut last_err = E::no_stores();

        for store in self.iter() {
            match store.put_encrypted(chunk.borrow()) {
                Ok(()) => return Ok(()),
                Err(err) => last_err = err,
            }
        }

        Err(last_err)
    }
}

pub trait CombinedStoreError: From<PsDataChunkError> + From<PsHkeyError> + Send + 'static {
    fn no_stores() -> Self;
}
