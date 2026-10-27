//! QUIC transport, mutual-TLS node identity, and peer discovery for CubeCoin.

#![forbid(unsafe_code)]

#[cfg(not(target_os = "linux"))]
compile_error!("cubecoin-p2p v1 currently supports Linux only");

mod config;
mod error;
mod identity;
mod node;
mod node_id;
mod tls;
mod transport;
mod wire;

pub use config::{BootstrapPeer, NodeConfig, PeerSnapshot};
pub use error::P2pError;
pub use node::NodeHandle;
pub use node_id::{NodeId, network_id_from_hex};
