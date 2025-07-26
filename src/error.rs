use std::io::Error as IOError;
use thiserror::Error;
//use vectorbase::util::error::GyError;

pub type ChapResult<T> = anyhow::Result<T>;

#[derive(Error, Debug)]
pub enum ChapError {
    #[error("Unexpected: {0}")]
    Unexpected(String),
    #[error("Unexpected IO: {0}")]
    UnexpectIO(IOError),
    #[error("Please enter a file name, example: `chap file.txt`")]
    NoFilePath,
    //#[error("vectorbase error: {0}")]
    //VectorBaseError(GyError),
    #[error("LLM api not registered: {0}")]
    LLMNotRegistered(String),
}
