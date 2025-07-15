pub mod in_memory;

use std::error::Error;

use ps_datachunk::DataChunk;
use ps_hash::Hash;
use ps_promise::Promise;

use crate::Hkey;

pub trait AsyncStore {
    type Chunk: DataChunk + Unpin;
    type Error: Error + Unpin;

    fn get(&self, hash: &Hash) -> Promise<Self::Chunk, Self::Error>;
    fn put(&self, data: &[u8]) -> Promise<Hkey, Self::Error>;
}
