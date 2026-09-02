pub mod combined;
pub mod in_memory;

use ps_cypher::validate_ecc;
use ps_datachunk::{BorrowedDataChunk, DataChunk, DataChunkError};
use ps_hash::Hash;

use crate::{
    constants::{MAX_DECRYPTED_SIZE, MAX_ENCRYPTED_SIZE, MAX_SIZE_RAW},
    Hkey, HkeyError, LongHkeyExpanded,
};

pub trait Store
where
    Self: Sized + Sync,
{
    type Chunk<'c>: DataChunk
    where
        Self: 'c;

    type Error: From<DataChunkError> + From<HkeyError> + Send;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error>;

    /// Storage primitive: writes `chunk` under its own hash exactly as given.
    ///
    /// Implementors provide this method; callers should not use it. The chunk
    /// is neither encrypted nor validated, and its hash is trusted to match
    /// its data. Deciding whether data needs to be stored at all, and in what
    /// form, is the job of [`put`](Self::put), which is the method callers
    /// should use: small data is inlined into the returned [`Hkey`], larger
    /// data is encrypted and stored as one or more chunks, and data that is
    /// already an encrypted chunk is stored as is.
    fn put_verbatim<C: DataChunk>(&self, chunk: C) -> Result<(), Self::Error>;

    fn put(&self, data: &[u8]) -> Result<Hkey, Self::Error> {
        if data.len() <= MAX_SIZE_RAW {
            return Hkey::from_raw(data)
                .map_err(HkeyError::Construction)
                .map_err(Into::into);
        }

        if data.len() <= MAX_ENCRYPTED_SIZE && validate_ecc(data) {
            let chunk = BorrowedDataChunk::from_data(data)?;
            let hash = chunk.hash();

            self.put_verbatim(chunk)?;

            Ok(Hkey::Direct(hash))
        } else if data.len() <= MAX_DECRYPTED_SIZE {
            let chunk = BorrowedDataChunk::from_data(data)?;
            let encrypted = chunk.encrypt()?;
            let hkey = Hkey::Encrypted(encrypted.hash(), encrypted.key());

            self.put_verbatim(encrypted)?;

            Ok(hkey)
        } else {
            LongHkeyExpanded::from_blob(self, data)?.shrink(self)
        }
    }
}
