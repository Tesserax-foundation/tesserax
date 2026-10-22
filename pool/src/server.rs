//! The stratum server of the pool, for XMRig rigs (`rx/tess`).
//!
//! A rig logs in with the address its rewards go to, optionally followed by `.worker` (or the
//! `rigid` of XMRig) and `+difficulty` for a fixed share difficulty. Each rig gets four bytes of
//! extra nonce of its own and a share difficulty that adapts to its hashrate.
//!
//! Every share is checked in full: its proof-of-work value against the share difficulty, then
//! its RandomTESS hash, recomputed by the pool. A rig can make up hashes whose value passes the
//! share difficulty at no cost, so a wrong hash bans its address for a while.

use crate::{
	chain::now_ms,
	ledger::{Found, Ledger, Share},
	upstream::{Block, Template, Upstream},
	verify::Verifier,
	work::{self, meets, Vardiff, RESERVED_SIZE},
};
use parking_lot::Mutex;
use primitive_types::U256;
use serde_json::{json, Value};
use std::{
	collections::{HashMap, HashSet, VecDeque},
	net::{IpAddr, SocketAddr},
	sync::{
		atomic::{AtomicU32, AtomicU64, Ordering},
		Arc,
	},
	time::{Duration, Instant},
};
use tokio::{
	io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
	net::{tcp::OwnedWriteHalf, TcpListener, TcpStream},
	sync::watch,
};

const ALGORITHM: &str = "rx/tess";
const MAX_MESSAGE: u64 = 16 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);
/// Jobs of a rig kept to accept the shares of: those of the current height only.
const KEPT_JOBS: usize = 4;
/// Shares are counted for hashrates over this window.
pub const HASHRATE_WINDOW: Duration = Duration::from_secs(600);

pub struct ServerConfig {
	pub listen: SocketAddr,
	pub vardiff: Vardiff,
	pub start_difficulty: u64,
	pub max_connections: usize,
	pub max_per_ip: usize,
	pub ban: Duration,
	/// Units of difficulty of the PPLNS window, as a multiple of the block difficulty.
	pub pplns_factor: f64,
}

/// A rig connected, as the statistics show it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Worker {
	pub address: String,
	pub worker: String,
	pub difficulty: u64,
	pub connected_at: i64,
}

/// What the API shows: recent shares and the rigs connected.
pub struct Stats {
	/// Time (ms), address, worker, difficulty.
	recent: Mutex<VecDeque<(i64, String, String, u64)>>,
	workers: Mutex<HashMap<u64, Worker>>,
	/// Start of the pool (ms): hashrates count the shares since, until the window is full.
	pub started: i64,
}

impl Default for Stats {
	fn default() -> Self {
		Stats { recent: Mutex::default(), workers: Mutex::default(), started: now_ms() }
	}
}

impl Stats {
	fn add(&self, share: &Share) {
		let mut recent = self.recent.lock();
		recent.push_back((share.created_at, share.address.clone(), share.worker.clone(), share.difficulty));
		let oldest = share.created_at - HASHRATE_WINDOW.as_millis() as i64;
		while recent.front().is_some_and(|(t, ..)| *t < oldest) {
			recent.pop_front();
		}
	}

	/// Hashrate of the shares matching `filter` over the window, or since `since` (ms) when
	/// later, such as the connection of a rig, in hashes per second.
	pub fn hashrate(&self, filter: impl Fn(&str, &str) -> bool, since: i64) -> f64 {
		let now = now_ms();
		let oldest = now - HASHRATE_WINDOW.as_millis() as i64;
		let seconds = ((now - since.max(self.started).max(oldest)) as f64 / 1000.0).max(1.0);
		let work: u128 = self
			.recent
			.lock()
			.iter()
			.filter(|(t, address, worker, _)| *t >= oldest && filter(address, worker))
			.map(|(.., difficulty)| u128::from(*difficulty))
			.sum();
		work as f64 / seconds
	}

	/// Last share time of each worker of `address`, and their share count, over the window.
	pub fn shares_of(&self, address: &str) -> HashMap<String, (i64, u32)> {
		let mut shares: HashMap<String, (i64, u32)> = HashMap::new();
		for (t, a, worker, _) in self.recent.lock().iter() {
			if a == address {
				let entry = shares.entry(worker.clone()).or_default();
				entry.0 = entry.0.max(*t);
				entry.1 += 1;
			}
		}
		shares
	}

