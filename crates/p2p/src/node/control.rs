//! Authenticated control-stream handshake, peer exchange, and liveness.

use std::{net::IpAddr, time::Duration};

use cubecoin_p2p_proto::v1::{GetPeers, Peers, Ping, Pong, envelope};
use quinn::{RecvStream, SendStream};
use tokio::{sync::mpsc, time};
use tracing::{Instrument, debug, info};

use crate::{
    error::P2pError,
    node_id::NodeId,
    wire::{
        envelope as make_envelope, expect_hello, expect_open, hello, next_frame, read_frames,
        stream_open, write_frame,
    },
};

use super::{HANDSHAKE_TIMEOUT, NodeHandle, peer_book::MAX_PEERS_PER_RESPONSE};

const PING_INTERVAL: Duration = Duration::from_secs(30);
const PING_NONCE_LENGTH: usize = 16;
const PEER_REQUEST_INTERVAL: Duration = Duration::from_secs(10);

impl NodeHandle {
    pub(super) async fn control_session(
        &self,
        mut send: SendStream,
        receive: RecvStream,
        outbound: bool,
        certificate_id: NodeId,
        observed_ip: IpAddr,
    ) -> Result<(), P2pError> {
        let (frames_tx, mut frames_rx) = mpsc::channel(16);
        let _reader =
            tokio::spawn(read_frames(receive, frames_tx).instrument(tracing::Span::current()));
        let handshake = async {
            let mut next_id = 1;
            let local_open = stream_open(next_id);
            next_id += 1;
            let local_hello = hello(
                next_id,
                &self.inner.config.network_id,
                self.node_id(),
                self.address(),
            );
            next_id += 1;
            let remote = if outbound {
                write_frame(&mut send, &local_open).await?;
                write_frame(&mut send, &local_hello).await?;
                expect_open(next_frame(&mut frames_rx).await?)?;
                expect_hello(
                    next_frame(&mut frames_rx).await?,
                    &self.inner.config.network_id,
                    certificate_id,
                    observed_ip,
                )?
            } else {
                expect_open(next_frame(&mut frames_rx).await?)?;
                let remote = expect_hello(
                    next_frame(&mut frames_rx).await?,
                    &self.inner.config.network_id,
                    certificate_id,
                    observed_ip,
                )?;
                write_frame(&mut send, &local_open).await?;
                write_frame(&mut send, &local_hello).await?;
                remote
            };
            Ok::<_, P2pError>((next_id, remote))
        };
        let (mut next_id, remote) = time::timeout(HANDSHAKE_TIMEOUT, handshake)
            .await
            .map_err(|_| P2pError::Protocol("control handshake timed out"))??;
        self.remember(remote.node_id, remote.address, true).await;
        let is_bootstrap = self
            .inner
            .config
            .bootstrap
            .as_ref()
            .is_some_and(|bootstrap| bootstrap.node_id == remote.node_id);
        info!(target: "cubecoin_p2p::node", event = "peer.authenticated", peer = %remote.node_id, address = %remote.address, direction = if outbound { "outbound" } else { "inbound" }, bootstrap = is_bootstrap, observed_ip = %observed_ip, "P2P peer authenticated");
        // Pull peers once after the handshake. Every node supports this, not just bootstrap nodes.
        let peer_request_id = next_id;
        let request = make_envelope(
            next_id,
            0,
            true,
            envelope::Body::GetPeers(GetPeers {
                limit: MAX_PEERS_PER_RESPONSE as u32,
            }),
        );
        next_id += 1;
        write_frame(&mut send, &request).await?;
        debug!(target: "cubecoin_p2p::node", event = "pex.request_sent", peer = %remote.node_id, request_id = peer_request_id, limit = MAX_PEERS_PER_RESPONSE, "peer list requested");

        let mut peer_response_expected = true;
        let mut ping = time::interval(PING_INTERVAL);
        ping.tick().await;
        let mut outstanding_ping: Option<[u8; PING_NONCE_LENGTH]> = None;
        let mut last_peer_request = time::Instant::now() - PEER_REQUEST_INTERVAL;
        loop {
            tokio::select! {
                biased;
                frame = frames_rx.recv() => {
                    let frame = frame.ok_or(P2pError::Protocol("control reader stopped"))??;
                    match frame.body {
                        Some(envelope::Body::Ping(Ping { nonce })) => {
                            if nonce.len() != PING_NONCE_LENGTH { return Err(P2pError::Protocol("invalid ping nonce")); }
                            write_frame(&mut send, &make_envelope(next_id, frame.message_id, false, envelope::Body::Pong(Pong { nonce }))).await?;
                            next_id += 1;
                        }
                        Some(envelope::Body::GetPeers(GetPeers { limit })) => {
                            if last_peer_request.elapsed() < PEER_REQUEST_INTERVAL {
                                return Err(P2pError::Protocol("peer exchange rate limited"));
                            }
                            last_peer_request = time::Instant::now();
                            let peers = self.shareable_peers(remote.node_id, limit).await;
                            debug!(target: "cubecoin_p2p::node", event = "pex.request_received", requester = %remote.node_id, requested = limit, returned = peers.len(), "peer list served");
                            write_frame(&mut send, &make_envelope(next_id, frame.message_id, false, envelope::Body::Peers(Peers { peers }))).await?;
                            next_id += 1;
                        }
                        Some(envelope::Body::Peers(Peers { peers })) if peer_response_expected && frame.reply_to == peer_request_id => {
                            peer_response_expected = false;
                            debug!(target: "cubecoin_p2p::node", event = "pex.response_received", from = %remote.node_id, offered = peers.len(), request_id = frame.reply_to, "peer list received");
                            self.receive_peers(peers).await;
                        }
                        Some(envelope::Body::Pong(Pong { nonce })) => {
                            let nonce: [u8; PING_NONCE_LENGTH] = nonce.try_into().map_err(|_| P2pError::Protocol("invalid pong nonce"))?;
                            if outstanding_ping != Some(nonce) { return Err(P2pError::Protocol("unexpected pong")); }
                            outstanding_ping = None;
                        }
                        Some(envelope::Body::Error(_)) => {}
                        _ => return Err(P2pError::Protocol("unexpected control message")),
                    }
                }
                _ = ping.tick() => {
                    if outstanding_ping.is_some() { return Err(P2pError::Protocol("ping timed out")); }
                    let nonce: [u8; PING_NONCE_LENGTH] = rand::random();
                    write_frame(&mut send, &make_envelope(next_id, 0, false, envelope::Body::Ping(Ping { nonce: nonce.to_vec() }))).await?;
                    next_id += 1;
                    outstanding_ping = Some(nonce);
                }
            }
        }
    }
}
