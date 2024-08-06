use ps_datachunk::PsDataChunkError;
use ps_hash::PsHashError;
use std::num::ParseIntError;
use std::str::Utf8Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PsHkeyError {
    #[error(transparent)]
    PsDataChunkError(#[from] PsDataChunkError),
    #[error(transparent)]
    PsHashError(#[from] PsHashError),
    #[error("Invalid hkey format")]
    FormatError,
    #[error("Invalid range, entity is of size {0}")]
    RangeError(usize),
    #[error("Failed to store with external storage function")]
    StorageError,
    #[error("While storing a List or LongHkey, expected Hkey::Encrypted, got {0}")]
    EncryptedIntoListRefError(crate::Hkey),
    #[error(transparent)]
    Utf8Error(#[from] Utf8Error),
    #[error(transparent)]
    ParseIntError(#[from] ParseIntError),
}

pub type Result<T> = std::result::Result<T, PsHkeyError>;
