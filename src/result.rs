use log::{self};
use thiserror::{self};

// ---

pub fn log_err(e: TProxyError) -> TProxyError { log::error!("{}", e); e }

// ---

#[derive(Debug, thiserror::Error)]
pub enum TProxyError {
    #[error("Fatal")]
    Fatal,

    #[error("Invalid operation")]
    InvalidOperation,

    #[error("Invalid schema")]
    InvalidSchema,

    #[error("Not found")]
    NotFound,

    #[error(transparent)]
    Errno(#[from] nix::errno::Errno),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::error::Error),

    #[error(transparent)]
    NetParseAddr(#[from] std::net::AddrParseError),

    #[error(transparent)]
    NumParseInt(#[from] std::num::ParseIntError),

    #[error(transparent)]
    Regex(#[from] regex::Error),

    #[error(transparent)]
    StringFromUtf8(#[from] std::string::FromUtf8Error),

    #[error(transparent)]
    Which(#[from] which::Error),
}

pub type Result<T> = std::result::Result<T, TProxyError>;