	pub fn workers(&self) -> Vec<Worker> {
		self.workers.lock().values().cloned().collect()
	}
}

pub struct Pool {
	pub config: ServerConfig,
	pub templates: watch::Receiver<Option<Arc<Template>>>,
	pub upstream: Upstream,
	pub verifier: Verifier,
	pub ledger: Arc<Ledger>,
	pub stats: Arc<Stats>,
	/// Shares not written yet: they are written every second, and before a block is recorded.
	pending: Mutex<Vec<Share>>,
	extra_nonces: AtomicU32,
	jobs: AtomicU64,
	sessions: AtomicU64,
	per_ip: Mutex<HashMap<IpAddr, usize>>,
	banned: Mutex<HashMap<IpAddr, Instant>>,
}

impl Pool {
	pub fn new(
		config: ServerConfig,
		templates: watch::Receiver<Option<Arc<Template>>>,
		upstream: Upstream,
		verifier: Verifier,
		ledger: Arc<Ledger>,
	) -> Arc<Self> {
		Arc::new(Pool {
			config,
			templates,
			upstream,
			verifier,
			ledger,
			stats: Arc::default(),
			pending: Mutex::default(),
			extra_nonces: AtomicU32::new(rand_u32()),
			jobs: AtomicU64::new(0),
			sessions: AtomicU64::new(0),
			per_ip: Mutex::default(),
			banned: Mutex::default(),
		})
	}

	/// Write the shares accepted since the last call.
	pub fn flush(&self) {
		let shares = std::mem::take(&mut *self.pending.lock());
		if shares.is_empty() {
			return;
		}
		if let Err(e) = self.ledger.add_shares(&shares) {
			tracing::error!("Writing {} shares: {e}", shares.len());
			// Kept for the next attempt.
			let mut pending = self.pending.lock();
			let newer = std::mem::replace(&mut *pending, shares);
			pending.extend(newer);
		}
	}

	/// The PPLNS window for a block difficulty.
	pub fn window(&self, difficulty: U256) -> u128 {
		let difficulty = if difficulty > U256::from(u128::MAX) { u128::MAX } else { difficulty.as_u128() };
		(difficulty as f64 * self.config.pplns_factor).min(u128::MAX as f64) as u128
	}
}

fn rand_u32() -> u32 {
	use std::hash::{BuildHasher, Hasher};
	std::collections::hash_map::RandomState::new().build_hasher().finish() as u32
}

/// Accept rigs until the process exits.
pub async fn run(pool: Arc<Pool>) -> std::io::Result<()> {
	let listener = TcpListener::bind(pool.config.listen).await?;
	tracing::info!("Stratum server of the pool listening on {}", pool.config.listen);
	let connections = Arc::new(AtomicU64::new(0));
	loop {
		let (stream, peer) = match listener.accept().await {
			Ok(accepted) => accepted,
			Err(e) => {
				tracing::warn!("Accepting a rig: {e}");
				tokio::time::sleep(Duration::from_millis(100)).await;
				continue;
			},
		};
		let ip = peer.ip();
		if pool.banned.lock().get(&ip).is_some_and(|until| *until > Instant::now()) {
			continue;
		}
		if connections.load(Ordering::Relaxed) as usize >= pool.config.max_connections {
			continue;
		}
		{
			let mut per_ip = pool.per_ip.lock();
			let count = per_ip.entry(ip).or_default();
			if *count >= pool.config.max_per_ip {
				continue;
			}
			*count += 1;
		}
		connections.fetch_add(1, Ordering::Relaxed);
		let pool = pool.clone();
		let connections = connections.clone();
		tokio::spawn(async move {
			let session = pool.sessions.fetch_add(1, Ordering::Relaxed);
			if let Err(e) = serve(&pool, stream, ip, session).await {
				tracing::debug!("Rig {peer} disconnected: {e}");
			}
			pool.stats.workers.lock().remove(&session);
			connections.fetch_sub(1, Ordering::Relaxed);
			let mut per_ip = pool.per_ip.lock();
			if let Some(count) = per_ip.get_mut(&ip) {
				*count -= 1;
				if *count == 0 {
					per_ip.remove(&ip);
				}
			}
		});
	}
}

struct Job {
	id: String,
	template: Arc<Template>,
	difficulty: u64,
	nonces: HashSet<[u8; 4]>,
}

