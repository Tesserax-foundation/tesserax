//! TLS of the gateway: TLS 1.3 only, and a key exchange that always includes ML-KEM-768.
//!
//! Clients that offer ML-KEM-768 on its own (the SDKs and tools of Tesserax) get it; browsers,
//! which only offer it in the hybrid X25519MLKEM768, get the hybrid. A client without ML-KEM
//! fails the handshake: no session of the gateway is protected by classical cryptography alone.

use arc_swap::ArcSwapOption;
use rustls::{
	crypto::{aws_lc_rs, CryptoProvider},
	pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
	server::{ClientHello, ResolvesServerCert},
	sign::CertifiedKey,
	ServerConfig,
};
use std::{fmt, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("certificate chain: {0}")]
	Chain(String),
	#[error("private key: {0}")]
	Key(String),
	#[error("certificate: {0}")]
	Certificate(String),
	#[error("TLS configuration: {0}")]
	Config(#[from] rustls::Error),
}

/// The cryptography of the gateway: that of aws-lc-rs, with ML-KEM in every key exchange.
pub fn provider() -> CryptoProvider {
	CryptoProvider {
		kx_groups: vec![aws_lc_rs::kx_group::MLKEM768, aws_lc_rs::kx_group::X25519MLKEM768],
		..aws_lc_rs::default_provider()
	}
}

/// The cryptography of the connections the gateway opens: X25519MLKEM768 first, classical key
/// exchanges for the servers that offer nothing else.
pub fn outgoing_provider() -> CryptoProvider {
	use aws_lc_rs::kx_group::{SECP256R1, SECP384R1, X25519, X25519MLKEM768};
	CryptoProvider { kx_groups: vec![X25519MLKEM768, X25519, SECP256R1, SECP384R1], ..aws_lc_rs::default_provider() }
}

/// The TLS configuration of the gateway, whose certificate comes from `certificates`.
pub fn server_config(certificates: Arc<Certificates>) -> Result<ServerConfig, Error> {
	let mut config = ServerConfig::builder_with_provider(Arc::new(provider()))
		.with_protocol_versions(&[&rustls::version::TLS13])?
		.with_no_client_auth()
		.with_cert_resolver(certificates);
	// The RPC of the node speaks HTTP/1.1, websockets included.
	config.alpn_protocols = vec![b"http/1.1".to_vec()];
	Ok(config)
}

/// A certificate and its key, as served.
pub struct Certificate {
	pub key: Arc<CertifiedKey>,
	/// Seconds since the Unix epoch.
	pub not_before: i64,
	pub not_after: i64,
}

impl Certificate {
	/// A certificate chain and its private key, PEM encoded, for `domain`.
	pub fn from_pem(chain: &[u8], key: &[u8], domain: &str) -> Result<Self, Error> {
		let chain = CertificateDer::pem_slice_iter(chain)
			.collect::<Result<Vec<_>, _>>()
			.map_err(|e| Error::Chain(e.to_string()))?;
		let leaf = chain.first().ok_or_else(|| Error::Chain("no certificate".into()))?;
		let (not_before, not_after) = check_leaf(leaf, domain)?;
		let key = PrivateKeyDer::from_pem_slice(key).map_err(|e| Error::Key(e.to_string()))?;
		let key = provider().key_provider.load_private_key(key).map_err(|e| Error::Key(e.to_string()))?;
		let key = CertifiedKey::new(chain, key);
		key.keys_match().map_err(|e| Error::Key(e.to_string()))?;
		Ok(Self { key: Arc::new(key), not_before, not_after })
	}

	/// Whether the certificate should be renewed at `now`: once two thirds of its lifetime have
	/// passed, as certificate authorities advise.
	pub fn due(&self, now: i64) -> bool {
		now >= self.not_before + (self.not_after - self.not_before) * 2 / 3
	}
}

/// The validity of the leaf certificate, checked to name `domain`.
fn check_leaf(leaf: &CertificateDer<'_>, domain: &str) -> Result<(i64, i64), Error> {
	let (_, certificate) = x509_parser::parse_x509_certificate(leaf).map_err(|e| Error::Certificate(e.to_string()))?;
	let names = certificate
		.subject_alternative_name()
		.map_err(|e| Error::Certificate(e.to_string()))?
		.map(|san| {
			san.value
				.general_names
				.iter()
				.filter_map(|name| match name {
					x509_parser::extensions::GeneralName::DNSName(name) => Some(name.to_ascii_lowercase()),
					_ => None,
				})
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	if !names.iter().any(|name| name == &domain.to_ascii_lowercase()) {
		return Err(Error::Certificate(format!("not valid for {domain} (names: {})", names.join(", "))));
	}
	let validity = certificate.validity();
	Ok((validity.not_before.timestamp(), validity.not_after.timestamp()))
}

/// The certificate served, replaced in place when it is renewed.
#[derive(Default)]
pub struct Certificates {
	current: ArcSwapOption<Certificate>,
}

impl Certificates {
	pub fn get(&self) -> Option<Arc<Certificate>> {
		self.current.load_full()
	}

	pub fn set(&self, certificate: Certificate) {
		self.current.store(Some(Arc::new(certificate)));
	}
}

impl ResolvesServerCert for Certificates {
	fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
		self.current.load().as_ref().map(|certificate| certificate.key.clone())
	}
}

impl fmt::Debug for Certificates {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Certificates").field("loaded", &self.current.load().is_some()).finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_key_exchange_has_ml_kem() {
		let groups: Vec<_> = provider().kx_groups.iter().map(|group| group.name()).collect();
		assert_eq!(groups, [rustls::NamedGroup::MLKEM768, rustls::NamedGroup::X25519MLKEM768]);
	}

	#[test]
	fn outgoing_connections_prefer_ml_kem() {
		assert_eq!(outgoing_provider().kx_groups[0].name(), rustls::NamedGroup::X25519MLKEM768);
	}
}
