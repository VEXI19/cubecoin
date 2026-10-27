use std::{io, path::PathBuf};

use rustls::Error as RustlsError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum P2pError {
    #[error("network id must contain exactly 32 bytes")]
    InvalidNetworkId,
    #[error("node id must be exactly 32 bytes of hexadecimal")]
    InvalidNodeId,
    #[error("endpoint must use a concrete loopback address and nonzero port")]
    InvalidEndpoint,
    #[error("unsafe node identity path: {0}")]
    UnsafePath(PathBuf),
    #[error("I/O error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("certificate error: {0}")]
    Certificate(String),
    #[error("TLS error: {0}")]
    Tls(#[from] RustlsError),
    #[error("QUIC error: {0}")]
    Quinn(#[from] quinn::ConnectionError),
    #[error("protocol error: {0}")]
    Protocol(&'static str),
    #[error("peer identity does not match the expected identity")]
    IdentityMismatch,
}
