// Copyright 2026 Tesserax developers
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! Post-quantum connection security.
//!
//! Connections are secured with TLS 1.3 (RFC 8446) restricted to post-quantum algorithms:
//!
//! - key exchange: ML-KEM-768 only (FIPS 203), no elliptic curve or hybrid group;
//! - authentication: ML-DSA-87 only (FIPS 204), mutual;
//! - record protection: AES-256-GCM or ChaCha20-Poly1305.
//!
//! Peers authenticate with raw public keys (RFC 7250) instead of X.509 certificates: each side
//! presents the `SubjectPublicKeyInfo` of its ML-DSA-87 node identity key and proves possession of
//! the private key in the TLS `CertificateVerify` message. The remote [`PeerId`] is derived from
//! that key. Session resumption and early data are disabled.

use crate::{
    config::Role,
    crypto::ml_dsa::{Keypair, PublicKey},
    error::NegotiationError,
    PeerId,
};

use futures::io::{AsyncRead, AsyncWrite};
use futures_rustls::{TlsAcceptor, TlsConnector, TlsStream};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{
        aws_lc_rs as provider, verify_tls13_signature_with_raw_key, CryptoProvider,
        WebPkiSupportedAlgorithms,
    },
    pki_types::{
        CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, SubjectPublicKeyInfoDer,
        UnixTime,
    },
    server::danger::{ClientCertVerified, ClientCertVerifier},
    sign::CertifiedKey,
    ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
};

use std::{sync::Arc, time::Duration};

/// Logging target for the file.
const LOG_TARGET: &str = "litep2p::crypto::tls";

/// Protocol negotiated with multistream-select before the TLS handshake.
pub(crate) const PROTOCOL_NAME: &str = "/tesserax/tls/1.0.0";

/// ALPN protocol of Tesserax TLS connections.
const ALPN: &[u8] = b"tesserax";

/// Server name sent by dialers. Peers are identified by their key, not by a name.
const SERVER_NAME: &str = "tesserax";

/// The only signature algorithm accepted: ML-DSA-87.
static SIGNATURE_ALGORITHMS: WebPkiSupportedAlgorithms = WebPkiSupportedAlgorithms {
    all: &[webpki::aws_lc_rs::ML_DSA_87],
    mapping: &[(SignatureScheme::ML_DSA_87, &[webpki::aws_lc_rs::ML_DSA_87])],
};

/// Crypto provider restricted to post-quantum key exchange and authentication.
fn crypto_provider() -> Arc<CryptoProvider> {
    Arc::new(CryptoProvider {
        cipher_suites: vec![
            provider::cipher_suite::TLS13_AES_256_GCM_SHA384,
            provider::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
        ],
        kx_groups: vec![provider::kx_group::MLKEM768],
        signature_verification_algorithms: SIGNATURE_ALGORITHMS,
        ..provider::default_provider()
    })
}

/// The local identity presented to the remote peer.
fn certified_key(keypair: &Keypair) -> Result<Arc<CertifiedKey>, NegotiationError> {
    let private_key =
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(keypair.to_pkcs8_der()));
    let signing_key = provider::sign::any_supported_type(&private_key)?;
    Ok(Arc::new(CertifiedKey::new(
        vec![CertificateDer::from(keypair.public_key_der())],
        signing_key,
    )))
}

/// Accepts any ML-DSA-87 raw public key. The remote identity is checked against the expected
/// `PeerId` by the caller once the handshake is complete.
#[derive(Debug)]
struct PeerKeyVerifier;

impl PeerKeyVerifier {
    fn check_key(key: &CertificateDer<'_>) -> Result<(), rustls::Error> {
        PublicKey::try_from_spki_der(key.as_ref())
            .map(|_| ())
            .map_err(|_| rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding))
    }

    fn verify_signature(
        message: &[u8],
        key: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        if dss.scheme != SignatureScheme::ML_DSA_87 {
            return Err(rustls::PeerMisbehaved::SignedHandshakeWithUnadvertisedSigScheme.into());
        }
        verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(key.as_ref()),
            dss,
            &SIGNATURE_ALGORITHMS,
        )
    }
}

impl ServerCertVerifier for PeerKeyVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if !intermediates.is_empty() {
            return Err(rustls::Error::General("unexpected certificate chain".into()));
        }
        Self::check_key(end_entity)?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Self::verify_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ML_DSA_87]
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

impl ClientCertVerifier for PeerKeyVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        if !intermediates.is_empty() {
            return Err(rustls::Error::General("unexpected certificate chain".into()));
        }
        Self::check_key(end_entity)?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Self::verify_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ML_DSA_87]
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

fn client_config(keypair: &Keypair) -> Result<ClientConfig, NegotiationError> {
    let mut config = ClientConfig::builder_with_provider(crypto_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PeerKeyVerifier))
        .with_client_cert_resolver(Arc::new(
            rustls::client::AlwaysResolvesClientRawPublicKeys::new(certified_key(keypair)?),
        ));
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.resumption = rustls::client::Resumption::disabled();
    config.enable_early_data = false;
    Ok(config)
}

