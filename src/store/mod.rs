pub mod in_memory;

use std::error::Error;

use ps_datachunk::DataChunk;
use ps_hash::Hash;

use crate::Hkey;

pub trait Store {
    type Chunk<'c>: DataChunk
    where
        Self: 'c;

    type Error: Error;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error>;
    fn put(&self, data: &[u8]) -> Result<Hkey, Self::Error>;
}
