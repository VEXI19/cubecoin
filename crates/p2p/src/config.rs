use std::{net::SocketAddr, path::PathBuf};

use crate::{
    error::P2pError,
    node_id::{NETWORK_ID_LENGTH, NodeId},
};

#[derive(Clone, Debug)]
pub struct BootstrapPeer {
    pub address: SocketAddr,
    pub node_id: NodeId,
}

#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub network_id: [u8; NETWORK_ID_LENGTH],
    pub address: SocketAddr,
    pub data_dir: PathBuf,
    pub bootstrap: Option<BootstrapPeer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerSnapshot {
    pub node_id: NodeId,
    pub address: SocketAddr,
    pub verified: bool,
    pub connected: bool,
}

pub(crate) fn validate_config(config: &NodeConfig) -> Result<(), P2pError> {
    if !valid_endpoint(config.address) {
        return Err(P2pError::InvalidEndpoint);
    }
    if !config.data_dir.is_absolute() {
        return Err(P2pError::UnsafePath(config.data_dir.clone()));
    }
    if let Some(bootstrap) = &config.bootstrap
        && (!valid_endpoint(bootstrap.address) || bootstrap.address == config.address)
    {
        return Err(P2pError::InvalidEndpoint);
    }
    Ok(())
}

pub(crate) fn valid_endpoint(address: SocketAddr) -> bool {
    address.port() != 0 && address.ip().is_loopback()
}
