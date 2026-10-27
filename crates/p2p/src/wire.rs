use std::net::{IpAddr, SocketAddr};

use cubecoin_p2p_proto::v1::{
    Envelope, Hello, PeerEndpoint, StreamKind, StreamOpen, envelope, peer_endpoint,
};
use prost::Message;
use quinn::{RecvStream, SendStream};
use tokio::{sync::mpsc, time};
use tracing::{debug, trace};

use crate::{config::valid_endpoint, error::P2pError, node_id::NodeId};

const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_LENGTH: usize = 64 * 1024;
const FRAME_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) struct RemoteHello {
    pub(crate) node_id: NodeId,
    pub(crate) address: SocketAddr,
}

pub(crate) fn stream_open(message_id: u64) -> Envelope {
    envelope(
        message_id,
        0,
        false,
        envelope::Body::StreamOpen(StreamOpen {
            kind: StreamKind::Control as i32,
            version: PROTOCOL_VERSION,
        }),
    )
}
pub(crate) fn hello(
    message_id: u64,
    network_id: &[u8; 32],
    node_id: NodeId,
    address: SocketAddr,
) -> Envelope {
    envelope(
        message_id,
        0,
        false,
        envelope::Body::Hello(Hello {
            network_id: network_id.to_vec(),
            node_id: node_id.as_bytes().to_vec(),
            endpoint: Some(endpoint_to_proto(address)),
        }),
    )
}
pub(crate) fn envelope(
    message_id: u64,
    reply_to: u64,
    expects_reply: bool,
    body: envelope::Body,
) -> Envelope {
    Envelope {
        version: PROTOCOL_VERSION,
        message_id,
        reply_to,
        expects_reply,
        body: Some(body),
    }
}

pub(crate) async fn write_frame(
    stream: &mut SendStream,
    envelope: &Envelope,
) -> Result<(), P2pError> {
    let bytes = envelope.encode_to_vec();
    if bytes.is_empty() || bytes.len() > MAX_FRAME_LENGTH {
        return Err(P2pError::Protocol("invalid outgoing frame"));
    }
    trace!(
        event = "wire.frame_sent",
        message_id = envelope.message_id,
        reply_to = envelope.reply_to,
        kind = body_kind(&envelope.body),
        expects_reply = envelope.expects_reply,
        bytes = bytes.len(),
        "control frame sent"
    );
    time::timeout(FRAME_WRITE_TIMEOUT, async {
        stream
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .map_err(|_| P2pError::Protocol("write frame length"))?;
        stream
            .write_all(&bytes)
            .await
            .map_err(|_| P2pError::Protocol("write frame"))
    })
    .await
    .map_err(|_| P2pError::Protocol("write frame timed out"))??;
    Ok(())
}

async fn read_frame(stream: &mut RecvStream) -> Result<Envelope, P2pError> {
    let mut length = [0; 4];
    stream
        .read_exact(&mut length)
        .await
        .map_err(|_| P2pError::Protocol("read frame length"))?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME_LENGTH {
        return Err(P2pError::Protocol("invalid frame length"));
    }
    let mut bytes = vec![0; length];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|_| P2pError::Protocol("read frame"))?;
    let envelope = Envelope::decode(bytes.as_slice())
        .map_err(|_| P2pError::Protocol("decode protobuf frame"))?;
    if envelope.version != PROTOCOL_VERSION || envelope.message_id == 0 {
        return Err(P2pError::Protocol("invalid envelope version or id"));
    }
    trace!(
        event = "wire.frame_received",
        message_id = envelope.message_id,
        reply_to = envelope.reply_to,
        kind = body_kind(&envelope.body),
        expects_reply = envelope.expects_reply,
        bytes = length,
        "control frame received"
    );
    Ok(envelope)
}

pub(crate) async fn read_frames(
    mut stream: RecvStream,
    sender: mpsc::Sender<Result<Envelope, P2pError>>,
) {
    loop {
        let frame = read_frame(&mut stream).await;
        let terminal = frame.is_err();
        if sender.send(frame).await.is_err() || terminal {
            return;
        }
    }
}

