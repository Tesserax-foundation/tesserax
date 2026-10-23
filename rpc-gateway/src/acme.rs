//! Certificate of the gateway from an ACME certificate authority (Let's Encrypt), obtained with
//! DNS-01 challenges and renewed in the background.
//!
//! The state directory keeps the ACME account (`account.json`), the certificate chain
//! (`certificate.pem`) and its key (`key.pem`), readable by the gateway only.

use crate::{
	dns::Vercel,
	tls::{Certificate, Certificates},
};
use instant_acme::{
	Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, NewAccount, NewOrder, OrderStatus,
	RetryPolicy,
};
use std::{
	path::{Path, PathBuf},
	sync::Arc,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("ACME: {0}")]
	Acme(#[from] instant_acme::Error),
	#[error(transparent)]
	Dns(#[from] crate::dns::Error),
	#[error(transparent)]
	Tls(#[from] crate::tls::Error),
	#[error("{0}: {1}")]
	Io(PathBuf, std::io::Error),
	#[error("account.json: {0}")]
	Account(serde_json::Error),
	#[error("order {0:?}: {1}")]
	Order(OrderStatus, String),
	#[error("authorization of {0}: {1:?}")]
	Authorization(String, AuthorizationStatus),
	#[error("no DNS-01 challenge offered for {0}")]
	NoDnsChallenge(String),
}

pub struct Acme {
	pub domain: String,
	pub directory: String,
	pub contact: Option<String>,
	pub state: PathBuf,
	pub dns: Vercel,
}

/// How long the TXT record of a challenge may take to reach every name server of the zone.
const PROPAGATION: Duration = Duration::from_secs(300);

fn now() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Acme {
	fn path(&self, name: &str) -> PathBuf {
		self.state.join(name)
	}

	/// The certificate of the state directory, if it is there and valid for the domain.
	pub async fn stored(&self) -> Option<Certificate> {
		let chain = tokio::fs::read(self.path("certificate.pem")).await.ok()?;
		let key = tokio::fs::read(self.path("key.pem")).await.ok()?;
		match Certificate::from_pem(&chain, &key, &self.domain) {
			Ok(certificate) if certificate.not_after > now() => Some(certificate),
			Ok(_) => None,
			Err(e) => {
				tracing::warn!("Stored certificate ignored: {e}");
				None
			},
		}
	}

	/// Serves the stored certificate, or obtains one, then renews it whenever it is due. Returns
	/// once a certificate is served; renewals go on in the background.
	pub async fn run(self, certificates: Arc<Certificates>) -> Result<(), Error> {
		tokio::fs::create_dir_all(&self.state).await.map_err(|e| Error::Io(self.state.clone(), e))?;
		match self.stored().await {
			Some(certificate) => {
				tracing::info!("Certificate of {} valid until {}", self.domain, certificate.not_after);
				certificates.set(certificate);
			},
			None => {
				let certificate = self.obtain().await?;
				certificates.set(certificate);
			},
		}
		tokio::spawn(async move {
			loop {
				tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
				let due = certificates.get().is_none_or(|certificate| certificate.due(now()));
				if !due {
					continue;
				}
				match self.obtain().await {
					Ok(certificate) => certificates.set(certificate),
					// The current certificate stays served; the next round tries again.
					Err(e) => tracing::error!("Renewing the certificate of {}: {e}", self.domain),
				}
			}
		});
		Ok(())
	}

	async fn account(&self) -> Result<Account, Error> {
		let path = self.path("account.json");
		match tokio::fs::read(&path).await {
			Ok(bytes) => {
				let credentials: AccountCredentials = serde_json::from_slice(&bytes).map_err(Error::Account)?;
				Ok(Account::builder()?.from_credentials(credentials).await?)
			},
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
				let contact = self.contact.as_ref().map(|email| format!("mailto:{email}"));
				let contact: Vec<&str> = contact.iter().map(String::as_str).collect();
				let (account, credentials) = Account::builder()?
					.create(
						&NewAccount { contact: &contact, terms_of_service_agreed: true, only_return_existing: false },
						self.directory.clone(),
						None,
					)
					.await?;
				let json = serde_json::to_vec_pretty(&credentials).map_err(Error::Account)?;
				write_private(&path, &json).await?;
				tracing::info!("ACME account created at {}", self.directory);
				Ok(account)
			},
			Err(e) => Err(Error::Io(path, e)),
		}
	}

	/// Obtains a new certificate and writes it to the state directory.
	async fn obtain(&self) -> Result<Certificate, Error> {
		tracing::info!("Obtaining a certificate for {}", self.domain);
		let account = self.account().await?;
		let mut order = account.new_order(&NewOrder::new(&[Identifier::Dns(self.domain.clone())])).await?;

		let mut records = Vec::new();
		let result = self.authorize(&mut order, &mut records).await;
		for id in records {
			if let Err(e) = self.dns.remove(&id).await {
				tracing::warn!("Deleting the challenge record {id}: {e}");
			}
		}
		result?;

		let key = order.finalize().await?;
		let chain = order.poll_certificate(&RetryPolicy::new().timeout(Duration::from_secs(180))).await?;
		let certificate = Certificate::from_pem(chain.as_bytes(), key.as_bytes(), &self.domain)?;
		write_private(&self.path("key.pem"), key.as_bytes()).await?;
		write_private(&self.path("certificate.pem"), chain.as_bytes()).await?;
		tracing::info!("Certificate of {} obtained, valid until {}", self.domain, certificate.not_after);
		Ok(certificate)
	}

	/// Answers the DNS-01 challenges of the order, pushing the identifiers of the TXT records
	/// written to `records`, and waits for the order to be ready.
	async fn authorize(&self, order: &mut instant_acme::Order, records: &mut Vec<String>) -> Result<(), Error> {
		let mut authorizations = order.authorizations();
		while let Some(result) = authorizations.next().await {
			let mut authorization = result?;
			match authorization.status {
				AuthorizationStatus::Pending => {},
				AuthorizationStatus::Valid => continue,
				status => return Err(Error::Authorization(self.domain.clone(), status)),
			}
			let mut challenge = authorization
				.challenge(ChallengeType::Dns01)
				.ok_or_else(|| Error::NoDnsChallenge(self.domain.clone()))?;
			let name = format!("_acme-challenge.{}", challenge.identifier().identifier_name());
			let value = challenge.key_authorization().dns_value();
			records.push(self.dns.add_txt(&name, &value).await?);
			self.dns.wait_until_served(&name, &value, PROPAGATION).await?;
			challenge.set_ready().await?;
		}
		let status = order.poll_ready(&RetryPolicy::new().timeout(Duration::from_secs(180))).await?;
		if status != OrderStatus::Ready {
			let state = order.refresh().await.map(|state| format!("{:?}", state.error)).unwrap_or_default();
			return Err(Error::Order(status, state));
		}
		Ok(())
	}
}

trait IdentifierName {
	fn identifier_name(&self) -> String;
}

impl IdentifierName for instant_acme::AuthorizedIdentifier<'_> {
	fn identifier_name(&self) -> String {
		match self.identifier {
			Identifier::Dns(name) => name.clone(),
			other => format!("{other:?}"),
		}
	}
}

/// Writes a file readable by its owner only, atomically.
async fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
	let partial = path.with_extension("partial");
	let io = |e| Error::Io(path.to_path_buf(), e);
	tokio::fs::write(&partial, bytes).await.map_err(io)?;
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		tokio::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o600)).await.map_err(io)?;
	}
	tokio::fs::rename(&partial, path).await.map_err(io)
}
