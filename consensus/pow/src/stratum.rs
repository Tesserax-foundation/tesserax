//! Stratum server for external miners such as XMRig (algorithm `rx/tess`).
//!
//! The server speaks the stratum protocol of RandomX pools: JSON-RPC messages, one per line, over
//! TCP. A miner logs in, receives a job and submits the nonces whose proof of work meets the job
//! target; the server pushes a new job whenever the block template changes.
//!
//! Every block mined through the server credits the author of the node: the server is meant for
//! the rigs of the node operator. The login of a miner is the name of the rig, or the address of
//! the author; another address is refused, so that nobody mines to this node believing that the
//! rewards go to them.
//!
//! A job is the 43-byte work input with a zero miner nonce at offset 39 and an extra nonce that
//! differs for each connection, so that two rigs never hash the same inputs. The target is the
//! block difficulty: every accepted share is a block.

use codec::Encode;
use sc_consensus_pow::MiningHandle;
use serde_json::{json, Value};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{HeaderBackend, HeaderMetadata};
use sp_consensus_pow::DifficultyApi;
use sp_core::{
	crypto::{AccountId32, Ss58Codec},
	U256,
};
use sp_runtime::{traits::Block as BlockT, SaturatedConversion};
use std::{
	collections::hash_map::RandomState,
	hash::{BuildHasher, Hasher},
	net::SocketAddr,
	sync::{
		atomic::{AtomicU64, AtomicUsize, Ordering},
		Arc,
	},
	time::Duration,
};
use tokio::{
	io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
	net::{tcp::OwnedWriteHalf, TcpListener, TcpStream},
	sync::watch,
};

use crate::{
	meets_difficulty, pow_value, seed_hash, work_input, Difficulty, Seal, TesseraxPow,
	EXTRA_NONCE_SIZE, LOG_TARGET, NONCE_SIZE, WORK_INPUT_SIZE,
};

/// Name of the algorithm in the stratum protocol.
pub const ALGORITHM: &str = "rx/tess";

/// Longest accepted message, in bytes.
const MAX_MESSAGE: u64 = 16 * 1024;

/// A connection without any message for this long is closed. Miners send a keepalive every
/// minute.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Interval between two checks for a new block template.
const TEMPLATE_POLL: Duration = Duration::from_millis(200);

/// Stratum server settings.
#[derive(Clone, Debug)]
pub struct StratumConfig {
	/// Address the server listens on.
	pub listen: SocketAddr,
	/// Account credited with the blocks mined through the server.
	pub author: AccountId32,
	/// Maximum number of simultaneous connections.
	pub max_connections: usize,
}

/// A block template, as sent to miners.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Template {
	pre_hash: [u8; 32],
	difficulty: Difficulty,
	height: u64,
	seed: [u8; 32],
}

/// The 64-bit share target of XMRig for `difficulty`: a miner submits a nonce when the last 8
/// bytes of the proof-of-work value, read as a little-endian integer, are below the target.
///
/// A value meets the difficulty when it is at most `U256::MAX / difficulty`, so every such value
/// passes the target; the node checks the full 256 bits.
pub fn share_target(difficulty: Difficulty) -> u64 {
	if difficulty.is_zero() {
		return u64::MAX;
	}
	let top = (U256::MAX / difficulty) >> 192;
	top.low_u64().saturating_add(1)
}

fn job(template: &Template, extra_nonce: &[u8; EXTRA_NONCE_SIZE], id: &str) -> Value {
	let mut nonce = [0u8; NONCE_SIZE];
	nonce[..EXTRA_NONCE_SIZE].copy_from_slice(extra_nonce);
	json!({
		"blob": hex::encode(work_input(&template.pre_hash, &nonce)),
		"job_id": id,
		"target": hex::encode(share_target(template.difficulty).to_le_bytes()),
		"algo": ALGORITHM,
		"height": template.height,
		"seed_hash": hex::encode(template.seed),
	})
}

fn response(id: &Value, result: Value) -> Value {
	json!({ "id": id, "jsonrpc": "2.0", "error": null, "result": result })
}

fn error(id: &Value, message: &str) -> Value {
	json!({
		"id": id,
		"jsonrpc": "2.0",
		"error": { "code": -1, "message": message },
		"result": null,
	})
}

/// Whether `login` may mine to `author`: a rig name, or the address of the author, optionally
/// followed by `.rig` or `+difficulty` as some miners add.
fn login_allowed(login: &str, author: &AccountId32) -> Result<(), String> {
	let account = login.split(['.', '+']).next().unwrap_or_default();
	match AccountId32::from_ss58check(account) {
		Ok(account) if &account == author => Ok(()),
		Ok(_) => Err(format!(
			"this node credits its blocks to {}; use that address or a rig name as login",
			author.to_ss58check()
		)),
		Err(_) => Ok(()),
	}
}