pub(crate) async fn next_frame(
    receiver: &mut mpsc::Receiver<Result<Envelope, P2pError>>,
) -> Result<Envelope, P2pError> {
    receiver
        .recv()
        .await
        .ok_or(P2pError::Protocol("control reader stopped"))?
}

pub(crate) fn expect_open(envelope: Envelope) -> Result<(), P2pError> {
    match envelope.body {
        Some(envelope::Body::StreamOpen(StreamOpen { kind, version }))
            if kind == StreamKind::Control as i32 && version == PROTOCOL_VERSION =>
        {
            Ok(())
        }
        _ => Err(P2pError::Protocol("missing control stream open")),
    }
}

pub(crate) fn expect_hello(
    envelope: Envelope,
    network_id: &[u8; 32],
    certificate_id: NodeId,
    observed_ip: IpAddr,
) -> Result<RemoteHello, P2pError> {
    let Some(envelope::Body::Hello(Hello {
        network_id: remote_network,
        node_id,
        endpoint,
    })) = envelope.body
    else {
        return Err(P2pError::Protocol("missing hello"));
    };
    if remote_network.as_slice() != network_id {
        return Err(P2pError::Protocol("wrong network"));
    }
    let node_id = node_id_from_bytes(&node_id)?;
    if node_id != certificate_id {
        return Err(P2pError::IdentityMismatch);
    }
    let address = endpoint_from_proto(endpoint).ok_or(P2pError::InvalidEndpoint)?;
    if address.ip() != observed_ip {
        return Err(P2pError::Protocol(
            "advertised endpoint IP differs from connection IP",
        ));
    }
    debug!(event = "hello.validated", peer = %node_id, address = %address, observed_ip = %observed_ip, "hello matches certificate identity");
    Ok(RemoteHello { node_id, address })
}

fn body_kind(body: &Option<envelope::Body>) -> &'static str {
    match body {
        Some(envelope::Body::StreamOpen(_)) => "stream_open",
        Some(envelope::Body::Hello(_)) => "hello",
        Some(envelope::Body::Ping(_)) => "ping",
        Some(envelope::Body::Pong(_)) => "pong",
        Some(envelope::Body::GetPeers(_)) => "get_peers",
        Some(envelope::Body::Peers(_)) => "peers",
        Some(envelope::Body::Error(_)) => "error",
        None => "none",
    }
}

pub(crate) fn node_id_from_bytes(bytes: &[u8]) -> Result<NodeId, P2pError> {
    Ok(NodeId(
        bytes.try_into().map_err(|_| P2pError::InvalidNodeId)?,
    ))
}
pub(crate) fn endpoint_to_proto(endpoint: SocketAddr) -> PeerEndpoint {
    let address = match endpoint.ip() {
        IpAddr::V4(ip) => peer_endpoint::Address::Ipv4(ip.octets().to_vec()),
        IpAddr::V6(ip) => peer_endpoint::Address::Ipv6(ip.octets().to_vec()),
    };
    PeerEndpoint {
        address: Some(address),
        port: u32::from(endpoint.port()),
    }
}
pub(crate) fn endpoint_from_proto(endpoint: Option<PeerEndpoint>) -> Option<SocketAddr> {
    let endpoint = endpoint?;
    let port = u16::try_from(endpoint.port).ok()?;
    let ip = match endpoint.address? {
        peer_endpoint::Address::Ipv4(bytes) => {
            let octets: [u8; 4] = bytes.try_into().ok()?;
            IpAddr::V4(std::net::Ipv4Addr::from(octets))
        }
        peer_endpoint::Address::Ipv6(bytes) => {
            let octets: [u8; 16] = bytes.try_into().ok()?;
            IpAddr::V6(std::net::Ipv6Addr::from(octets))
        }
    };
    let address = SocketAddr::new(ip, port);
    valid_endpoint(address).then_some(address)
}
