pub mod in_memory;

use std::sync::Arc;

use ps_cypher::validate;
use ps_datachunk::{Bytes, DataChunk, OwnedDataChunk, PsDataChunkError};
use ps_hash::Hash;
use ps_promise::{Promise, PromiseRejection};

use crate::{
    constants::{MAX_DECRYPTED_SIZE, MAX_ENCRYPTED_SIZE, MAX_SIZE_RAW},
    Hkey, LongHkeyExpanded, PsHkeyError,
};

pub trait AsyncStore
where
    Self: Clone + Sized + Send + Sync + 'static,
{
    type Chunk: DataChunk + Unpin;
    type Error: From<PsDataChunkError> + From<PsHkeyError> + PromiseRejection + Unpin;

    fn get(&self, hash: &Hash) -> Promise<Self::Chunk, Self::Error>;

    fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Promise<(), Self::Error>;

    fn put(&self, data: Bytes) -> Promise<Hkey, Self::Error> {
        let this = self.clone();

        Promise::new(async move {
            if data.len() <= MAX_SIZE_RAW {
                return Ok(Hkey::Raw(Arc::from(&*data)));
            }

            if data.len() <= MAX_ENCRYPTED_SIZE && validate(&data) {
                let chunk = OwnedDataChunk::from_bytes(data)?;
                let hash = chunk.hash();

                this.put_encrypted(chunk).await?;

                Ok(Hkey::Direct(hash))
            } else if data.len() <= MAX_DECRYPTED_SIZE {
                let chunk = OwnedDataChunk::from_bytes(data)?;
                let encrypted = chunk.encrypt()?;
                let hkey = Hkey::Encrypted(encrypted.hash(), encrypted.key());

                this.put_encrypted(encrypted).await?;

                Ok(hkey)
            } else {
                LongHkeyExpanded::from_blob_async(&this, &data)
                    .await?
                    .shrink_async(&this)
                    .await
            }
        })
    }
}
