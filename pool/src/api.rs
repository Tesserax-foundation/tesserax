//! Read-only HTTP API of the pool, and its dashboard.
//!
//! - `GET /api/stats`: the pool, the network and the current round
//! - `GET /api/blocks`: blocks found, with their status and reward
//! - `GET /api/payments`: payouts
//! - `GET /api/miners/{address}`: hashrate, workers, balance and payouts of a miner

use crate::{chain::now_ms, server::Pool};
use axum::{
	extract::{Path, Query, State},
	http::{header, HeaderValue, StatusCode},
	response::{IntoResponse, Response},
	routing::get,
	Json, Router,
};
use primitive_types::U256;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

/// Seconds between blocks, the target of the difficulty adjustment.
const BLOCK_SECONDS: f64 = 18.0;

/// What the dashboard says about the pool.
#[derive(Clone, Serialize)]
pub struct Info {
	pub name: String,
	pub address: String,
	pub stratum: String,
	pub explorer: Option<String>,
	/// Name of the network, such as Testnet.
	pub network: Option<String>,
	pub fee_percent: f64,
	pub min_payout: String,
	pub payout_interval_minutes: u64,
	pub maturity: u64,
	pub pplns_factor: f64,
	/// Reward of a block, for the estimates of blocks not final yet, until one is credited.
	pub block_reward: String,
}

#[derive(Clone)]
struct Api {
	pool: Arc<Pool>,
	info: Arc<Info>,
}

#[derive(Deserialize)]
struct Limit {
	limit: Option<u32>,
}

