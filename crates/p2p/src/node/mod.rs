//! Node lifecycle and shared state.
//!
//! Connection admission lives in `session`, outbound work in `dial`, the
//! control-stream protocol in `control`, and persistent peer state in
//! `peer_book`.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use quinn::Endpoint;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::{
    sync::{Mutex, Semaphore},
    time,
};
use tracing::{debug, info};

use crate::{
    config::{NodeConfig, validate_config},
    error::P2pError,
    identity::load_or_create_identity,
    node_id::{NodeId, node_id_from_public_key},
    tls::certificate_for,
    transport::server_config,
};

mod control;
mod dial;
mod peer_book;
mod session;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_INBOUND_HANDSHAKES: usize = 8;

/// A running QUIC node. Cloning this handle does not create another listener.
#[derive(Clone)]
pub struct NodeHandle {
    inner: Arc<Inner>,
}

struct Inner {
    config: NodeConfig,
    node_id: NodeId,
    endpoint: Endpoint,
    certificate: CertificateDer<'static>,
    private_key: PrivateKeyDer<'static>,
    peers: Mutex<HashMap<NodeId, PeerState>>,
    active: Mutex<HashMap<NodeId, ActiveConnection>>,
    inbound_handshakes: Arc<Semaphore>,
    stopping: AtomicBool,
    next_attempt: AtomicU64,
}

#[derive(Clone)]
struct PeerState {
    address: SocketAddr,
    verified: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ConnectionDirection {
    Inbound,
    Outbound,
}

#[derive(Clone, Copy)]
struct ActiveConnection {
    direction: ConnectionDirection,
    token: u64,
    established: bool,
}

impl NodeHandle {
    /// Starts a listener and, when configured, attempts the pinned bootstrap peer.
    pub async fn start(config: NodeConfig) -> Result<Self, P2pError> {
        validate_config(&config)?;
        let identity = load_or_create_identity(&config.data_dir)?;
        let node_id =
            node_id_from_public_key(&config.network_id, identity.key_pair.public_key_raw())?;
        let (certificate, private_key) = certificate_for(&identity.key_pair, config.address.ip())?;
        let server_config = server_config(certificate.clone(), private_key.clone_key())?;
        let endpoint =
            Endpoint::server(server_config, config.address).map_err(|source| P2pError::Io {
                action: "bind QUIC endpoint",
                source,
            })?;
        let inner = Arc::new(Inner {
            config,
            node_id,
            endpoint,
            certificate,
            private_key,
            peers: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            inbound_handshakes: Arc::new(Semaphore::new(MAX_INBOUND_HANDSHAKES)),
            stopping: AtomicBool::new(false),
            next_attempt: AtomicU64::new(1),
        });
        let handle = Self { inner };
        let listener = handle.clone();
        tokio::spawn(async move { listener.accept_loop().await });

        let role = if handle.inner.config.bootstrap.is_some() {
            "peer"
        } else {
            "bootstrap"
        };
        info!(target: "cubecoin_p2p::node", event = "node.listening", node_id = %handle.node_id(), address = %handle.address(), role, "P2P node listening");
        if let Some(bootstrap) = handle.inner.config.bootstrap.clone() {
            info!(target: "cubecoin_p2p::node", event = "bootstrap.configured", bootstrap_node_id = %bootstrap.node_id, bootstrap_addr = %bootstrap.address, "bootstrap discovery configured");
            handle
                .remember(bootstrap.node_id, bootstrap.address, false)
                .await;
            handle
                .schedule_dial(
                    bootstrap.node_id,
                    bootstrap.address,
                    Some(bootstrap.node_id),
                )
                .await;
        }
        let maintainer = handle.clone();
        tokio::spawn(async move { maintainer.maintenance_loop().await });
        Ok(handle)
    }

    pub fn node_id(&self) -> NodeId {
        self.inner.node_id
    }
    pub fn address(&self) -> SocketAddr {
        self.inner
            .endpoint
            .local_addr()
            .unwrap_or(self.inner.config.address)
    }

    pub fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::Release);
        self.inner.endpoint.close(0u32.into(), b"node shutdown");
    }

    async fn accept_loop(&self) {
        while let Some(incoming) = self.inner.endpoint.accept().await {
            let Ok(permit) = self.inner.inbound_handshakes.clone().try_acquire_owned() else {
                debug!(target: "cubecoin_p2p::node", "inbound QUIC handshake limit reached");
                continue;
            };
            let node = self.clone();
            tokio::spawn(async move {
                match time::timeout(HANDSHAKE_TIMEOUT, incoming).await {
                    Err(_) => {
                        debug!(target: "cubecoin_p2p::node", "inbound QUIC handshake timed out")
                    }
                    Ok(Err(error)) => {
                        debug!(target: "cubecoin_p2p::node", %error, "QUIC handshake failed")
                    }
                    Ok(Ok(connection)) => {
                        drop(permit);
                        debug!(target: "cubecoin_p2p::node", event = "quic.inbound_established", remote = %connection.remote_address(), "inbound QUIC connection established");
                        if let Err(error) = node.run_connection(connection, false, None, None).await
                        {
                            debug!(target: "cubecoin_p2p::node", error = %error, "rejected inbound P2P connection");
                        }
                    }
                }
            });
        }
    }

    async fn maybe_dial_candidates(&self) {
        let candidates: Vec<_> = self
            .inner
            .peers
            .lock()
            .await
            .iter()
            .filter(|(id, peer)| **id != self.node_id() && peer.address != self.address())
            .map(|(id, peer)| (*id, peer.address))
            .collect();
        for (id, address) in candidates {
            self.schedule_dial(id, address, Some(id)).await;
        }
    }

    async fn maintenance_loop(&self) {
        let mut interval = time::interval(Duration::from_secs(5));
        interval.tick().await;
        loop {
            interval.tick().await;
            if self.inner.stopping.load(Ordering::Acquire) {
                return;
            }
            self.maybe_dial_candidates().await;
        }
    }
}

#[cfg(test)]
mod tests;
