//! TXT records of the DNS-01 challenges, on a zone hosted by Vercel DNS.
//!
//! The gateway is reached through a TCP proxy whose ports 80 and 443 are not its own, so it
//! proves that it controls its domain through DNS: it writes the TXT record the certificate
//! authority asks for, waits until every authoritative name server of the zone serves it, and
//! deletes it once the authority has checked it.

use hickory_resolver::{
	config::{NameServerConfig, ResolverConfig, ResolverOpts},
	net::runtime::TokioRuntimeProvider,
	proto::rr::RData,
	TokioResolver,
};
use serde::Deserialize;
use std::{
	net::IpAddr,
	time::{Duration, Instant},
};

const VERCEL_API: &str = "https://api.vercel.com";

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("Vercel API: {0}")]
	Http(#[from] reqwest::Error),
	#[error("Vercel API: {status}: {body}")]
	Api { status: u16, body: String },
	#[error("{0} is not in the zone {1}")]
	OutsideZone(String, String),
	#[error("DNS: {0}")]
	Resolve(String),
	#[error("{0} was not served by every name server of the zone after {1} s")]
	NotPropagated(String, u64),
}

/// The DNS zone of the domain of the gateway, on Vercel DNS.
pub struct Vercel {
	http: reqwest::Client,
	token: String,
	team: Option<String>,
	zone: String,
}

#[derive(Deserialize)]
struct Created {
	uid: String,
}

impl Vercel {
	pub fn new(token: String, team: Option<String>, zone: String) -> Result<Self, Error> {
		let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
		Ok(Self { http, token, team, zone: zone.trim_end_matches('.').to_ascii_lowercase() })
	}

	fn url(&self, path: &str) -> String {
		let mut url = format!("{VERCEL_API}/v2/domains/{}/records{path}", self.zone);
		if let Some(team) = &self.team {
			url.push_str("?teamId=");
			url.push_str(team);
		}
		url
	}

	/// The name of `fqdn` relative to the zone, as Vercel names records.
	fn relative(&self, fqdn: &str) -> Result<String, Error> {
		let fqdn = fqdn.trim_end_matches('.').to_ascii_lowercase();
		fqdn.strip_suffix(&format!(".{}", self.zone))
			.map(str::to_string)
			.ok_or_else(|| Error::OutsideZone(fqdn.clone(), self.zone.clone()))
	}

	/// Adds a TXT record and returns its identifier.
	pub async fn add_txt(&self, fqdn: &str, value: &str) -> Result<String, Error> {
		let body = serde_json::json!({
			"type": "TXT",
			"name": self.relative(fqdn)?,
			"value": value,
			"ttl": 60,
			"comment": "ACME challenge of tesserax-rpc-gateway, deleted once checked",
		});
		let response = self.http.post(self.url("")).bearer_auth(&self.token).json(&body).send().await?;
		let status = response.status();
		if !status.is_success() {
			return Err(Error::Api { status: status.as_u16(), body: response.text().await.unwrap_or_default() });
		}
		Ok(response.json::<Created>().await?.uid)
	}

	pub async fn remove(&self, id: &str) -> Result<(), Error> {
		let response = self.http.delete(self.url(&format!("/{id}"))).bearer_auth(&self.token).send().await?;
		let status = response.status();
		if !status.is_success() && status.as_u16() != 404 {
			return Err(Error::Api { status: status.as_u16(), body: response.text().await.unwrap_or_default() });
		}
		Ok(())
	}

	/// Waits until every authoritative name server of the zone serves `value` in the TXT records
	/// of `fqdn`, which the certificate authority queries directly.
	pub async fn wait_until_served(&self, fqdn: &str, value: &str, timeout: Duration) -> Result<(), Error> {
		let servers = authoritative_servers(&self.zone).await?;
		let start = Instant::now();
		loop {
			let mut served = true;
			for server in &servers {
				if !serves(*server, fqdn, value).await {
					served = false;
					break;
				}
			}
			if served {
				return Ok(());
			}
			if start.elapsed() > timeout {
				return Err(Error::NotPropagated(fqdn.to_string(), timeout.as_secs()));
			}
			tokio::time::sleep(Duration::from_secs(5)).await;
		}
	}
}

/// The addresses of the name servers of `zone`.
async fn authoritative_servers(zone: &str) -> Result<Vec<IpAddr>, Error> {
	let resolver = TokioResolver::builder_tokio()
		.map_err(|e| Error::Resolve(e.to_string()))?
		.build()
		.map_err(|e| Error::Resolve(e.to_string()))?;
	let names: Vec<String> = resolver
		.ns_lookup(format!("{zone}."))
		.await
		.map_err(|e| Error::Resolve(format!("NS of {zone}: {e}")))?
		.answers()
		.iter()
		.filter_map(|record| match &record.data {
			RData::NS(ns) => Some(ns.0.to_ascii()),
			_ => None,
		})
		.collect();
	let mut servers = Vec::new();
	for name in &names {
		let addresses = resolver.lookup_ip(name.as_str()).await.map_err(|e| Error::Resolve(format!("{name}: {e}")))?;
		servers.extend(addresses.iter().filter(IpAddr::is_ipv4));
	}
	if servers.is_empty() {
		return Err(Error::Resolve(format!("no name server found for {zone}")));
	}
	Ok(servers)
}

/// Whether `server` answers `value` among the TXT records of `fqdn`, asked without any cache.
async fn serves(server: IpAddr, fqdn: &str, value: &str) -> bool {
	let config = ResolverConfig::from_parts(None, vec![], vec![NameServerConfig::udp_and_tcp(server)]);
	let mut options = ResolverOpts::default();
	options.cache_size = 0;
	options.attempts = 1;
	let Ok(resolver) =
		TokioResolver::builder_with_config(config, TokioRuntimeProvider::default()).with_options(options).build()
	else {
		return false;
	};
	match resolver.txt_lookup(format!("{}.", fqdn.trim_end_matches('.'))).await {
		Ok(lookup) => lookup.answers().iter().any(|record| match &record.data {
			RData::TXT(txt) => txt.to_string() == value,
			_ => false,
		}),
		Err(_) => false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn names_relative_to_the_zone() {
		let vercel = Vercel::new("token".into(), None, "Tesserax.app.".into()).unwrap();
		assert_eq!(vercel.relative("_acme-challenge.rpc.tesserax.app").unwrap(), "_acme-challenge.rpc");
		assert_eq!(vercel.relative("_acme-challenge.rpc.tesserax.app.").unwrap(), "_acme-challenge.rpc");
		assert!(vercel.relative("_acme-challenge.example.com").is_err());
		assert!(vercel.relative("tesserax.app").is_err());
	}
}
