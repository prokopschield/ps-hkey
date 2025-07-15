pub mod in_memory;

use std::error::Error;

use ps_datachunk::DataChunk;
use ps_hash::Hash;

use crate::Hkey;

pub trait Store {
    type Chunk: DataChunk;
    type Error: Error;

    fn get(&self, hash: &Hash) -> Result<Self::Chunk, Self::Error>;
    fn put(&self, data: &[u8]) -> Result<Hkey, Self::Error>;
}
