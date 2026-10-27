use std::{
    net::{SocketAddr, UdpSocket},
    time::Duration,
};

use tempfile::TempDir;

use crate::{
    config::{BootstrapPeer, NodeConfig},
    node_id::NodeId,
};

use super::NodeHandle;
#[test]
fn node_ids_are_network_separated() {
    let key = [7; 32];
    assert_ne!(
        NodeId::from_public_key(&[1; 32], &key),
        NodeId::from_public_key(&[2; 32], &key)
    );
}
#[test]
fn node_id_hex_round_trip() {
    let id = NodeId::from_public_key(&[1; 32], &[2; 32]);
    assert_eq!(NodeId::from_hex(&id.to_string()).expect("valid id"), id);
}

fn free_loopback_address() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("reserve loopback port");
    socket.local_addr().expect("reserved address")
}

fn test_config(
    root: &TempDir,
    name: &str,
    address: SocketAddr,
    bootstrap: Option<BootstrapPeer>,
) -> NodeConfig {
    NodeConfig {
        network_id: [9; 32],
        address,
        data_dir: root.path().join(name),
        bootstrap,
    }
}

async fn wait_for_peer(node: &NodeHandle, peer: NodeId) {
    for _ in 0..100 {
        if node
            .peers()
            .await
            .into_iter()
            .any(|item| item.node_id == peer && item.verified)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("peer {peer} was not authenticated in time");
}

#[tokio::test]
async fn bootstrap_peer_exchange_connects_local_nodes() {
    let root = TempDir::new().expect("temporary data root");
    let bootstrap = NodeHandle::start(test_config(
        &root,
        "bootstrap",
        free_loopback_address(),
        None,
    ))
    .await
    .expect("start bootstrap");
    let bootstrap_peer = BootstrapPeer {
        address: bootstrap.address(),
        node_id: bootstrap.node_id(),
    };
    let first = NodeHandle::start(test_config(
        &root,
        "first",
        free_loopback_address(),
        Some(bootstrap_peer.clone()),
    ))
    .await
    .expect("start first node");
    wait_for_peer(&bootstrap, first.node_id()).await;
    let second = NodeHandle::start(test_config(
        &root,
        "second",
        free_loopback_address(),
        Some(bootstrap_peer),
    ))
    .await
    .expect("start second node");
    wait_for_peer(&first, second.node_id()).await;
    wait_for_peer(&second, first.node_id()).await;
    first.shutdown();
    second.shutdown();
    bootstrap.shutdown();
}

#[tokio::test]
async fn unverified_peer_exchange_cannot_replace_verified_endpoint() {
    let root = TempDir::new().expect("temporary data root");
    let node = NodeHandle::start(test_config(&root, "node", free_loopback_address(), None))
        .await
        .expect("start node");
    let peer = NodeId::from_public_key(&[9; 32], &[3; 32]);
    let verified = free_loopback_address();
    let hint = free_loopback_address();
    node.remember(peer, verified, true).await;
    node.remember(peer, hint, false).await;
    let snapshot = node
        .peers()
        .await
        .into_iter()
        .find(|item| item.node_id == peer)
        .expect("stored peer");
    assert_eq!(snapshot.address, verified);
    assert!(snapshot.verified);
    node.shutdown();
}
