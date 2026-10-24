//! The connection to the stratum server of the node, in pool mode: the node sends block templates
//! with the first bytes of the extra nonce fixed, and takes the blocks the rigs find.
//!
//! The pool logs in with its own address: the node refuses it unless it credits its blocks to
//! that address (`--author`), so the rewards cannot go elsewhere by mistake.

use crate::work::{RESERVED_OFFSET, RESERVED_SIZE, WORK_INPUT_SIZE};
use primitive_types::U256;
use serde_json::{json, Value};
use std::{
	collections::HashMap,
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	time::Duration,
};
use tokio::{
	io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
	net::TcpStream,
	sync::{mpsc, oneshot, watch},
};

/// A block template of the node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
	/// Job id of the node.
	pub job_id: String,
	/// Work input with the prefix of the node, zero elsewhere.
	pub blob: [u8; WORK_INPUT_SIZE],
	pub height: u64,
	pub seed: [u8; 32],
	pub difficulty: U256,
}

/// A block found by a rig, for the node.
#[derive(Debug)]
pub struct Block {
	pub job_id: String,
	pub reserved: [u8; RESERVED_SIZE],
	pub nonce: [u8; 4],
	pub hash: [u8; 32],
	pub value: [u8; 32],
}

/// Submissions of blocks to the node.
#[derive(Clone)]
pub struct Upstream {
	blocks: mpsc::Sender<(Block, oneshot::Sender<Result<(), String>>)>,
}

impl Upstream {
	/// Submit `block`; Ok once the node imported it.
	pub async fn submit(&self, block: Block) -> Result<(), String> {
		let (reply, response) = oneshot::channel();
		self.blocks.send((block, reply)).await.map_err(|_| "no connection to the node".to_string())?;
		response.await.map_err(|_| "the connection to the node closed".to_string())?
	}
}

/// Connect to the node at `address`, `host:port`, as the pool `login`, and keep the connection.
/// Templates are published in the returned receiver. The host is resolved again at each
/// connection, so that a node whose address changes is found again.
pub fn start(address: String, login: String) -> (Upstream, watch::Receiver<Option<Arc<Template>>>) {
	let (templates, receiver) = watch::channel(None);
	let (blocks, mut queue) = mpsc::channel(64);
	tokio::spawn(async move {
		let mut delay = Duration::from_secs(1);
		loop {
			match session(&address, &login, &templates, &mut queue).await {
				Ok(()) => tracing::warn!("The node closed the stratum connection"),
				Err(Fatal(message)) => {
					tracing::error!("The node refused the pool: {message}");
					std::process::exit(1);
				},
				Err(Retry(message)) => tracing::warn!("Stratum connection to the node: {message}"),
			}
			tokio::time::sleep(delay).await;
			delay = (delay * 2).min(Duration::from_secs(30));
		}
	});
	(Upstream { blocks }, receiver)
}

enum Failure {
	/// The node refuses the pool: retrying cannot help.
	Fatal(String),
	Retry(String),
}
use Failure::{Fatal, Retry};

impl From<std::io::Error> for Failure {
	fn from(error: std::io::Error) -> Self {
		Retry(error.to_string())
	}
}

fn parse_template(job: &Value) -> Result<Template, String> {
	let field = |name: &str| job.get(name).ok_or_else(|| format!("job without {name}"));
	let blob: [u8; WORK_INPUT_SIZE] = hex::decode(field("blob")?.as_str().unwrap_or_default())
		.ok()
		.and_then(|b| b.try_into().ok())
		.ok_or("invalid job blob")?;
	let seed: [u8; 32] = hex::decode(field("seed_hash")?.as_str().unwrap_or_default())
		.ok()
		.and_then(|b| b.try_into().ok())
		.ok_or("invalid job seed")?;
	let difficulty = U256::from_dec_str(field("difficulty")?.as_str().unwrap_or_default())
		.map_err(|_| "invalid job difficulty, update the node")?;
	if field("reserved_offset")?.as_u64() != Some(RESERVED_OFFSET as u64)
		|| field("reserved_size")?.as_u64() != Some(RESERVED_SIZE as u64)
	{
		return Err("the node does not reserve the extra nonce of the pool".into());
	}
	Ok(Template {
		job_id: field("job_id")?.as_str().unwrap_or_default().to_string(),
		blob,
		height: field("height")?.as_u64().ok_or("invalid job height")?,
		seed,
		difficulty,
	})
}

