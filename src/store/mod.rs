pub mod combined;
pub mod in_memory;

use ps_cypher::{validate, Validity};
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

        if data.len() <= MAX_ENCRYPTED_SIZE && Validity::Pristine == validate(data) {
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

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use ps_cypher::encrypt;

    use crate::{store::in_memory::InMemoryStore, Hkey, Store};

    use super::*;

    /// Fills a buffer with xorshift output, which zstd cannot compress.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;

        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;

                state.to_le_bytes()[0]
            })
            .collect()
    }

    #[test]
    fn put_stores_encrypted_chunk_directly() {
        let store = InMemoryStore::default();
        let encrypted = encrypt(&incompressible(1024)).expect("encryption should succeed");

        let hkey = store.put(&encrypted).expect("put should succeed");

        assert_eq!(hkey, Hkey::Direct(encrypted.hash));
        assert!(store.get(&encrypted.hash).is_ok());
    }

    #[test]
    fn put_encrypts_foreign_codeword() {
        let store = InMemoryStore::default();
        let codeword = ps_ecc::encode(&incompressible(1024), 12).expect("encoding should succeed");

        let hkey = store.put(&codeword).expect("put should succeed");

        assert!(matches!(hkey, Hkey::Encrypted(..)), "got {hkey:?}");
    }

    #[test]
    fn largest_encrypted_chunk_fits_direct_path() {
        let store = InMemoryStore::default();
        let encrypted =
            encrypt(&incompressible(MAX_DECRYPTED_SIZE)).expect("encryption should succeed");

        assert!(encrypted.len() <= MAX_ENCRYPTED_SIZE, "{}", encrypted.len());

        let hkey = store.put(&encrypted).expect("put should succeed");

        assert_eq!(hkey, Hkey::Direct(encrypted.hash));
    }
}
