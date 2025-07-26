pub mod in_memory;

use ps_cypher::validate;
use ps_datachunk::{BorrowedDataChunk, DataChunk, PsDataChunkError};
use ps_hash::Hash;

use crate::{
    constants::{MAX_DECRYPTED_SIZE, MAX_ENCRYPTED_SIZE, MAX_SIZE_RAW},
    Hkey, LongHkeyExpanded, PsHkeyError,
};

pub trait Store
where
    Self: Sized + Sync,
{
    type Chunk<'c>: DataChunk
    where
        Self: 'c;

    type Error: From<PsDataChunkError> + From<PsHkeyError> + Send;

    fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error>;

    fn put_encrypted<C: DataChunk>(&self, data: &C) -> Result<(), Self::Error>;

    fn put(&self, data: &[u8]) -> Result<Hkey, Self::Error> {
        if data.len() <= MAX_SIZE_RAW {
            return Ok(Hkey::Raw(data.into()));
        }

        let chunk = BorrowedDataChunk::from_data(data)?;

        if data.len() <= MAX_ENCRYPTED_SIZE && validate(data) {
            self.put_encrypted(&chunk)?;

            Ok(Hkey::Direct(chunk.hash()))
        } else if data.len() <= MAX_DECRYPTED_SIZE {
            let encrypted = chunk.encrypt()?;
            let hkey = Hkey::Encrypted(encrypted.hash(), encrypted.key());

            self.put_encrypted(&encrypted)?;

            Ok(hkey)
        } else {
            LongHkeyExpanded::from_blob(self, data)?.shrink(self)
        }
    }
}