struct Server<B, C, L>
where
	B: BlockT,
	C: ProvideRuntimeApi<B>
		+ HeaderBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ Send
		+ Sync
		+ 'static,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B> + 'static,
{
	handle: MiningHandle<B, TesseraxPow<B, C>, L>,
	author: AccountId32,
	max_connections: usize,
	connections: AtomicUsize,
	extra_nonces: AtomicU64,
	jobs: AtomicU64,
	templates: watch::Receiver<Option<Arc<Template>>>,
}

/// A logged in miner.
struct Session {
	id: String,
	rig: String,
	extra_nonce: [u8; EXTRA_NONCE_SIZE],
	/// Last job sent, and the one before it to tell stale shares apart.
	job: Option<(String, Arc<Template>)>,
	previous_job: Option<String>,
}

/// Run the stratum server until the process exits.
pub async fn run<B, C, L>(
	handle: MiningHandle<B, TesseraxPow<B, C>, L>,
	client: Arc<C>,
	config: StratumConfig,
) where
	B: BlockT,
	C: ProvideRuntimeApi<B>
		+ HeaderBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ Send
		+ Sync
		+ 'static,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B> + 'static,
{
	let listener = match TcpListener::bind(config.listen).await {
		Ok(listener) => listener,
		Err(e) => {
			log::error!(
				target: LOG_TARGET,
				"Stratum server failed to listen on {}: {e}",
				config.listen,
			);
			return;
		},
	};
	log::info!(
		target: LOG_TARGET,
		"⛏  Stratum server listening on {} ({ALGORITHM}), blocks credited to {}",
		config.listen,
		config.author.to_ss58check(),
	);

	let (sender, templates) = watch::channel(None);
	tokio::spawn(watch_templates(handle.clone(), client, sender));

	let server = Arc::new(Server {
		handle,
		author: config.author,
		max_connections: config.max_connections,
		connections: AtomicUsize::new(0),
		extra_nonces: AtomicU64::new(RandomState::new().build_hasher().finish()),
		jobs: AtomicU64::new(0),
		templates,
	});

	loop {
		let (stream, peer) = match listener.accept().await {
			Ok(accepted) => accepted,
			Err(e) => {
				log::warn!(target: LOG_TARGET, "Stratum server failed to accept a connection: {e}");
				tokio::time::sleep(Duration::from_millis(100)).await;
				continue;
			},
		};
		if server.connections.fetch_add(1, Ordering::SeqCst) >= server.max_connections {
			server.connections.fetch_sub(1, Ordering::SeqCst);
			log::debug!(
				target: LOG_TARGET,
				"Stratum connection from {peer} refused: too many miners",
			);
			continue;
		}
		let server = server.clone();
		tokio::spawn(async move {
			if let Err(e) = serve(&server, stream).await {
				log::debug!(target: LOG_TARGET, "Stratum connection from {peer} closed: {e}");
			}
			server.connections.fetch_sub(1, Ordering::SeqCst);
		});
	}
}

/// Publish the block template of the mining worker whenever it changes.
async fn watch_templates<B, C, L>(
	handle: MiningHandle<B, TesseraxPow<B, C>, L>,
	client: Arc<C>,
	sender: watch::Sender<Option<Arc<Template>>>,
) where
	B: BlockT,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B>,
{
	loop {
		let template = handle.metadata().and_then(|metadata| {
			let seed = seed_hash::<B, _>(client.as_ref(), metadata.best_hash)
				.map_err(|e| {
					log::warn!(target: LOG_TARGET, "Failed to compute the RandomTESS key: {e}")
				})
				.ok()?;
			let parent = client.header_metadata(metadata.best_hash).ok()?;
			Some(Template {
				pre_hash: metadata.pre_hash.as_ref().try_into().ok()?,
				difficulty: metadata.difficulty,
				height: parent.number.saturated_into::<u64>() + 1,
				seed: seed.as_ref().try_into().ok()?,
			})
		});
		sender.send_if_modified(|current| {
			// A template that disappears (a block was just mined, or the node is syncing) keeps
			// miners on the last job until the next one.
			match template {
				Some(template) if current.as_deref() != Some(&template) => {
					*current = Some(Arc::new(template));
					true
				},
				_ => false,
			}
		});
		tokio::time::sleep(TEMPLATE_POLL).await;
	}
}

async fn send(writer: &mut OwnedWriteHalf, message: &Value) -> std::io::Result<()> {
	let mut line = serde_json::to_vec(message).expect("JSON values serialize; qed");
	line.push(b'\n');
	writer.write_all(&line).await
}

