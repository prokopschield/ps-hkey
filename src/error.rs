use ps_datachunk::PsDataChunkError;
use ps_hash::{HashError, HashValidationError};
use std::num::ParseIntError;
use std::str::Utf8Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PsHkeyError {
    #[error(transparent)]
    HashError(#[from] HashError),
    #[error(transparent)]
    HashValidationError(#[from] HashValidationError),
    #[error(transparent)]
    ParseIntError(#[from] ParseIntError),
    #[error(transparent)]
    PsDataChunkError(#[from] PsDataChunkError),
    #[error(transparent)]
    Utf8Error(#[from] Utf8Error),

    #[error("Invalid hkey format")]
    FormatError,
    #[error("Invalid range, entity is of size {0}")]
    RangeError(usize),
    #[error("Failed to store with external storage function")]
    StorageError,
    #[error("While storing a List or LongHkey, expected Hkey::Encrypted, got {0}")]
    EncryptedIntoListRefError(crate::Hkey),
    #[error("Reached unreachable code.")]
    UnreachableCodeReached,
}

pub type Result<T> = std::result::Result<T, PsHkeyError>;
