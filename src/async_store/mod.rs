pub mod in_memory;

use ps_datachunk::DataChunk;
use ps_hash::Hash;
use ps_promise::{Promise, PromiseRejection};

use crate::Hkey;

pub trait AsyncStore {
    type Chunk: DataChunk + Unpin;
    type Error: PromiseRejection + Unpin;

    fn get(&self, hash: &Hash) -> Promise<Self::Chunk, Self::Error>;
    fn put(&self, data: &[u8]) -> Promise<Hkey, Self::Error>;
}
