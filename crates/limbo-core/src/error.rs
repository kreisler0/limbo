use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// Malformed input data (DER, CRX, jsonlz4, INI, ...).
    #[error("invalid data: {0}")]
    Format(String),
    #[error("decryption failed: {0}")]
    Crypto(String),
    /// The Firefox primary password is wrong (or one is set and none was given).
    #[error("the Firefox primary password is incorrect")]
    WrongPrimaryPassword,
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("not found: {0}")]
    NotFound(String),
    /// The OS secret store (DPAPI) failed.
    #[error("secret storage error: {0}")]
    Protect(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

