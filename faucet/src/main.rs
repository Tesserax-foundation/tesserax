//! Faucet of the Tesserax testnet.
//!
//! Gives test TESS to any address that asks, once per address every 24 hours, a few times per IP
//! address, and up to a cap per 24 hours. Its key is generated on its first start and kept in its
//! state directory; send it test TESS, or make it the author of a miner, to fund it.
//!
//! - `GET /`: the page of the faucet
//! - `GET /api/info`: its address, balance and limits
//! - `POST /api/claim` with `{"address": "te..."}`: sends the test TESS, once a block includes them

mod limits;

use axum::{
	extract::{ConnectInfo, State},
	http::{header, HeaderMap, HeaderValue, StatusCode},
	response::{IntoResponse, Response},
	routing::{get, post},
	Json, Router,
};
use clap::Parser;
use limits::{Limits, Refusal};
use serde::Deserialize;
use serde_json::json;
use std::{
	net::SocketAddr,
	path::PathBuf,
	sync::Arc,
	time::{SystemTime, UNIX_EPOCH},
};
use tesserax_sdk::{parse_address, Amount, Client, Keypair};
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[command(name = "tesserax-faucet", version, about = "Faucet of the Tesserax testnet")]
struct Cli {
	/// RPC of a node of the testnet.
	#[arg(long, env = "NODE_URL", default_value = "ws://127.0.0.1:9944")]
	node: String,
	/// Address of the HTTP server.
	#[arg(long, env = "LISTEN", default_value = "[::]:8080")]
	listen: SocketAddr,
	/// Directory of the key of the faucet and of its claims.
	#[arg(long, env = "STATE", default_value = "/data")]
	state: PathBuf,
	/// TESS given per claim.
	#[arg(long, env = "AMOUNT", default_value = "10")]
	amount: String,
	/// Claims per IP address every 24 hours.
	#[arg(long, env = "IP_CLAIMS", default_value_t = 3)]
	ip_claims: usize,
	/// TESS given at most every 24 hours.
	#[arg(long, env = "DAILY_CAP", default_value = "2000")]
	daily_cap: String,
	/// Explorer, for the links to transactions and accounts.
	#[arg(long, env = "EXPLORER")]
	explorer: Option<String>,
}

struct Faucet {
	node: String,
	key: Keypair,
	amount: Amount,
	limits: Mutex<Limits>,
	/// One transfer at a time: each takes the next nonce of the faucet.
	client: Mutex<Option<Client>>,
	explorer: Option<String>,
}

fn now() -> u64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
	(status, Json(json!({ "error": message.into() }))).into_response()
}

/// The key of the faucet, generated on the first start.
fn load_or_create_key(state: &std::path::Path) -> Result<Keypair, String> {
	let path = state.join("faucet.phrase");
	match std::fs::read_to_string(&path) {
		Ok(phrase) => Keypair::from_uri(phrase.trim()).map_err(|e| format!("{}: {e}", path.display())),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
			let (key, phrase) = Keypair::generate().map_err(|e| e.to_string())?;
			let partial = path.with_extension("partial");
			std::fs::write(&partial, format!("{phrase}\n")).map_err(|e| format!("{}: {e}", partial.display()))?;
			#[cfg(unix)]
			{
				use std::os::unix::fs::PermissionsExt;
				std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o600))
					.map_err(|e| format!("{}: {e}", partial.display()))?;
			}
			std::fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
			tracing::info!("Key of the faucet generated in {}", path.display());
			Ok(key)
		},
		Err(e) => Err(format!("{}: {e}", path.display())),
	}
}

impl Faucet {
	/// The client of the node, connected again when the connection was lost.
	async fn client(&self, slot: &mut Option<Client>) -> Result<Client, String> {
		if let Some(client) = slot {
			if client.best_block_number().await.is_ok() {
				return Ok(client.clone());
			}
		}
		let client = Client::connect(&self.node).await.map_err(|e| format!("node unreachable: {e}"))?;
		*slot = Some(client.clone());
		Ok(client)
	}
}

/// The address of the client: the first of `X-Forwarded-For`, set by the proxy in front of the
/// faucet, or the address of the connection.
fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> String {
	headers
		.get("x-forwarded-for")
		.and_then(|v| v.to_str().ok())
		.and_then(|v| v.split(',').next())
		.map(|v| v.trim().to_string())
		.filter(|v| !v.is_empty())
		.unwrap_or_else(|| peer.ip().to_string())
}

async fn info(State(faucet): State<Arc<Faucet>>) -> Response {
	let balance = {
		let mut slot = faucet.client.lock().await;
		match faucet.client(&mut slot).await {
			Ok(client) => match client.balance(&faucet.key.account_id()).await {
				Ok(balance) => Some(balance.transferable.to_string()),
				Err(_) => {
					*slot = None;
					None
				},
			},
			Err(_) => None,
		}
	};
	let mut limits = faucet.limits.lock().await;
	Json(json!({
		"address": faucet.key.address(),
		"balance": balance,
		"amount": faucet.amount.to_string(),
		"ip_claims_per_day": limits.ip_claims,
		"daily_cap": Amount(limits.cap).to_string(),
		"given_today": Amount(limits.given(now())).to_string(),
		"explorer": faucet.explorer,
	}))
	.into_response()
}

