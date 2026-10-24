//! What the pool reads from and writes to the chain: whether its blocks stay in the chain, what
//! they paid, and the payouts to the miners.

use crate::ledger::Ledger;
use std::{sync::Arc, time::Duration};
use subxt::{
	backend::legacy::LegacyRpcMethods,
	config::substrate::DigestItem,
	OnlineClient,
};
use tesserax_sdk::{
	runtime::{emission, shielded, transaction_payment},
	AccountId, Amount, Client, Keypair, TesseraxConfig,
};

/// Consensus engine id of the proof-of-work seal.
const POW_ENGINE_ID: [u8; 4] = *b"pow_";

/// A block of the best chain: its hash and the nonce of its seal.
pub struct ChainBlock {
	pub hash: String,
	pub nonce: String,
}

/// One connection to the RPC of the node.
struct Connection {
	api: OnlineClient<TesseraxConfig>,
	rpc: LegacyRpcMethods<TesseraxConfig>,
	client: Client,
}

impl Connection {
	/// `ws://` on the machine of the node or a private network, or `wss://` with post-quantum TLS.
	async fn open(url: &str) -> Result<Self> {
		let client = Client::connect(url).await.map_err(|e| e.to_string())?;
		Ok(Connection { api: client.api().clone(), rpc: client.rpc().clone(), client })
	}
}

/// The RPC of the node. A connection the node closed, when it restarts for instance, is opened
/// again at the next call.
#[derive(Clone)]
pub struct Chain {
	url: Arc<str>,
	connection: Arc<tokio::sync::Mutex<Option<Arc<Connection>>>>,
}

pub type Result<T> = std::result::Result<T, String>;

fn hex(bytes: &[u8]) -> String {
	format!("0x{}", ::hex::encode(bytes))
}

/// Whether `error` means that the connection is closed and must be opened again.
fn closes(error: &str) -> bool {
	error.contains("restart required") || error.contains("connection closed") || error.contains("Connection closed")
}

impl Chain {
	pub async fn connect(url: &str) -> Result<Self> {
		let connection = Connection::open(url).await?;
		Ok(Chain { url: url.into(), connection: Arc::new(tokio::sync::Mutex::new(Some(Arc::new(connection)))) })
	}

	/// The connection, opened again if the last one was closed.
	async fn connection(&self) -> Result<Arc<Connection>> {
		let mut current = self.connection.lock().await;
		if let Some(connection) = current.as_ref() {
			return Ok(connection.clone());
		}
		let connection = Arc::new(Connection::open(&self.url).await?);
		tracing::info!("Connected again to the RPC of the node");
		*current = Some(connection.clone());
		Ok(connection)
	}

	/// Drop the connection if `error` closed it.
	async fn failed(&self, error: &str) {
		if closes(error) {
			tracing::warn!("The RPC connection to the node closed: opening another");
			*self.connection.lock().await = None;
		}
	}

	/// `result`, after dropping the connection if its error closed it.
	async fn checked<T>(&self, result: Result<T>) -> Result<T> {
		if let Err(error) = &result {
			self.failed(error).await;
		}
		result
	}

	pub async fn best_number(&self) -> Result<u64> {
		let connection = self.connection().await?;
		let result = connection.client.best_block_number().await.map(u64::from).map_err(|e| e.to_string());
		self.checked(result).await
	}

	/// The block of the best chain at `height`, if any.
	pub async fn block_at(&self, height: u64) -> Result<Option<ChainBlock>> {
		let connection = self.connection().await?;
		let result = async {
			let Some(hash) = connection
				.rpc
				.chain_get_block_hash(Some(height.into()))
				.await
				.map_err(|e| e.to_string())?
			else {
				return Ok(None);
			};
			let Some(header) = connection.rpc.chain_get_header(Some(hash)).await.map_err(|e| e.to_string())? else {
				return Ok(None);
			};
			let nonce = header.digest.logs.iter().find_map(|item| match item {
				DigestItem::Seal(engine, seal) if *engine == POW_ENGINE_ID && seal.len() >= 11 => Some(::hex::encode(&seal[..11])),
				_ => None,
			});
			Ok(nonce.map(|nonce| ChainBlock { hash: hex(hash.as_ref()), nonce }))
		}
		.await;
		self.checked(result).await
	}

	/// What the block `hash` paid to `author`: the mining reward and the fees and tips of its
	/// transactions, all of which go to the miner.
	pub async fn reward(&self, hash: &str, author: &AccountId) -> Result<u128> {
		let hash: [u8; 32] = ::hex::decode(hash.trim_start_matches("0x"))
			.ok()
			.and_then(|b| b.try_into().ok())
			.ok_or("invalid block hash")?;
		let connection = self.connection().await?;
		let result = async {
			let events = connection.api.events().at(subxt::utils::H256(hash)).await.map_err(|e| e.to_string())?;
			let mut reward = 0u128;
			for event in events.find::<emission::events::Rewarded>() {
				let event = event.map_err(|e| e.to_string())?;
				if &event.author == author {
					reward += event.amount;
				}
			}
			for event in events.find::<transaction_payment::events::TransactionFeePaid>() {
				let event = event.map_err(|e| e.to_string())?;
				reward += event.actual_fee + event.tip;
			}
			for event in events.find::<shielded::events::Transacted>() {
				reward += u128::from(event.map_err(|e| e.to_string())?.fee);
			}
			Ok(reward)
		}
		.await;
		self.checked(result).await
	}

