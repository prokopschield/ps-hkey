use ps_hash::PsHashError;
use std::num::ParseIntError;
use std::str::Utf8Error;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PsHkeyError {
    #[error(transparent)]
    PsHashError(#[from] PsHashError),
    #[error("Invalid hkey format")]
    FormatError,
    #[error("Failed to store with external storage function")]
    StorageError,
    #[error(transparent)]
    Utf8Error(#[from] Utf8Error),
    #[error(transparent)]
    ParseIntError(#[from] ParseIntError),
}

pub type Result<T> = std::result::Result<T, PsHkeyError>;
