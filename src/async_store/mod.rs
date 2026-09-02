pub mod in_memory;
pub mod mixed;

use ps_cypher::{validate, Validity};
use ps_datachunk::{Bytes, DataChunk, DataChunkError, OwnedDataChunk};
use ps_hash::Hash;
use ps_promise::{Promise, PromiseRejection};

use crate::{
    constants::{MAX_DECRYPTED_SIZE, MAX_ENCRYPTED_SIZE, MAX_SIZE_RAW},
    Hkey, HkeyError, LongHkeyExpanded,
};

pub trait AsyncStore
where
    Self: Clone + Sized + Send + 'static,
{
    type Chunk: DataChunk + Send;
    type Error: From<DataChunkError> + From<HkeyError> + PromiseRejection + Send;

    fn get(&self, hash: &Hash) -> Promise<Self::Chunk, Self::Error>;

    /// Storage primitive: writes `chunk` under its own hash exactly as given.
    ///
    /// Implementors provide this method; callers should not use it. The chunk
    /// is neither encrypted nor validated, and its hash is trusted to match
    /// its data. Deciding whether data needs to be stored at all, and in what
    /// form, is the job of [`put`](Self::put), which is the method callers
    /// should use: small data is inlined into the returned [`Hkey`], larger
    /// data is encrypted and stored as one or more chunks, and data that is
    /// already an encrypted chunk is stored as is.
    fn put_verbatim<C: DataChunk>(&self, chunk: C) -> Promise<(), Self::Error>;

    fn put(&self, data: Bytes) -> Promise<Hkey, Self::Error> {
        if data.len() <= MAX_SIZE_RAW {
            return match Hkey::from_raw(&data) {
                Ok(hkey) => Promise::resolve(hkey),
                Err(err) => Promise::reject(Self::Error::from(HkeyError::Construction(err))),
            };
        }

        let this = self.clone();

        Promise::lazy(async move {
            if data.len() <= MAX_ENCRYPTED_SIZE && Validity::Pristine == validate(&data) {
                let chunk = OwnedDataChunk::from_bytes(data)?;
                let hash = chunk.hash();

                this.put_verbatim(chunk).await?;

                Ok(Hkey::Direct(hash))
            } else if data.len() <= MAX_DECRYPTED_SIZE {
                let chunk = OwnedDataChunk::from_bytes(data)?;
                let encrypted = chunk.encrypt()?;
                let hkey = Hkey::Encrypted(encrypted.hash(), encrypted.key());

                this.put_verbatim(encrypted).await?;

                Ok(hkey)
            } else {
                LongHkeyExpanded::from_blob_async(this.clone(), &data)
                    .await?
                    .shrink_async(this)
                    .await
            }
        })
    }
}