async fn serve<B, C, L>(server: &Server<B, C, L>, stream: TcpStream) -> std::io::Result<()>
where
	B: BlockT,
	C: ProvideRuntimeApi<B>
		+ HeaderBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ Send
		+ Sync
		+ 'static,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B> + 'static,
{
	use std::io::{Error, ErrorKind};

	let _ = stream.set_nodelay(true);
	let (reader, mut writer) = stream.into_split();
	let mut reader = BufReader::new(reader);
	let mut templates = server.templates.clone();
	let mut session: Option<Session> = None;
	let mut line = Vec::new();

	loop {
		// A partially read message stays in `line` when a job is sent in the meantime.
		let mut limited = (&mut reader).take(MAX_MESSAGE - line.len() as u64);
		tokio::select! {
			read = tokio::time::timeout(IDLE_TIMEOUT, limited.read_until(b'\n', &mut line)) => {
				match read {
					Err(_) => return Err(Error::new(ErrorKind::TimedOut, "idle")),
					Ok(Err(e)) => return Err(e),
					Ok(Ok(0)) if line.len() as u64 >= MAX_MESSAGE => {
						return Err(Error::new(ErrorKind::InvalidData, "message too long"))
					},
					Ok(Ok(0)) => return Ok(()),
					Ok(Ok(_)) if line.last() != Some(&b'\n') => continue,
					Ok(Ok(_)) => {},
				}
				let request = serde_json::from_slice::<Value>(&line);
				line.clear();
				let Ok(request) = request else {
					return Err(Error::new(ErrorKind::InvalidData, "invalid JSON"));
				};
				let reply = handle_request(server, &mut templates, &mut session, &request).await;
				send(&mut writer, &reply).await?;
			},
			changed = templates.changed(), if session.is_some() => {
				if changed.is_err() {
					return Ok(());
				}
				let template = templates.borrow_and_update().clone();
				if let (Some(session), Some(template)) = (session.as_mut(), template) {
					let params = new_job(server, session, template);
					let job = json!({ "jsonrpc": "2.0", "method": "job", "params": params });
					send(&mut writer, &job).await?;
				}
			},
		}
	}
}

fn new_job<B, C, L>(
	server: &Server<B, C, L>,
	session: &mut Session,
	template: Arc<Template>,
) -> Value
where
	B: BlockT,
	C: ProvideRuntimeApi<B>
		+ HeaderBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ Send
		+ Sync
		+ 'static,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B> + 'static,
{
	let id = format!("{:x}", server.jobs.fetch_add(1, Ordering::Relaxed));
	let params = job(&template, &session.extra_nonce, &id);
	session.previous_job = session.job.take().map(|(id, _)| id);
	session.job = Some((id, template));
	params
}