#[derive(Deserialize)]
struct ClaimRequest {
	address: String,
}

async fn claim(
	State(faucet): State<Arc<Faucet>>,
	ConnectInfo(peer): ConnectInfo<SocketAddr>,
	headers: HeaderMap,
	Json(request): Json<ClaimRequest>,
) -> Response {
	let Ok(to) = parse_address(request.address.trim()) else {
		return error(StatusCode::BAD_REQUEST, "not a Tesserax address");
	};
	let address = tesserax_sdk::format_address(&to);
	let ip = client_ip(&headers, peer);
	let at = now();
	let hours = |seconds: u64| seconds.div_ceil(3600);
	{
		let mut limits = faucet.limits.lock().await;
		if let Err(refusal) = limits.claim(&address, &ip, faucet.amount.0, at) {
			let message = match refusal {
				Refusal::Address(wait) => {
					format!("this address already received test TESS: try again in {} h", hours(wait))
				},
				Refusal::Ip(wait) => format!("too many claims from your network: try again in {} h", hours(wait)),
				Refusal::Cap(wait) => format!("the faucet gave all it can today: try again in {} h", hours(wait)),
			};
			return error(StatusCode::TOO_MANY_REQUESTS, message);
		}
	}

	let result = {
		let mut slot = faucet.client.lock().await;
		match faucet.client(&mut slot).await {
			Ok(client) => {
				// Keep a little for the fee.
				let reserve = faucet.amount.0.saturating_add(Amount::from_tess("1").expect("a valid amount; qed").0);
				match client.balance(&faucet.key.account_id()).await {
					Ok(balance) if balance.transferable.0 < reserve => {
						drop(slot);
						faucet.limits.lock().await.cancel(&address, at);
						return error(StatusCode::SERVICE_UNAVAILABLE, "the faucet is empty for now: try again later");
					},
					_ => {},
				}
				let sent = client.transfer(&faucet.key, &to, faucet.amount).await;
				if matches!(sent, Err(tesserax_sdk::Error::Node(_))) {
					*slot = None;
				}
				sent.map_err(|e| e.to_string())
			},
			Err(e) => Err(e),
		}
	};

	let mut limits = faucet.limits.lock().await;
	match result {
		Ok(outcome) => {
			if let Err(e) = limits.save() {
				tracing::error!("Saving the claims: {e}");
			}
			tracing::info!("Sent {} TESS to {address}", faucet.amount);
			Json(json!({
				"amount": faucet.amount.to_string(),
				"address": address,
				"block_hash": outcome.block_hash,
				"block_number": outcome.block_number,
				"extrinsic_hash": outcome.extrinsic_hash,
			}))
			.into_response()
		},
		Err(e) => {
			limits.cancel(&address, at);
			tracing::warn!("Sending to {address}: {e}");
			error(StatusCode::SERVICE_UNAVAILABLE, "the transfer failed: try again in a minute")
		},
	}
}

fn asset(content_type: &'static str, body: &'static [u8]) -> Response {
	let mut response = body.into_response();
	let headers = response.headers_mut();
	headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
	headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"));
	response
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
	std::fs::create_dir_all(&cli.state).map_err(|e| format!("{}: {e}", cli.state.display()))?;
	let key = load_or_create_key(&cli.state)?;
	let amount = Amount::from_tess(&cli.amount).map_err(|e| e.to_string())?;
	let cap = Amount::from_tess(&cli.daily_cap).map_err(|e| e.to_string())?;
	let limits = Limits::open(&cli.state.join("claims.json"), cli.ip_claims.max(1), cap.0)
		.map_err(|e| format!("claims.json: {e}"))?;
	tracing::info!("Faucet {} gives {} TESS per claim", key.address(), amount);

	let faucet = Arc::new(Faucet {
		node: cli.node,
		key,
		amount,
		limits: Mutex::new(limits),
		client: Mutex::new(None),
		explorer: cli.explorer.map(|e| e.trim_end_matches('/').to_string()),
	});

	let app = Router::new()
		.route("/api/info", get(info))
		.route("/api/claim", post(claim))
		.route("/", get(|| async { asset("text/html; charset=utf-8", include_bytes!("../web/index.html")) }))
		.route("/app.js", get(|| async { asset("text/javascript; charset=utf-8", include_bytes!("../web/app.js")) }))
		.route("/style.css", get(|| async { asset("text/css; charset=utf-8", include_bytes!("../web/style.css")) }))
		.route("/logo.png", get(|| async { asset("image/png", include_bytes!("../web/logo.png")) }))
		.with_state(faucet);

	let listener = tokio::net::TcpListener::bind(cli.listen).await.map_err(|e| format!("{}: {e}", cli.listen))?;
	tracing::info!("Serving on {}", cli.listen);
	axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
		// Transfers in progress finish before the process exits.
		.with_graceful_shutdown(stop())
		.await
		.map_err(|e| e.to_string())
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
