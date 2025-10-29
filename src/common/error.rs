use log::SetLoggerError;
use mlua::prelude::LuaError;
use std::io;
use std::io::Error as IOError;
use thiserror::Error;

pub type ChapResult<T> = Result<T, ChapError>;

#[derive(Error, Debug)]
pub enum ChapError {
    #[error("Unexpected: {0}")]
    Unexpected(String),
    #[error("Unexpected IO: {0}")]
    UnexpectIO(IOError),
    #[error("Please enter a file name, example: `chap file.txt`")]
    NoFilePath,
    #[error("File not found: {0}")]
    FileNotFound(String),
    //#[error("vectorbase error: {0}")]
    //VectorBaseError(ChapError),
    #[error("LLM api not registered: {0}")]
    LLMNotRegistered(String),
    #[error("log error: {0}")]
    LogError(SetLoggerError),
    #[error("lua error: {0}")]
    LuaFail(LuaError),
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

impl From<IOError> for ChapError {
    fn from(e: IOError) -> Self {
        ChapError::Unexpected(e.to_string())
    }
}

impl From<ChapError> for String {
    fn from(e: ChapError) -> Self {
        format!("{}", e)
    }
}

impl From<SetLoggerError> for ChapError {
    fn from(e: SetLoggerError) -> Self {
        ChapError::LogError(e)
    }
}

impl From<LuaError> for ChapError {
    fn from(e: LuaError) -> Self {
        ChapError::LuaFail(e)
    }
}