struct Session {
	id: String,
	address: String,
	worker: String,
	reserved: [u8; RESERVED_SIZE],
	difficulty: u64,
	fixed: bool,
	jobs: VecDeque<Job>,
	since: Instant,
	shares: u32,
	accepted: u32,
	invalid: u32,
}

/// Split a login into its address, worker and fixed difficulty.
fn parse_login(login: &str, rigid: Option<&str>) -> Result<(String, String, Option<u64>), String> {
	let (rest, fixed) = match login.split_once('+') {
		Some((rest, difficulty)) => (rest, Some(difficulty.parse::<u64>().map_err(|_| "invalid +difficulty".to_string())?)),
		None => (login, None),
	};
	let (address, worker) = match rest.split_once('.') {
		Some((address, worker)) => (address, Some(worker)),
		None => (rest, None),
	};
	if !address.starts_with("te") || tesserax_sdk::parse_address(address).is_err() {
		return Err("log in with your Tesserax address (te…), where the rewards go".into());
	}
	let worker: String = worker
		.or(rigid)
		.unwrap_or("default")
		.chars()
		.filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
		.take(32)
		.collect();
	Ok((address.to_string(), if worker.is_empty() { "default".into() } else { worker }, fixed))
}

fn response(id: &Value, result: Value) -> Value {
	json!({ "id": id, "jsonrpc": "2.0", "error": null, "result": result })
}

fn error(id: &Value, message: &str) -> Value {
	json!({ "id": id, "jsonrpc": "2.0", "error": { "code": -1, "message": message }, "result": null })
}

async fn send(writer: &mut OwnedWriteHalf, message: &Value) -> std::io::Result<()> {
	let mut line = serde_json::to_vec(message).expect("JSON values serialize; qed");
	line.push(b'\n');
	writer.write_all(&line).await
}

fn new_job(pool: &Pool, session: &mut Session, template: Arc<Template>) -> Value {
	let id = format!("{:x}", pool.jobs.fetch_add(1, Ordering::Relaxed));
	let mut blob = template.blob;
	blob[work::RESERVED_OFFSET..work::MINER_NONCE_OFFSET].copy_from_slice(&session.reserved);
	// The share difficulty never exceeds the block difficulty.
	let block = if template.difficulty > U256::from(u64::MAX) { u64::MAX } else { template.difficulty.as_u64() };
	let difficulty = session.difficulty.min(block.max(1));
	let params = json!({
		"blob": hex::encode(blob),
		"job_id": id,
		"target": hex::encode(work::target(difficulty).to_le_bytes()),
		"algo": ALGORITHM,
		"height": template.height,
		"seed_hash": hex::encode(template.seed),
	});
	// Shares of an older height can no longer count.
	session.jobs.retain(|job| job.template.height == template.height);
	if session.jobs.len() >= KEPT_JOBS {
		session.jobs.pop_front();
	}
	session.jobs.push_back(Job { id, template, difficulty, nonces: HashSet::new() });
	params
}

async fn serve(pool: &Arc<Pool>, stream: TcpStream, ip: IpAddr, number: u64) -> std::io::Result<()> {
	use std::io::{Error, ErrorKind};
	let _ = stream.set_nodelay(true);
	let (reader, mut writer) = stream.into_split();
	let mut reader = BufReader::new(reader);
	let mut templates = pool.templates.clone();
	let mut session: Option<Session> = None;
	let mut line = Vec::new();
	loop {
		let mut limited = (&mut reader).take(MAX_MESSAGE - line.len() as u64);
		tokio::select! {
			read = tokio::time::timeout(IDLE_TIMEOUT, limited.read_until(b'\n', &mut line)) => {
				match read {
					Err(_) => return Err(Error::new(ErrorKind::TimedOut, "idle")),
					Ok(Err(e)) => return Err(e),
					Ok(Ok(0)) if line.len() as u64 >= MAX_MESSAGE => return Err(Error::new(ErrorKind::InvalidData, "message too long")),
					Ok(Ok(0)) => return Ok(()),
					Ok(Ok(_)) if line.last() != Some(&b'\n') => continue,
					Ok(Ok(_)) => {},
				}
				let request = serde_json::from_slice::<Value>(&line);
				line.clear();
				let Ok(request) = request else { return Err(Error::new(ErrorKind::InvalidData, "invalid JSON")) };
				let (reply, push) = handle(pool, &mut templates, &mut session, &request, number).await;
				send(&mut writer, &reply).await?;
				if let Some(job) = push {
					send(&mut writer, &json!({ "jsonrpc": "2.0", "method": "job", "params": job })).await?;
				}
				if let Some(session) = &session {
					if session.invalid >= 3 && session.invalid > session.accepted / 10 {
						tracing::warn!("Banning {ip} ({}): {} invalid shares", session.address, session.invalid);
						pool.banned.lock().insert(ip, Instant::now() + pool.config.ban);
						return Err(Error::new(ErrorKind::PermissionDenied, "banned"));
					}
				}
			},
			changed = templates.changed(), if session.is_some() => {
				if changed.is_err() {
					return Ok(());
				}
				let template = templates.borrow_and_update().clone();
				if let (Some(session), Some(template)) = (session.as_mut(), template) {
					let job = new_job(pool, session, template);
					send(&mut writer, &json!({ "jsonrpc": "2.0", "method": "job", "params": job })).await?;
				}
			},
		}
	}
}

