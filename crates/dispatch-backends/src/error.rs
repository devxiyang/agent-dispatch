use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("unsupported backend operation: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, BackendError>;
