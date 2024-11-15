use std::io::Error as IOError;
use thiserror::Error;
pub type ChapResult<T> = Result<T, ChapError>;

#[derive(Error, Debug)]
pub enum ChapError {
    #[error("Unexpected: {0}")]
    Unexpected(String),
    #[error("Unexpected IO: {0}")]
    UnexpectIO(IOError),
}

impl From<IOError> for ChapError {
    fn from(e: IOError) -> Self {
        ChapError::UnexpectIO(e)
    }
}

impl From<&str> for ChapError {
    fn from(e: &str) -> Self {
        ChapError::Unexpected(e.to_string())
    }
}

impl From<String> for ChapError {
    fn from(e: String) -> Self {
        ChapError::Unexpected(e)
    }
}