	pub async fn transferable(&self, account: &AccountId) -> Result<u128> {
		let connection = self.connection().await?;
		let result = connection.client.balance(account).await.map(|b| b.transferable.planck()).map_err(|e| e.to_string());
		self.checked(result).await
	}

	/// Pay `amount` to `to`: Ok with the transaction and block hashes once included; Err(true)
	/// when the transfer surely did not happen, Err(false) when it is not known.
	pub async fn pay(&self, key: &Keypair, to: &AccountId, amount: u128) -> std::result::Result<(String, String), (bool, String)> {
		let connection = self.connection().await.map_err(|e| (true, e))?;
		match connection.client.transfer(key, to, Amount(amount)).await {
			Ok(outcome) => Ok((outcome.extrinsic_hash, outcome.block_hash)),
			Err(tesserax_sdk::Error::Dispatch(message)) => Err((true, message)),
			Err(error) => {
				let error = error.to_string();
				self.failed(&error).await;
				Err((false, error))
			},
		}
	}
}

/// Settings of the payouts.
pub struct Payouts {
	pub min_payout: u128,
	pub interval: Duration,
}

/// Follow the blocks of the pool until they are final, then credit them.
pub async fn confirm_blocks(chain: Chain, ledger: Arc<Ledger>, author: AccountId, maturity: u64, fee_bps: u32) {
	let mut interval = tokio::time::interval(Duration::from_secs(20));
	loop {
		interval.tick().await;
		let result: Result<()> = async {
			let best = chain.best_number().await?;
			for block in ledger.pending_blocks().map_err(|e| e.to_string())? {
				let canonical = chain.block_at(block.height).await?;
				let ours = canonical.as_ref().filter(|b| b.nonce == block.nonce);
				if block.hash.is_none() {
					if let Some(ours) = ours {
						ledger.set_block_hash(&block.nonce, &ours.hash).map_err(|e| e.to_string())?;
					}
				}
				if best < block.height + maturity {
					continue;
				}
				match ours {
					Some(ours) => {
						let reward = chain.reward(&ours.hash, &author).await?;
						ledger.confirm_block(&block.nonce, &ours.hash, reward, fee_bps).map_err(|e| e.to_string())?;
						tracing::info!("Block #{} is final: {} TESS to split", block.height, Amount(reward));
					},
					None => {
						ledger.orphan_block(&block.nonce).map_err(|e| e.to_string())?;
						tracing::warn!("Block #{} left the chain: orphaned", block.height);
					},
				}
			}
			Ok(())
		}
		.await;
		if let Err(error) = result {
			tracing::warn!("Following the blocks of the pool: {error}");
		}
	}
}

/// Pay the balances of at least the minimum payout, every interval.
pub async fn pay_miners(chain: Chain, ledger: Arc<Ledger>, key: Keypair, settings: Arc<Payouts>) {
	let mut interval = tokio::time::interval(settings.interval);
	let fee_margin = Amount::from_tess("0.01").expect("a valid amount; qed").planck();
	loop {
		interval.tick().await;
		let due = match ledger.balances_due(settings.min_payout) {
			Ok(due) if !due.is_empty() => due,
			Ok(_) => continue,
			Err(e) => {
				tracing::warn!("Reading the balances: {e}");
				continue;
			},
		};
		let account = key.account_id();
		let total: u128 = due.iter().map(|(_, amount)| amount).sum();
		match chain.transferable(&account).await {
			Ok(available) if available >= total + fee_margin * due.len() as u128 => {},
			Ok(available) => {
				tracing::warn!("Payouts postponed: they need {} TESS, the pool account has {}", Amount(total), Amount(available));
				continue;
			},
			Err(e) => {
				tracing::warn!("Payouts postponed: {e}");
				continue;
			},
		}
		for (address, amount) in due {
			let Ok(to) = tesserax_sdk::parse_address(&address) else { continue };
			let id = match ledger.start_payment(&address, amount, now_ms()) {
				Ok(id) => id,
				Err(e) => {
					tracing::warn!("Recording a payment to {address}: {e}");
					continue;
				},
			};
			let recorded = match chain.pay(&key, &to, amount).await {
				Ok((tx, block)) => {
					tracing::info!("Paid {} TESS to {address} in {tx}", Amount(amount));
					ledger.payment_sent(id, &tx, &block)
				},
				Err((true, error)) => {
					tracing::warn!("Payment of {} TESS to {address} failed: {error}", Amount(amount));
					ledger.payment_failed(id, &error)
				},
				Err((false, error)) => {
					tracing::error!("Payment {id} to {address} may or may not have happened: {error}. Check the chain before paying it again.");
					ledger.payment_unknown(id, &error)
				},
			};
			if let Err(e) = recorded {
				tracing::error!("Recording payment {id}: {e}");
			}
		}
	}
}

pub fn now_ms() -> i64 {
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_millis() as i64)
		.unwrap_or_default()
}
