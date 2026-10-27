//! Connection lifecycle, slot reservation, and duplicate-dial resolution.

use std::sync::atomic::Ordering;

use quinn::Connection;
use tokio::time;
use tracing::{Instrument, debug, info};

use crate::{error::P2pError, node_id::NodeId, tls::peer_id_from_connection};

use super::{ActiveConnection, ConnectionDirection, HANDSHAKE_TIMEOUT, NodeHandle};

const TARGET_OUTBOUND: usize = 8;
const MAX_CONNECTIONS: usize = 24;

impl NodeHandle {
    pub(super) async fn run_connection(
        &self,
        connection: Connection,
        outbound: bool,
        expected_id: Option<NodeId>,
        outbound_token: Option<u64>,
    ) -> Result<(), P2pError> {
        let started = time::Instant::now();
        let certificate_id = peer_id_from_connection(&connection, &self.inner.config.network_id)?;
        let direction_name = if outbound { "outbound" } else { "inbound" };
        debug!(target: "cubecoin_p2p::node", event = "tls.peer_identity", peer = %certificate_id, remote = %connection.remote_address(), direction = direction_name, expected = ?expected_id, "peer certificate identity derived");
        if certificate_id == self.node_id() {
            return Err(P2pError::Protocol("self connection"));
        }
        if let Some(expected_id) = expected_id
            && certificate_id != expected_id
        {
            debug!(target: "cubecoin_p2p::node", event = "tls.identity_rejected", peer = %certificate_id, expected = %expected_id, reason = "expected_mismatch", "peer identity mismatch");
            return Err(P2pError::IdentityMismatch);
        }
        let direction = if outbound {
            let token = outbound_token.ok_or(P2pError::Protocol("missing outbound reservation"))?;
            if !self.activate_outbound(certificate_id, token).await {
                return Err(P2pError::Protocol("duplicate connection"));
            }
            (ConnectionDirection::Outbound, token)
        } else {
            let Some(token) = self.accept_inbound(certificate_id).await else {
                return Err(P2pError::Protocol("duplicate or excess connection"));
            };
            (ConnectionDirection::Inbound, token)
        };

        let result = async {
            let (send, receive) = if outbound {
                time::timeout(HANDSHAKE_TIMEOUT, connection.open_bi())
                    .await
                    .map_err(|_| P2pError::Protocol("open control stream timed out"))?
                    .map_err(|_| P2pError::Protocol("open control stream"))?
            } else {
                time::timeout(HANDSHAKE_TIMEOUT, connection.accept_bi())
                    .await
                    .map_err(|_| P2pError::Protocol("control stream timed out"))?
                    .map_err(|_| P2pError::Protocol("accept control stream"))?
            };
            self.control_session(send, receive, outbound, certificate_id, connection.remote_address().ip())
                .instrument(tracing::info_span!(target: "cubecoin_p2p::node", "p2p.control", peer = %certificate_id, direction = direction_name, observed_ip = %connection.remote_address().ip()))
                .await
        }
        .await;
        connection.close(0u32.into(), b"control session ended");
        self.release_connection(certificate_id, direction.0, direction.1)
            .await;
        info!(target: "cubecoin_p2p::node", event = "peer.disconnected", peer = %certificate_id, direction = direction_name, reason = if result.is_ok() { "control_session_ended" } else { "error" }, elapsed_ms = started.elapsed().as_millis() as u64, "P2P peer disconnected");
        result.map(|_| ())
    }

    pub(super) async fn reserve_outbound(&self, node_id: NodeId) -> Option<u64> {
        let mut active = self.inner.active.lock().await;
        let outbound = active
            .values()
            .filter(|connection| connection.direction == ConnectionDirection::Outbound)
            .count();
        if active.len() >= MAX_CONNECTIONS
            || outbound >= TARGET_OUTBOUND
            || active.contains_key(&node_id)
        {
            let reason = if active.len() >= MAX_CONNECTIONS {
                "connection_limit"
            } else if outbound >= TARGET_OUTBOUND {
                "outbound_limit"
            } else {
                "already_connected"
            };
            debug!(target: "cubecoin_p2p::node", event = "discovery.dial_skipped", peer = %node_id, reason, active = active.len(), outbound, "dial skipped");
            return None;
        }
        let token = self.inner.next_attempt.fetch_add(1, Ordering::Relaxed);
        active.insert(
            node_id,
            ActiveConnection {
                direction: ConnectionDirection::Outbound,
                token,
                established: false,
            },
        );
        Some(token)
    }

    pub(super) async fn activate_outbound(&self, node_id: NodeId, token: u64) -> bool {
        let mut active = self.inner.active.lock().await;
        let Some(connection) = active.get_mut(&node_id) else {
            return false;
        };
        if connection.direction != ConnectionDirection::Outbound || connection.token != token {
            return false;
        }
        connection.established = true;
        true
    }

    pub(super) async fn accept_inbound(&self, node_id: NodeId) -> Option<u64> {
        let mut active = self.inner.active.lock().await;
        match active.get(&node_id) {
            None if active.len() < MAX_CONNECTIONS => {
                let token = self.inner.next_attempt.fetch_add(1, Ordering::Relaxed);
                active.insert(
                    node_id,
                    ActiveConnection {
                        direction: ConnectionDirection::Inbound,
                        token,
                        established: true,
                    },
                );
                Some(token)
            }
            Some(connection)
                if connection.direction == ConnectionDirection::Outbound
                    && !connection.established
                    && self.node_id() > node_id =>
            {
                // When both peers dial, the lower NodeId's outbound connection wins.
                let token = self.inner.next_attempt.fetch_add(1, Ordering::Relaxed);
                active.insert(
                    node_id,
                    ActiveConnection {
                        direction: ConnectionDirection::Inbound,
                        token,
                        established: true,
                    },
                );
                Some(token)
            }
            _ => None,
        }
    }

    pub(super) async fn release_connection(
        &self,
        node_id: NodeId,
        direction: ConnectionDirection,
        token: u64,
    ) {
        let mut active = self.inner.active.lock().await;
        if active.get(&node_id).is_some_and(|connection| {
            connection.direction == direction && connection.token == token
        }) {
            active.remove(&node_id);
        }
    }
}
