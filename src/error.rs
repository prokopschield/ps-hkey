use ps_hash::PsHashError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PsHkeyError {
    #[error(transparent)]
    PsHashError(#[from] PsHashError),
    #[error("Invalid hkey format")]
    FormatError,
}

pub type Result<T> = std::result::Result<T, PsHkeyError>;
