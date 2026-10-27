use cubecoin_core::config::LoggingConfig;
use cubecoin_p2p::{BootstrapPeer, NodeConfig, NodeId, P2pError, network_id_from_hex};
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub network_id: String,
    pub node_addr: String,
    pub node_data_dir: PathBuf,
    #[serde(default)]
    pub bootstrap_addr: Option<String>,
    #[serde(default)]
    pub bootstrap_node_id: Option<String>,
    #[serde(flatten)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("NODE_ADDR must be an IP address and port, for example 127.0.0.1:5000")]
    InvalidNodeAddress,
    #[error("BOOTSTRAP_ADDR must be an IP address and port")]
    InvalidBootstrapAddress,
    #[error("BOOTSTRAP_ADDR and BOOTSTRAP_NODE_ID must either both be set or both be absent")]
    IncompleteBootstrap,
    #[error(transparent)]
    P2p(#[from] P2pError),
}

impl TryFrom<Config> for NodeConfig {
    type Error = ConfigError;

    fn try_from(config: Config) -> Result<Self, Self::Error> {
        let address = config
            .node_addr
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::InvalidNodeAddress)?;
        let bootstrap = match (config.bootstrap_addr, config.bootstrap_node_id) {
            (None, None) => None,
            (Some(address), Some(node_id)) => Some(BootstrapPeer {
                address: address
                    .parse()
                    .map_err(|_| ConfigError::InvalidBootstrapAddress)?,
                node_id: NodeId::from_hex(&node_id)?,
            }),
            _ => return Err(ConfigError::IncompleteBootstrap),
        };
        Ok(NodeConfig {
            network_id: network_id_from_hex(&config.network_id)?,
            address,
            data_dir: config.node_data_dir,
            bootstrap,
        })
    }
}