/// Handle a request: the reply, and a job to push after it when the difficulty changed.
async fn handle(
	pool: &Arc<Pool>,
	templates: &mut watch::Receiver<Option<Arc<Template>>>,
	session: &mut Option<Session>,
	request: &Value,
	number: u64,
) -> (Value, Option<Value>) {
	let id = request.get("id").cloned().unwrap_or(Value::Null);
	let params = request.get("params").cloned().unwrap_or(Value::Null);
	match request.get("method").and_then(Value::as_str) {
		Some("login") => {
			if session.is_some() {
				return (error(&id, "Already logged in"), None);
			}
			if let Some(algorithms) = params.get("algo").and_then(Value::as_array) {
				if !algorithms.iter().any(|a| a.as_str() == Some(ALGORITHM)) {
					return (error(&id, "this miner does not support rx/tess"), None);
				}
			}
			let login = params.get("login").and_then(Value::as_str).unwrap_or_default();
			let rigid = params.get("rigid").and_then(Value::as_str).filter(|r| !r.is_empty());
			let (address, worker, fixed) = match parse_login(login, rigid) {
				Ok(parsed) => parsed,
				Err(e) => return (error(&id, &e), None),
			};
			let Some(template) = templates.borrow_and_update().clone() else {
				return (error(&id, "the pool is not connected to its node yet, retry later"), None);
			};
			let vardiff = pool.config.vardiff;
			let mut new = Session {
				id: format!("{:016x}{:08x}", rand_u32() as u64 | (u64::from(rand_u32()) << 32), number as u32),
				address: address.clone(),
				worker: worker.clone(),
				reserved: pool.extra_nonces.fetch_add(1, Ordering::Relaxed).to_le_bytes(),
				difficulty: fixed.map(|d| d.max(vardiff.min)).unwrap_or(pool.config.start_difficulty),
				fixed: fixed.is_some(),
				jobs: VecDeque::new(),
				since: Instant::now(),
				shares: 0,
				accepted: 0,
				invalid: 0,
			};
			let job = new_job(pool, &mut new, template);
			pool.stats.workers.lock().insert(
				number,
				Worker { address: address.clone(), worker: worker.clone(), difficulty: new.difficulty, connected_at: now_ms() },
			);
			tracing::info!("Rig {worker} of {address} connected");
			let result = json!({ "id": new.id, "job": job, "extensions": ["algo", "keepalive"], "status": "OK" });
			*session = Some(new);
			(response(&id, result), None)
		},
		Some("submit") => {
			let Some(session) = session.as_mut() else { return (error(&id, "Unauthenticated"), None) };
			if params.get("id").and_then(Value::as_str) != Some(session.id.as_str()) {
				return (error(&id, "Unauthenticated"), None);
			}
			let reply = submit(pool, session, &params).await;
			let reply = match reply {
				Ok(()) => response(&id, json!({ "status": "OK" })),
				Err(message) => error(&id, message),
			};
			// Adjust the difficulty every 30 seconds or 20 shares.
			let elapsed = session.since.elapsed().as_secs_f64();
			let mut push = None;
			if !session.fixed && (elapsed >= 30.0 || session.shares >= 20) {
				if let Some(next) = pool.config.vardiff.retarget(session.difficulty, session.shares, elapsed) {
					session.difficulty = next;
					if let Some(worker) = pool.stats.workers.lock().get_mut(&number) {
						worker.difficulty = next;
					}
					if let Some(template) = pool.templates.borrow().clone() {
						push = Some(new_job(pool, session, template));
					}
				}
				session.since = Instant::now();
				session.shares = 0;
			}
			(reply, push)
		},
		Some("keepalived") => (response(&id, json!({ "status": "KEEPALIVED" })), None),
		Some(method) => (error(&id, &format!("Unsupported method {method}")), None),
		None => (error(&id, "Missing method"), None),
	}
}

