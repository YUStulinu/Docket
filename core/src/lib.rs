//! Docket's core, shared by the Android app (through UniFFI) and the desktop command line.

pub mod api;
pub mod crypto;
pub mod dates;
pub mod extract;
pub mod fuzzy;
pub mod ics;
pub mod llm;
pub mod model;
pub mod reminders;
pub mod search;
pub mod text;
pub mod vault;

uniffi::setup_scaffolding!();

/// Everything that can go wrong (DocketException in Kotlin).
pub use DocketError as Error;

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum DocketError {
    #[error("bad key: {0}")]
    BadKey(String),
    #[error("cannot decrypt {0}: wrong key, or the file was changed")]
    Decrypt(String),
    #[error("corrupt record: {0}")]
    Corrupt(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("i/o: {0}")]
    Io(String),
    #[error("language model: {0}")]
    Model(String),
}

impl From<std::io::Error> for DocketError {
    fn from(e: std::io::Error) -> Self {
        DocketError::Io(e.to_string())
    }
}
