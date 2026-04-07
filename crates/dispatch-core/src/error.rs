use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("task `{0}` does not exist")]
    TaskNotFound(String),
    #[error("backend `{0}` is not registered")]
    UnknownBackend(String),
    #[error("invalid task state transition: {0}")]
    InvalidState(String),
    #[error("filesystem error at `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("yaml serialization error: {0}")]
    SerdeYaml(#[from] serde_yaml::Error),
}

pub type Result<T> = std::result::Result<T, DispatchError>;
