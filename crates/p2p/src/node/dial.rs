//! Outbound QUIC dialing and dial-task scheduling.

use std::net::SocketAddr;

use tokio::time;
use tracing::debug;

use crate::{error::P2pError, node_id::NodeId, transport::client_config};

use super::{ConnectionDirection, HANDSHAKE_TIMEOUT, NodeHandle};

impl NodeHandle {
    pub(super) async fn schedule_dial(
        &self,
        claimed_id: NodeId,
        address: SocketAddr,
        expected_id: Option<NodeId>,
    ) {
        let Some(token) = self.reserve_outbound(claimed_id).await else {
            return;
        };
        debug!(target: "cubecoin_p2p::node", event = "discovery.dial_scheduled", peer = %claimed_id, address = %address, expected = ?expected_id, "dial scheduled");
        self.spawn_reserved_dial(claimed_id, address, expected_id, token);
    }

    fn spawn_reserved_dial(
        &self,
        claimed_id: NodeId,
        address: SocketAddr,
        expected_id: Option<NodeId>,
        token: u64,
    ) {
        let node = self.clone();
        tokio::spawn(async move {
            if let Err(error) = node
                .dial_reserved(claimed_id, address, expected_id, token)
                .await
            {
                debug!(target: "cubecoin_p2p::node", peer = %claimed_id, %address, error = %error, "P2P dial failed");
            }
        });
    }

    async fn dial_reserved(
        &self,
        claimed_id: NodeId,
        address: SocketAddr,
        expected_id: Option<NodeId>,
        token: u64,
    ) -> Result<(), P2pError> {
        let result = async {
            let config = client_config(
                self.inner.certificate.clone(),
                self.inner.private_key.clone_key(),
            )?;
            let connecting = self
                .inner
                .endpoint
                .connect_with(config, address, "cubecoin")
                .map_err(|_| P2pError::Protocol("could not begin QUIC connection"))?;
            let connection = time::timeout(HANDSHAKE_TIMEOUT, connecting)
                .await
                .map_err(|_| P2pError::Protocol("QUIC handshake timed out"))??;
            self.run_connection(connection, true, expected_id, Some(token))
                .await
        }
        .await;
        self.release_connection(claimed_id, ConnectionDirection::Outbound, token)
            .await;
        result
    }
}