fn failure(error: impl std::fmt::Display) -> Response {
	tracing::warn!("API: {error}");
	(StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal error" }))).into_response()
}

fn difficulty_number(difficulty: U256) -> f64 {
	if difficulty > U256::from(u128::MAX) {
		u128::MAX as f64
	} else {
		difficulty.as_u128() as f64
	}
}

async fn stats(State(api): State<Api>) -> Response {
	let template = api.pool.templates.borrow().clone();
	let workers = api.pool.stats.workers();
	let miners = workers.iter().map(|w| &w.address).collect::<std::collections::BTreeSet<_>>().len();
	let (blocks, last_block) = match api.pool.ledger.block_counts() {
		Ok(counts) => counts,
		Err(e) => return failure(e),
	};
	let round = match api.pool.ledger.round_difficulty() {
		Ok(round) => round,
		Err(e) => return failure(e),
	};
	let network = template.map(|t| {
		let difficulty = difficulty_number(t.difficulty);
		json!({ "height": t.height, "difficulty": t.difficulty.to_string(), "hashrate": difficulty / BLOCK_SECONDS })
	});
	let effort = api
		.pool
		.templates
		.borrow()
		.as_ref()
		.map(|t| round as f64 / difficulty_number(t.difficulty).max(1.0) * 100.0);
	Json(json!({
		"pool": &*api.info,
		"hashrate": api.pool.stats.hashrate(|_, _| true, 0),
		"miners": miners,
		"workers": workers.len(),
		"blocks": blocks,
		"last_block_at": last_block,
		"round_effort": effort,
		"network": network,
		"time": now_ms(),
	}))
	.into_response()
}

async fn blocks(State(api): State<Api>, Query(limit): Query<Limit>) -> Response {
	match api.pool.ledger.blocks(limit.limit.unwrap_or(50).min(500)) {
		Ok(blocks) => Json(blocks).into_response(),
		Err(e) => failure(e),
	}
}

async fn payments(State(api): State<Api>, Query(limit): Query<Limit>) -> Response {
	match api.pool.ledger.payments(None, limit.limit.unwrap_or(50).min(500)) {
		Ok(payments) => Json(payments).into_response(),
		Err(e) => failure(e),
	}
}

async fn miner(State(api): State<Api>, Path(address): Path<String>) -> Response {
	if !address.starts_with("te") || tesserax_sdk::parse_address(&address).is_err() {
		return (StatusCode::BAD_REQUEST, Json(json!({ "error": "not a Tesserax address" }))).into_response();
	}
	let fee_bps = (api.info.fee_percent * 100.0).round() as u32;
	let reward = match api.pool.ledger.last_reward() {
		Ok(Some(reward)) => reward,
		Ok(None) => api.info.block_reward.parse().unwrap_or(0),
		Err(e) => return failure(e),
	};
	let account = match api.pool.ledger.account(&address, fee_bps, reward) {
		Ok(account) => account,
		Err(e) => return failure(e),
	};
	let payments = match api.pool.ledger.payments(Some(&address), 50) {
		Ok(payments) => payments,
		Err(e) => return failure(e),
	};
	let shares = api.pool.stats.shares_of(&address);
	let online: Vec<_> = api.pool.stats.workers().into_iter().filter(|w| w.address == address).collect();
	// Rigs that are all connected since a while count over the full window.
	let since = if shares.len() > online.len() { 0 } else { online.iter().map(|w| w.connected_at).min().unwrap_or(0) };
	let mut workers: BTreeMap<String, Value> = BTreeMap::new();
	for worker in online {
		let name = worker.worker.clone();
		let (last_share, count) = shares.get(&name).copied().unwrap_or_default();
		workers.insert(
			name.clone(),
			json!({
				"name": name,
				"online": true,
				"difficulty": worker.difficulty,
				"hashrate": api.pool.stats.hashrate(|a, w| a == address && w == name, worker.connected_at),
				"shares": count,
				"last_share_at": (last_share > 0).then_some(last_share),
			}),
		);
	}
	// Workers that left recently still show, with their last shares.
	for (name, (last_share, count)) in shares {
		workers.entry(name.clone()).or_insert_with(|| {
			json!({
				"name": name,
				"online": false,
				"hashrate": api.pool.stats.hashrate(|a, w| a == address && w == name, 0),
				"shares": count,
				"last_share_at": last_share,
			})
		});
	}
	Json(json!({
		"address": address,
		"hashrate": api.pool.stats.hashrate(|a, _| a == address, since),
		"workers": workers.into_values().collect::<Vec<_>>(),
		"balance": account.balance,
		"immature": account.immature,
		"paid": account.paid,
		"payments": payments,
	}))
	.into_response()
}

fn asset(content_type: &'static str, body: &'static [u8]) -> Response {
	let mut response = body.into_response();
	response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
	response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"));
	response
}

/// Serve the API and the dashboard until the process exits.
pub async fn run(listen: SocketAddr, pool: Arc<Pool>, info: Info) -> std::io::Result<()> {
	let api = Api { pool, info: Arc::new(info) };
	let app = Router::new()
		.route("/api/stats", get(stats))
		.route("/api/blocks", get(blocks))
		.route("/api/payments", get(payments))
		.route("/api/miners/{address}", get(miner))
		.route("/", get(|| async { asset("text/html; charset=utf-8", include_bytes!("../web/index.html")) }))
		.route("/app.js", get(|| async { asset("text/javascript; charset=utf-8", include_bytes!("../web/app.js")) }))
		.route("/style.css", get(|| async { asset("text/css; charset=utf-8", include_bytes!("../web/style.css")) }))
		.route("/logo.png", get(|| async { asset("image/png", include_bytes!("../web/logo.png")) }))
		.layer(axum::middleware::from_fn(security_headers))
		.with_state(api);
	let listener = tokio::net::TcpListener::bind(listen).await?;
	tracing::info!("Dashboard and API on http://{listen}");
	axum::serve(listener, app).await
}

async fn security_headers(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
	let api = request.uri().path().starts_with("/api/");
	let mut response = next.run(request).await;
	let headers = response.headers_mut();
	headers.insert(
		header::CONTENT_SECURITY_POLICY,
		HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"),
	);
	headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
	headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
	headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
	if api {
		// The statistics are public: other sites and miners' tools may read them.
		headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
		headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
	}
	response
}