async fn handle_request<B, C, L>(
	server: &Server<B, C, L>,
	templates: &mut watch::Receiver<Option<Arc<Template>>>,
	session: &mut Option<Session>,
	request: &Value,
) -> Value
where
	B: BlockT,
	C: ProvideRuntimeApi<B>
		+ HeaderBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ Send
		+ Sync
		+ 'static,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B> + 'static,
{
	let id = request.get("id").cloned().unwrap_or(Value::Null);
	let params = request.get("params").cloned().unwrap_or(Value::Null);
	match request.get("method").and_then(Value::as_str) {
		Some("login") => {
			if let Some(algorithms) = params.get("algo").and_then(Value::as_array) {
				if !algorithms.iter().any(|a| a.as_str() == Some(ALGORITHM)) {
					return error(&id, "this miner does not support rx/tess");
				}
			}
			let login = params.get("login").and_then(Value::as_str).unwrap_or_default();
			if let Err(e) = login_allowed(login, &server.author) {
				return error(&id, &e);
			}
			let Some(template) = templates.borrow_and_update().clone() else {
				return error(&id, "the node has no block template yet, retry later");
			};

			let counter = server.extra_nonces.fetch_add(1, Ordering::Relaxed);
			let mut extra_nonce = [0u8; EXTRA_NONCE_SIZE];
			extra_nonce.copy_from_slice(&counter.to_le_bytes()[..EXTRA_NONCE_SIZE]);
			let rig = match params.get("rigid").and_then(Value::as_str) {
				Some(rig) if !rig.is_empty() => rig,
				_ => login,
			};
			let mut new = Session {
				id: hex::encode(extra_nonce),
				rig: rig.chars().take(64).collect(),
				extra_nonce,
				job: None,
				previous_job: None,
			};
			let job = new_job(server, &mut new, template);
			log::info!(target: LOG_TARGET, "⛏  Stratum miner {} connected", new.rig);
			let result = json!({
				"id": new.id,
				"job": job,
				"extensions": ["algo", "keepalive"],
				"status": "OK",
			});
			*session = Some(new);
			response(&id, result)
		},
		Some("submit") => {
			let Some(session) = session.as_mut() else {
				return error(&id, "Unauthenticated");
			};
			if params.get("id").and_then(Value::as_str) != Some(session.id.as_str()) {
				return error(&id, "Unauthenticated");
			}
			let job_id = params.get("job_id").and_then(Value::as_str).unwrap_or_default();
			let template = match &session.job {
				Some((current, template)) if current == job_id => template.clone(),
				_ if session.previous_job.as_deref() == Some(job_id) => {
					return error(&id, "Stale share");
				},
				_ => return error(&id, "Unknown job"),
			};
			let decode = |field: &str| -> Option<Vec<u8>> {
				hex::decode(params.get(field)?.as_str()?).ok()
			};
			let (Some(miner_nonce), Some(hash)) = (decode("nonce"), decode("commitment")) else {
				return error(&id, "Malformed share");
			};
			let (Ok(miner_nonce), Ok(hash)) =
				(<[u8; 4]>::try_from(miner_nonce), <[u8; 32]>::try_from(hash))
			else {
				return error(&id, "Malformed share");
			};

			let mut nonce = [0u8; NONCE_SIZE];
			nonce[..EXTRA_NONCE_SIZE].copy_from_slice(&session.extra_nonce);
			nonce[EXTRA_NONCE_SIZE..].copy_from_slice(&miner_nonce);
			let input: [u8; WORK_INPUT_SIZE] = work_input(&template.pre_hash, &nonce);
			if !meets_difficulty(&pow_value(&input, &hash), template.difficulty) {
				return error(&id, "Low difficulty share");
			}
			let metadata = server.handle.metadata();
			if metadata.map(|m| m.pre_hash.as_ref() == template.pre_hash) != Some(true) {
				return error(&id, "Stale share");
			}

			// Verifying the hash runs the virtual machine and importing the block executes it:
			// both block, so they run outside of the network tasks.
			let handle = server.handle.clone();
			let seal = Seal { nonce, hash }.encode();
			let submit = move || futures::executor::block_on(handle.submit(seal));
			let imported = tokio::task::spawn_blocking(submit).await.unwrap_or(false);
			if imported {
				log::info!(
					target: LOG_TARGET,
					"⛏  Block #{} mined by stratum miner {}",
					template.height,
					session.rig,
				);
				response(&id, json!({ "status": "OK" }))
			} else {
				error(&id, "Block rejected")
			}
		},
		Some("keepalived") => response(&id, json!({ "status": "KEEPALIVED" })),
		Some(method) => error(&id, &format!("Unsupported method {method}")),
		None => error(&id, "Missing method"),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn share_target_passes_every_block() {
		for difficulty in [1u64, 2, 3, 1000, 123_456_789, u64::MAX] {
			let difficulty = U256::from(difficulty);
			let target = share_target(difficulty);
			// The largest value meeting the difficulty passes the target, the smallest one
			// above it is the first that may not.
			let max = U256::MAX / difficulty;
			let top = |v: U256| (v >> 192).low_u64();
			assert!(top(max) < target || target == u64::MAX, "{difficulty}");
			assert!(top(max).saturating_add(1) >= target, "{difficulty}");
		}
		assert_eq!(share_target(U256::one()), u64::MAX);
		assert_eq!(share_target(U256::from(2)), 1 << 63);
		// Above 2^64, only values whose 64 high bits are zero pass.
		assert_eq!(share_target(U256::from(u64::MAX) * 4), 1);
	}

	#[test]
	fn jobs_have_the_xmrig_layout() {
		let template = Template {
			pre_hash: [0xaa; 32],
			difficulty: U256::from(2),
			height: 7,
			seed: [0xbb; 32],
		};
		let job = job(&template, &[1, 2, 3, 4, 5, 6, 7], "2a");
		let blob = hex::decode(job["blob"].as_str().unwrap()).unwrap();
		assert_eq!(blob.len(), WORK_INPUT_SIZE);
		assert_eq!(&blob[32..39], &[1, 2, 3, 4, 5, 6, 7]);
		// XMRig treats a non-zero nonce as a pool-fixed prefix.
		assert_eq!(&blob[39..], &[0, 0, 0, 0]);
		assert_eq!(job["target"], "0000000000000080");
		assert_eq!(job["algo"], "rx/tess");
		assert_eq!(job["height"], 7);
		assert_eq!(job["seed_hash"], hex::encode([0xbb; 32]));
	}

	#[test]
	fn logins() {
		let author = AccountId32::new([1; 32]);
		let other = AccountId32::new([2; 32]);
		assert!(login_allowed("rig1", &author).is_ok());
		assert!(login_allowed("x", &author).is_ok());
		assert!(login_allowed(&author.to_ss58check(), &author).is_ok());
		assert!(login_allowed(&format!("{}.rig1", author.to_ss58check()), &author).is_ok());
		assert!(login_allowed(&other.to_ss58check(), &author).is_err());
		assert!(login_allowed(&format!("{}+5000", other.to_ss58check()), &author).is_err());
	}
}