async fn submit(pool: &Arc<Pool>, session: &mut Session, params: &Value) -> Result<(), &'static str> {
	let job_id = params.get("job_id").and_then(Value::as_str).unwrap_or_default();
	let decode = |field: &str| -> Option<Vec<u8>> { hex::decode(params.get(field)?.as_str()?).ok() };
	let nonce: [u8; 4] = decode("nonce").and_then(|n| n.try_into().ok()).ok_or("Malformed share")?;
	let hash: [u8; 32] = decode("commitment").and_then(|h| h.try_into().ok()).ok_or("Malformed share")?;

	let current_height = pool.templates.borrow().as_ref().map(|t| t.height);
	let Some(job) = session.jobs.iter_mut().find(|job| job.id == job_id) else {
		return Err("Unknown job");
	};
	if Some(job.template.height) != current_height {
		return Err("Stale share");
	}
	if !job.nonces.insert(nonce) {
		return Err("Duplicate share");
	}
	let input = work::work_input(&job.template.blob, session.reserved, nonce);
	let value = randomtess::commitment(&input, &hash);
	if !meets(&value, U256::from(job.difficulty)) {
		session.invalid += 1;
		return Err("Low difficulty share");
	}
	let template = job.template.clone();
	let difficulty = job.difficulty;
	match pool.verifier.hash(template.seed, input.to_vec()).await {
		Ok(expected) if expected == hash => {},
		Ok(_) => {
			// Honest rigs never send a wrong hash.
			session.invalid += 3;
			return Err("Invalid share");
		},
		Err(e) => {
			tracing::error!("Verifying a share: {e}");
			return Err("The pool cannot verify shares, retry later");
		},
	}

	let share = Share { address: session.address.clone(), worker: session.worker.clone(), difficulty, created_at: now_ms() };
	pool.stats.add(&share);
	pool.pending.lock().push(share);
	session.shares += 1;
	session.accepted += 1;

	if meets(&value, template.difficulty) {
		let block = Block { job_id: template.job_id.clone(), reserved: session.reserved, nonce, hash, value };
		match pool.upstream.submit(block).await {
			Ok(()) => {
				let mut seal_nonce = [0u8; 11];
				seal_nonce.copy_from_slice(&input[32..]);
				let difficulty = if template.difficulty > U256::from(u128::MAX) { u128::MAX } else { template.difficulty.as_u128() };
				let found = Found {
					height: template.height,
					nonce: hex::encode(seal_nonce),
					finder: session.address.clone(),
					worker: session.worker.clone(),
					network_difficulty: difficulty,
					found_at: now_ms(),
				};
				pool.flush();
				match pool.ledger.record_block(&found, pool.window(template.difficulty)) {
					Ok(()) => tracing::info!("⛏  Block #{} found by {} of {}", template.height, session.worker, session.address),
					Err(e) => tracing::error!("Recording block #{}: {e}", template.height),
				}
			},
			Err(e) => tracing::warn!("The node refused the block #{} of {}: {e}", template.height, session.address),
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	const ALICE: &str = "teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5";

	#[test]
	fn logins_carry_the_address_the_worker_and_a_difficulty() {
		assert_eq!(parse_login(ALICE, None).unwrap(), (ALICE.into(), "default".into(), None));
		assert_eq!(parse_login(&format!("{ALICE}.rig-1"), None).unwrap(), (ALICE.into(), "rig-1".into(), None));
		assert_eq!(parse_login(ALICE, Some("garage")).unwrap(), (ALICE.into(), "garage".into(), None));
		assert_eq!(parse_login(&format!("{ALICE}.rig+50000"), None).unwrap(), (ALICE.into(), "rig".into(), Some(50_000)));
		assert_eq!(parse_login(&format!("{ALICE}.r<i>g"), None).unwrap().1, "rig");
		assert!(parse_login("x", None).is_err());
		assert!(parse_login("0x0101010101010101010101010101010101010101010101010101010101010101", None).is_err());
		assert!(parse_login(&format!("{ALICE}+abc"), None).is_err());
		// A Polkadot address is not a Tesserax address.
		assert!(parse_login("15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5", None).is_err());
	}
}
