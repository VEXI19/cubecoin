use std::net::IpAddr;

use quinn::Connection;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::{
    DigitallySignedStruct, DistinguishedName as RustlsDistinguishedName, Error as RustlsError,
    SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use tracing::debug;
use x509_parser::parse_x509_certificate;

use crate::{
    error::P2pError,
    node_id::{NodeId, node_id_from_public_key},
};

pub(crate) fn certificate_for(
    key_pair: &KeyPair,
    address: IpAddr,
) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), P2pError> {
    let mut params = CertificateParams::new(Vec::<String>::new())
        .map_err(|error| P2pError::Certificate(error.to_string()))?;
    params.subject_alt_names.push(SanType::IpAddress(address));
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![
        rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        rcgen::ExtendedKeyUsagePurpose::ServerAuth,
    ];
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, "cubecoin-node");
    params.distinguished_name = distinguished_name;
    let certificate = params
        .self_signed(key_pair)
        .map_err(|error| P2pError::Certificate(error.to_string()))?;
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
    debug!(event = "tls.certificate_created", address = %address, algorithm = "ed25519", "self-signed node certificate created");
    Ok((CertificateDer::from(certificate.der().to_vec()), key))
}

pub(crate) fn peer_id_from_connection(
    connection: &Connection,
    network_id: &[u8; 32],
) -> Result<NodeId, P2pError> {
    let identity = connection
        .peer_identity()
        .ok_or(P2pError::Protocol("missing peer certificate"))?;
    let certificates = identity
        .downcast::<Vec<CertificateDer<'static>>>()
        .map_err(|_| P2pError::Protocol("unexpected peer certificate type"))?;
    let certificate = certificates
        .first()
        .ok_or(P2pError::Protocol("empty peer certificate chain"))?;
    node_id_from_certificate(certificate, network_id)
}

fn node_id_from_certificate(
    certificate: &CertificateDer<'_>,
    network_id: &[u8; 32],
) -> Result<NodeId, P2pError> {
    let (_, certificate) = parse_x509_certificate(certificate.as_ref())
        .map_err(|_| P2pError::Certificate("invalid peer certificate".to_owned()))?;
    node_id_from_public_key(
        network_id,
        &certificate
            .tbs_certificate
            .subject_pki
            .subject_public_key
            .data,
    )
}

#[derive(Debug)]
pub(crate) struct AnyNodeCertificate;

impl AnyNodeCertificate {
    fn validate(certificate: &CertificateDer<'_>) -> Result<(), RustlsError> {
        let (_, certificate) = parse_x509_certificate(certificate.as_ref())
            .map_err(|_| RustlsError::General("invalid node certificate".to_owned()))?;
        if certificate
            .tbs_certificate
            .subject_pki
            .algorithm
            .algorithm
            .to_id_string()
            != "1.3.101.112"
            || certificate
                .tbs_certificate
                .subject_pki
                .subject_public_key
                .data
                .len()
                != 32
        {
            debug!(
                event = "tls.certificate_rejected",
                reason = "not_ed25519_or_wrong_key_length",
                "peer certificate rejected"
            );
            return Err(RustlsError::General(
                "node certificate does not contain a valid Ed25519 key".to_owned(),
            ));
        }
        Ok(())
    }
    fn schemes() -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
    fn verify_tls12(
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
        .map(|_| HandshakeSignatureValid::assertion())
    }
    fn verify_tls13(
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
        .map(|_| HandshakeSignatureValid::assertion())
    }
}

impl ServerCertVerifier for AnyNodeCertificate {
    fn verify_server_cert(
        &self,
        certificate: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Self::validate(certificate)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Self::verify_tls12(message, certificate, signature)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Self::verify_tls13(message, certificate, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        Self::schemes()
    }
}

impl ClientCertVerifier for AnyNodeCertificate {
    fn root_hint_subjects(&self) -> &[RustlsDistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        certificate: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, RustlsError> {
        Self::validate(certificate)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Self::verify_tls12(message, certificate, signature)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Self::verify_tls13(message, certificate, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        Self::schemes()
    }
}
