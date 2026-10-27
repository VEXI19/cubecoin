//! Peer-book storage and peer-exchange record processing.

use std::net::SocketAddr;

use cubecoin_p2p_proto::v1::PeerRecord;
use tracing::debug;

use crate::{
    config::{PeerSnapshot, valid_endpoint},
    node_id::NodeId,
    wire::{endpoint_from_proto, endpoint_to_proto, node_id_from_bytes},
};

use super::{NodeHandle, PeerState};

pub(super) const MAX_PEERS_PER_RESPONSE: usize = 32;
const MAX_PEER_BOOK: usize = 1024;

impl NodeHandle {
    pub async fn peers(&self) -> Vec<PeerSnapshot> {
        let peers = self.inner.peers.lock().await;
        let active = self.inner.active.lock().await;
        peers
            .iter()
            .map(|(node_id, peer)| PeerSnapshot {
                node_id: *node_id,
                address: peer.address,
                verified: peer.verified,
                connected: active.contains_key(node_id),
            })
            .collect()
    }

    pub(super) async fn receive_peers(&self, peers: Vec<PeerRecord>) {
        for record in peers.into_iter().take(MAX_PEERS_PER_RESPONSE) {
            let Ok(node_id) = node_id_from_bytes(&record.node_id) else {
                continue;
            };
            let Some(address) = endpoint_from_proto(record.endpoint) else {
                continue;
            };
            if node_id == self.node_id() {
                continue;
            }
            self.remember(node_id, address, false).await;
        }
        self.maybe_dial_candidates().await;
    }

    pub(super) async fn shareable_peers(
        &self,
        requester: NodeId,
        requested: u32,
    ) -> Vec<PeerRecord> {
        let limit = usize::min(
            requested.clamp(1, MAX_PEERS_PER_RESPONSE as u32) as usize,
            MAX_PEERS_PER_RESPONSE,
        );
        self.inner
            .peers
            .lock()
            .await
            .iter()
            .filter(|(id, peer)| **id != requester && peer.verified)
            .take(limit)
            .map(|(id, peer)| PeerRecord {
                node_id: id.as_bytes().to_vec(),
                endpoint: Some(endpoint_to_proto(peer.address)),
            })
            .collect()
    }

    pub(super) async fn remember(
        &self,
        node_id: NodeId,
        address: SocketAddr,
        verified: bool,
    ) -> bool {
        if node_id == self.node_id() || !valid_endpoint(address) {
            return false;
        }
        let mut peers = self.inner.peers.lock().await;
        if let Some(peer) = peers.get_mut(&node_id) {
            if verified || !peer.verified {
                peer.address = address;
            }
            peer.verified |= verified;
            return false;
        }
        if peers.len() >= MAX_PEER_BOOK {
            return false;
        }
        peers.insert(node_id, PeerState { address, verified });
        debug!(target: "cubecoin_p2p::node", event = "peer.book_inserted", peer = %node_id, address = %address, verified, "peer book entry created");
        true
    }
}
