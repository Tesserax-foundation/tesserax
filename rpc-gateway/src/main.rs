//! Public RPC gateway of Tesserax.
//!
//! Terminates TLS 1.3 with ML-KEM in every key exchange (ML-KEM-768, or X25519MLKEM768 for
//! browsers) and relays the connections to the RPC of a node, which serves the safe methods only.
//! Its certificate comes from Let's Encrypt through DNS-01 challenges on the DNS zone of the
//! domain, and is renewed in the background; a certificate and key in PEM files can be given
//! instead.
//!
//! ```text
//! VERCEL_TOKEN=<token> tesserax-rpc-gateway --domain rpc.example.com --dns-zone example.com \
//!     --upstream 127.0.0.1:9945 --state /var/lib/tesserax-rpc-gateway
//! ```

mod acme;
mod dns;
mod proxy;
mod tls;

use clap::Parser;
use std::{
	path::PathBuf,
	sync::{atomic::Ordering, Arc},
	time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore};

#[derive(Parser, Debug)]
#[command(name = "tesserax-rpc-gateway", version, about = "Public RPC gateway of Tesserax, with post-quantum TLS")]
struct Cli {
	/// Address the gateway listens on. `[::]` takes IPv4 and IPv6 connections.
	#[arg(long, env = "LISTEN", default_value = "[::]:8443")]
	listen: String,
	/// RPC of the node, `host:port`: an endpoint serving the safe methods only, with a rate limit.
	#[arg(long, env = "UPSTREAM")]
	upstream: String,
	/// Domain of the gateway, which its certificate names.
	#[arg(long, env = "DOMAIN")]
	domain: String,
	/// Certificate chain, PEM. With `--key`, served instead of obtaining one.
	#[arg(long, env = "CERTIFICATE", requires = "key")]
	certificate: Option<PathBuf>,
	/// Private key of `--certificate`, PEM.
	#[arg(long, env = "KEY", requires = "certificate")]
	key: Option<PathBuf>,
	/// Directory of the ACME account and of the certificate obtained.
	#[arg(long, env = "STATE", default_value = "/data")]
	state: PathBuf,
	/// ACME directory of the certificate authority.
	#[arg(long, env = "ACME_DIRECTORY", default_value = "https://acme-v02.api.letsencrypt.org/directory")]
	acme_directory: String,
	/// Email the certificate authority may write to about the account.
	#[arg(long, env = "ACME_CONTACT")]
	acme_contact: Option<String>,
	/// DNS zone of the domain, on Vercel DNS, where the DNS-01 challenges are written. The token
	/// of the Vercel API is read from the environment variable VERCEL_TOKEN only.
	#[arg(long, env = "DNS_ZONE")]
	dns_zone: Option<String>,
	/// Vercel team owning the zone, if it is not a personal account.
	#[arg(long, env = "VERCEL_TEAM")]
	vercel_team: Option<String>,
	/// Connections open at once.
	#[arg(long, env = "MAX_CONNECTIONS", default_value_t = 4096)]
	max_connections: usize,
	/// Seconds a client has to complete its TLS handshake.
	#[arg(long, env = "HANDSHAKE_TIMEOUT", default_value_t = 10)]
	handshake_timeout: u64,
}

#[tokio::main]
async fn main() {
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
		.init();
	if let Err(e) = run(Cli::parse()).await {
		tracing::error!("{e}");
		std::process::exit(1);
	}
}

async fn run(cli: Cli) -> Result<(), String> {
	// Outgoing connections, to the certificate authority and the DNS API, offer X25519MLKEM768
	// first but may fall back to classical key exchanges: the API of Let's Encrypt offers no
	// ML-KEM. They carry nothing secret: account requests signed by the gateway, the certificate
	// request and the certificate, all public. The TLS the gateway serves is set apart, in
	// `tls::server_config`, and always has ML-KEM.
	let _ = tls::outgoing_provider().install_default();

	let certificates = Arc::new(tls::Certificates::default());
	match (&cli.certificate, &cli.key) {
		(Some(certificate), Some(key)) => {
			let chain = std::fs::read(certificate).map_err(|e| format!("{}: {e}", certificate.display()))?;
			let key = std::fs::read(key).map_err(|e| format!("{}: {e}", key.display()))?;
			certificates.set(tls::Certificate::from_pem(&chain, &key, &cli.domain).map_err(|e| e.to_string())?);
		},
		_ => {
			let zone = cli.dns_zone.clone().ok_or("set --dns-zone, or --certificate and --key")?;
			let token = std::env::var("VERCEL_TOKEN").map_err(|_| "set VERCEL_TOKEN to a token of the Vercel API")?;
			// The secret leaves the environment once read.
			std::env::remove_var("VERCEL_TOKEN");
			let acme = acme::Acme {
				domain: cli.domain.clone(),
				directory: cli.acme_directory.clone(),
				contact: cli.acme_contact.clone(),
				state: cli.state.clone(),
				dns: dns::Vercel::new(token, cli.vercel_team.clone(), zone).map_err(|e| e.to_string())?,
			};
			acme.run(certificates.clone()).await.map_err(|e| e.to_string())?;
		},
	}

	let config = tls::server_config(certificates).map_err(|e| e.to_string())?;
	let listener = TcpListener::bind(&cli.listen).await.map_err(|e| format!("listening on {}: {e}", cli.listen))?;
	let stats = Arc::new(proxy::Stats::default());
	let proxy = Arc::new(proxy::Proxy {
		acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
		upstream: cli.upstream.clone(),
		connections: Arc::new(Semaphore::new(cli.max_connections)),
		handshake_timeout: Duration::from_secs(cli.handshake_timeout),
		stats: stats.clone(),
	});
	tracing::info!(
		"Serving https://{} and wss://{} on {}, relayed to {}",
		cli.domain,
		cli.domain,
		cli.listen,
		cli.upstream
	);

	tokio::spawn({
		let proxy = proxy.clone();
		let max = cli.max_connections;
		async move {
			let mut interval = tokio::time::interval(Duration::from_secs(600));
			interval.tick().await;
			loop {
				interval.tick().await;
				tracing::info!(
					"{} connections open; since start: {} relayed, {} refused when full, {} failed handshakes (no ML-KEM, timeouts), {} node unreachable",
					max - proxy.connections.available_permits(),
					stats.accepted.load(Ordering::Relaxed),
					stats.refused.load(Ordering::Relaxed),
					stats.handshake_failures.load(Ordering::Relaxed),
					stats.upstream_failures.load(Ordering::Relaxed),
				);
			}
		}
	});

	tokio::select! {
		result = proxy.run(listener) => result.map_err(|e| e.to_string()),
		_ = stop() => {
			tracing::info!("Stopping");
			Ok(())
		},
	}
}

/// Ctrl-C, or the termination signal of a service manager.
async fn stop() {
	#[cfg(unix)]
	{
		let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
			.expect("signal handlers install; qed");
		tokio::select! {
			_ = tokio::signal::ctrl_c() => {},
			_ = terminate.recv() => {},
		}
	}
	#[cfg(not(unix))]
	let _ = tokio::signal::ctrl_c().await;
}
