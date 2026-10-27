use std::{sync::Arc, time::Duration};

use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::{error::P2pError, tls::AnyNodeCertificate};

const ALPN: &[u8] = b"cubecoin/p2p/1";
const QUIC_IDLE_TIMEOUT_MS: u32 = 120_000;
const QUIC_KEEP_ALIVE: Duration = Duration::from_secs(15);

pub(crate) fn server_config(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::ServerConfig, P2pError> {
    let mut tls = rustls::ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(AnyNodeCertificate))
        .with_single_cert(vec![certificate], key)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
        .map_err(|error| P2pError::Certificate(error.to_string()))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    config.transport = transport_config();
    Ok(config)
}

pub(crate) fn client_config(
    certificate: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::ClientConfig, P2pError> {
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyNodeCertificate))
        .with_client_auth_cert(vec![certificate], key)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|error| P2pError::Certificate(error.to_string()))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport_config());
    Ok(config)
}

fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(1u8.into());
    transport.max_concurrent_uni_streams(0u8.into());
    transport.max_idle_timeout(Some(quinn::VarInt::from_u32(QUIC_IDLE_TIMEOUT_MS).into()));
    transport.keep_alive_interval(Some(QUIC_KEEP_ALIVE));
    Arc::new(transport)
}
