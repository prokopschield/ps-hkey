use ps_datachunk::DataChunkError;
use ps_hash::{HashError, HashValidationError};
use std::num::ParseIntError;
use std::str::Utf8Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HkeyError {
    #[error(transparent)]
    Construction(#[from] HkeyConstructionError),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    HashValidation(#[from] HashValidationError),
    #[error(transparent)]
    ParseInt(#[from] ParseIntError),
    #[error(transparent)]
    DataChunk(#[from] DataChunkError),
    #[error(transparent)]
    Utf8(#[from] Utf8Error),

    #[error("Invalid hkey format")]
    Format,
    #[error("Invalid range, entity is of size {0}")]
    Range(usize),
    #[error("Failed to store with external storage function")]
    Storage,
    #[error("While storing a List or LongHkey, expected Hkey::Encrypted, got {0}")]
    EncryptedIntoListRef(crate::Hkey),
    #[error("Reached unreachable code.")]
    Unreachable,
}

pub type Result<T, E = HkeyError> = std::result::Result<T, E>;

#[derive(Error, Debug)]
pub enum HkeyConstructionError {
    #[error("Maximum length exceeded.")]
    TooLong,
}

#[derive(Error, Debug)]
pub enum HkeyFromCompactError {
    #[error(transparent)]
    Construction(#[from] HkeyConstructionError),
    #[error("Hash validation error: {0}")]
    HashValidation(#[from] ps_hash::HashValidationError),
}