async fn session(
	address: &str,
	login: &str,
	templates: &watch::Sender<Option<Arc<Template>>>,
	queue: &mut mpsc::Receiver<(Block, oneshot::Sender<Result<(), String>>)>,
) -> Result<(), Failure> {
	let stream = TcpStream::connect(address).await?;
	let _ = stream.set_nodelay(true);
	let (reader, mut writer) = stream.into_split();
	let mut lines = BufReader::new(reader).lines();
	let ids = AtomicU64::new(1);
	let send = async |writer: &mut tokio::net::tcp::OwnedWriteHalf, method: &str, params: Value| {
		let id = ids.fetch_add(1, Ordering::Relaxed);
		let mut line = serde_json::to_vec(&json!({ "id": id, "jsonrpc": "2.0", "method": method, "params": params }))
			.expect("JSON values serialize; qed");
		line.push(b'\n');
		writer.write_all(&line).await.map(|()| id)
	};

	let params = json!({ "login": login, "pass": "x", "agent": concat!("tesserax-pool/", env!("CARGO_PKG_VERSION")), "algo": ["rx/tess"], "mode": "pool" });
	send(&mut writer, "login", params).await?;
	let reply: Value = match lines.next_line().await? {
		Some(line) => serde_json::from_str(&line).map_err(|e| Retry(e.to_string()))?,
		None => return Err(Retry("closed during the login".into())),
	};
	if let Some(message) = reply["error"]["message"].as_str() {
		// The node has no template while it starts or syncs.
		return Err(if message.contains("template") { Retry(message.into()) } else { Fatal(message.into()) });
	}
	let session_id = reply["result"]["id"].as_str().unwrap_or_default().to_string();
	let first = parse_template(&reply["result"]["job"]).map_err(Fatal)?;
	tracing::info!("Connected to the node at {address}, height {}", first.height);
	templates.send_replace(Some(Arc::new(first)));

	let mut waiting: HashMap<u64, oneshot::Sender<Result<(), String>>> = HashMap::new();
	let mut keepalive = tokio::time::interval(Duration::from_secs(60));
	keepalive.tick().await;
	loop {
		tokio::select! {
			line = lines.next_line() => {
				let Some(line) = line? else { return Ok(()) };
				let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
				if message["method"] == "job" {
					match parse_template(&message["params"]) {
						Ok(template) => {
							templates.send_replace(Some(Arc::new(template)));
						},
						Err(e) => tracing::warn!("Ignoring a job of the node: {e}"),
					}
				} else if let Some(reply) = message["id"].as_u64().and_then(|id| waiting.remove(&id)) {
					let outcome = match message["error"]["message"].as_str() {
						Some(error) => Err(error.to_string()),
						None => Ok(()),
					};
					let _ = reply.send(outcome);
				}
			},
			Some((block, reply)) = queue.recv() => {
				let params = json!({
					"id": session_id,
					"job_id": block.job_id,
					"nonce": hex::encode(block.nonce),
					"extra_nonce": hex::encode(block.reserved),
					"result": hex::encode(block.value),
					"commitment": hex::encode(block.hash),
				});
				let id = send(&mut writer, "submit", params).await?;
				waiting.insert(id, reply);
			},
			_ = keepalive.tick() => {
				send(&mut writer, "keepalived", json!({ "id": session_id })).await?;
			},
		}
	}
}