fn server_config(keypair: &Keypair) -> Result<ServerConfig, NegotiationError> {
    let mut config = ServerConfig::builder_with_provider(crypto_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(Arc::new(PeerKeyVerifier))
        .with_cert_resolver(Arc::new(rustls::server::AlwaysResolvesServerRawPublicKeys::new(
            certified_key(keypair)?,
        )));
    config.alpn_protocols = vec![ALPN.to_vec()];
    config.send_tls13_tickets = 0;
    config.max_early_data_size = 0;
    Ok(config)
}

/// Identity of the remote peer of an established TLS connection.
fn remote_peer_id<S>(stream: &TlsStream<S>) -> Result<PeerId, NegotiationError> {
    let (_, connection) = stream.get_ref();
    if connection.negotiated_key_exchange_group().map(|group| group.name())
        != Some(rustls::NamedGroup::MLKEM768)
    {
        return Err(NegotiationError::TlsError("unexpected key exchange group".into()));
    }
    if connection.alpn_protocol() != Some(ALPN) {
        return Err(NegotiationError::TlsError("unexpected application protocol".into()));
    }
    let key = connection
        .peer_certificates()
        .and_then(|keys| keys.first())
        .ok_or(NegotiationError::PeerIdMissing)?;
    Ok(PublicKey::try_from_spki_der(key.as_ref())?.to_peer_id())
}

/// Perform the TLS handshake over `io` and return the secured stream with the remote `PeerId`.
pub(crate) async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(
    io: S,
    keypair: &Keypair,
    role: Role,
    timeout: Duration,
) -> Result<(TlsStream<S>, PeerId), NegotiationError> {
    let handshake = async move {
        tracing::debug!(target: LOG_TARGET, ?role, "start TLS handshake");

        let stream = match role {
            Role::Dialer => {
                let connector = TlsConnector::from(Arc::new(client_config(keypair)?));
                let server_name = ServerName::try_from(SERVER_NAME)
                    .expect("server name is a valid DNS name; qed");
                TlsStream::Client(connector.connect(server_name, io).await.map_err(|error| {
                    NegotiationError::TlsError(error.to_string())
                })?)
            }
            Role::Listener => {
                let acceptor = TlsAcceptor::from(Arc::new(server_config(keypair)?));
                TlsStream::Server(acceptor.accept(io).await.map_err(|error| {
                    NegotiationError::TlsError(error.to_string())
                })?)
            }
        };

        let peer = remote_peer_id(&stream)?;
        tracing::debug!(target: LOG_TARGET, ?role, ?peer, "TLS handshake done");
        Ok((stream, peer))
    };

    match tokio::time::timeout(timeout, handshake).await {
        Err(_) => Err(NegotiationError::Timeout),
        Ok(result) => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::{TcpListener, TcpStream};
    use tokio_util::compat::TokioAsyncReadCompatExt;

    async fn connect(
        dialer: &Keypair,
        listener: &Keypair,
    ) -> (Result<PeerId, NegotiationError>, Result<PeerId, NegotiationError>) {
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let (client, server) =
            tokio::join!(TcpStream::connect(address), async { server.accept().await.unwrap().0 });
        let (client, server) = (client.unwrap().compat(), server.compat());
        let timeout = Duration::from_secs(10);

        let (dialed, accepted) = tokio::join!(
            handshake(client, dialer, Role::Dialer, timeout),
            handshake(server, listener, Role::Listener, timeout),
        );
        (dialed.map(|(_, peer)| peer), accepted.map(|(_, peer)| peer))
    }

    #[tokio::test]
    async fn mutual_authentication() {
        let dialer = Keypair::generate();
        let listener = Keypair::generate();

        let (dialed, accepted) = connect(&dialer, &listener).await;
        assert_eq!(dialed.unwrap(), listener.public().to_peer_id());
        assert_eq!(accepted.unwrap(), dialer.public().to_peer_id());
    }

    #[tokio::test]
    async fn data_flows_both_ways() {
        use futures::{AsyncReadExt, AsyncWriteExt};

        let dialer = Keypair::generate();
        let listener = Keypair::generate();
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let (client, server) =
            tokio::join!(TcpStream::connect(address), async { server.accept().await.unwrap().0 });
        let timeout = Duration::from_secs(10);
        let (dialed, accepted) = tokio::join!(
            handshake(client.unwrap().compat(), &dialer, Role::Dialer, timeout),
            handshake(server.compat(), &listener, Role::Listener, timeout),
        );
        let (mut client, _) = dialed.unwrap();
        let (mut server, _) = accepted.unwrap();

        client.write_all(b"ping").await.unwrap();
        client.flush().await.unwrap();
        let mut buffer = [0u8; 4];
        server.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"ping");

        server.write_all(b"pong").await.unwrap();
        server.flush().await.unwrap();
        client.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"pong");
    }

    #[tokio::test]
    async fn rejects_classical_key_exchange() {
        // A client offering only X25519 (with an otherwise valid ML-DSA identity) is refused.
        let dialer = Keypair::generate();
        let listener = Keypair::generate();
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        let (client, server) =
            tokio::join!(TcpStream::connect(address), async { server.accept().await.unwrap().0 });

        let mut classical = CryptoProvider { ..(*crypto_provider()).clone() };
        classical.kx_groups = vec![provider::kx_group::X25519];
        let config = ClientConfig::builder_with_provider(Arc::new(classical))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PeerKeyVerifier))
            .with_client_cert_resolver(Arc::new(
                rustls::client::AlwaysResolvesClientRawPublicKeys::new(
                    certified_key(&dialer).unwrap(),
                ),
            ));
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from(SERVER_NAME).unwrap();

        let (dialed, accepted) = tokio::join!(
            connector.connect(server_name, client.unwrap().compat()),
            handshake(server.compat(), &listener, Role::Listener, Duration::from_secs(10)),
        );
        assert!(dialed.is_err());
        assert!(accepted.is_err());
    }

    #[test]
    fn provider_is_post_quantum_only() {
        let provider = crypto_provider();
        assert_eq!(provider.kx_groups.len(), 1);
        assert_eq!(provider.kx_groups[0].name(), rustls::NamedGroup::MLKEM768);
        assert!(provider
            .cipher_suites
            .iter()
            .all(|suite| suite.version() == &rustls::version::TLS13));
        assert_eq!(provider.signature_verification_algorithms.mapping.len(), 1);
    }
}
