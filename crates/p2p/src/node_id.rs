use std::fmt;

use sha2::{Digest, Sha256};

use crate::error::P2pError;

pub(crate) const NETWORK_ID_LENGTH: usize = 32;
const NODE_ID_DOMAIN: &[u8] = b"cubecoin/node-id/ed25519/v1\0";
const NODE_ID_LENGTH: usize = 32;

/// Stable, self-certifying identity of a P2P node.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId(pub(crate) [u8; NODE_ID_LENGTH]);

impl NodeId {
    pub fn from_hex(value: &str) -> Result<Self, P2pError> {
        let bytes = hex::decode(value).map_err(|_| P2pError::InvalidNodeId)?;
        let bytes: [u8; NODE_ID_LENGTH] = bytes.try_into().map_err(|_| P2pError::InvalidNodeId)?;
        Ok(Self(bytes))
    }

    pub fn from_public_key(network_id: &[u8; NETWORK_ID_LENGTH], public_key: &[u8; 32]) -> Self {
        let mut hash = Sha256::new();
        hash.update(NODE_ID_DOMAIN);
        hash.update(network_id);
        hash.update(public_key);
        Self(hash.finalize().into())
    }

    pub fn as_bytes(self) -> [u8; NODE_ID_LENGTH] {
        self.0
    }
}

/// Parses the fixed-width hexadecimal network identifier used in configuration.
pub fn network_id_from_hex(value: &str) -> Result<[u8; NETWORK_ID_LENGTH], P2pError> {
    let bytes = hex::decode(value).map_err(|_| P2pError::InvalidNetworkId)?;
    bytes.try_into().map_err(|_| P2pError::InvalidNetworkId)
}

impl fmt::Debug for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

pub(crate) fn node_id_from_public_key(
    network_id: &[u8; NETWORK_ID_LENGTH],
    raw: &[u8],
) -> Result<NodeId, P2pError> {
    let public_key: [u8; 32] = raw
        .try_into()
        .map_err(|_| P2pError::Certificate("certificate key is not Ed25519".to_owned()))?;
    Ok(NodeId::from_public_key(network_id, &public_key))
}
