//! Atomic swaps: offers of TESS, buying them with a payment locked on Bitcoin or an EVM chain, and
//! the maker that fills offers and collects their payments (`docs/swaps.md`).

use super::{error, tess, Result, Wallet};
use clap::Subcommand;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tesserax_sdk::{
	format_address, parse_address,
	swaps::{
		bitcoin::{self as btc, BitcoinKey, BitcoinLock, CompressedPublicKey, Esplora, Htlc, OutPoint, Secp256k1},
		evm::{self, Evm, EvmAddress, LockState, PrivateKeySigner, B256, U256},
		hash_of, new_secret, payment_due, tess_for, Asset, Chain, Payment, SwapEvent,
	},
	AccountId, Amount, Client,
};

/// Seconds a payment stays locked for the maker: the maker must fill and claim within it.
const PAYMENT_LOCK_SECONDS: u64 = 24 * 3600;

/// Seconds a payment lock must still last for the maker to fill it: the six hours the taker has
/// to claim the TESS, and six more for the maker to claim the payment.
const MIN_REMAINING_SECONDS: u64 = 12 * 3600;

#[derive(Subcommand)]
pub enum SwapCommand {
	/// The open offers.
	Offers,
	/// Offer TESS of an account for a payment on Bitcoin or an EVM chain.
	Offer {
		account: String,
		/// bitcoin, ethereum, base or arbitrum.
		#[arg(long)]
		chain: String,
		/// btc on Bitcoin; eth, usdc or usdt on the EVM chains.
		#[arg(long)]
		asset: String,
		/// Price of one TESS in the asset, such as 0.05.
		#[arg(long)]
		price: String,
		/// TESS offered.
		#[arg(long)]
		amount: String,
		/// Smallest trade in TESS.
		#[arg(long, default_value = "1")]
		min: String,
		/// EVM address the payments go to; on Bitcoin, the compressed public key claiming them,
		/// that of --btc-key-file by default.
		#[arg(long)]
		payout: Option<String>,
		#[command(flatten)]
		pay: PayArgs,
	},
	/// Withdraw an offer: what remains returns to the account.
	Cancel { account: String, offer: u64 },
	/// Buy TESS of an offer: lock the payment on its chain, wait for the maker, claim the TESS.
	Buy {
		/// Account receiving the TESS: it needs no TESS.
		account: String,
		offer: u64,
		/// TESS to buy.
		amount: String,
		#[command(flatten)]
		pay: PayArgs,
	},
	/// Claim the TESS of a payment already locked, once the maker filled it.
	Claim {
		account: String,
		/// The id of the payment lock, 0x..., or its Bitcoin output, txid:vout.
		lock: String,
		#[command(flatten)]
		pay: PayArgs,
	},
	/// Take back a payment the maker never filled, once its lock expired.
	RefundPayment {
		/// The id of the payment lock, 0x..., or its Bitcoin output, txid:vout.
		lock: String,
		#[command(flatten)]
		pay: PayArgs,
	},
	/// Run the maker of an account: fill its offers for the payments locked for them, collect the
	/// payments once their secrets are revealed, and take back trades that expired.
	Make {
		account: String,
		/// Seconds between two rounds.
		#[arg(long, default_value_t = 20)]
		interval: u64,
		#[command(flatten)]
		pay: PayArgs,
	},
}

#[derive(clap::Args)]
pub struct PayArgs {
	/// File holding the private key of the EVM account, in hexadecimal: it pays the locks and the
	/// gas.
	#[arg(long, env = "TESSERAX_EVM_KEY_FILE")]
	evm_key_file: Option<PathBuf>,
	/// RPC of the EVM chain, instead of its public RPC: `base=https://...`, repeated.
	#[arg(long = "evm-rpc")]
	evm_rpc: Vec<String>,
	/// Use one local EVM chain (anvil, chain id 31337) for every chain, at this RPC: for tests.
	#[arg(long)]
	evm_local: Option<String>,
	/// File holding the Bitcoin private key, in WIF: it pays the locks of buys from its P2WPKH
	/// address, and the maker claims the payments with it.
	#[arg(long, env = "TESSERAX_BTC_KEY_FILE")]
	btc_key_file: Option<PathBuf>,
	/// Esplora API of the Bitcoin network, instead of the one of mempool.space.
	#[arg(long)]
	esplora: Option<String>,
}

fn chain_named(name: &str) -> Result<Chain> {
	match name.to_lowercase().as_str() {
		"bitcoin" => Ok(Chain::Bitcoin),
		"ethereum" => Ok(Chain::Ethereum),
		"base" => Ok(Chain::Base),
		"arbitrum" => Ok(Chain::Arbitrum),
		other => Err(format!("unknown chain {other}: bitcoin, ethereum, base or arbitrum")),
	}
}

fn payment_named(chain: &str, asset: &str) -> Result<Payment> {
	let chain = chain_named(chain)?;
	let asset = match (chain, asset.to_lowercase().as_str()) {
		(Chain::Bitcoin, "btc") => Asset::Native,
		(Chain::Bitcoin, other) => return Err(format!("{other} is not paid on Bitcoin: btc")),
		(_, "eth") => Asset::Native,
		(_, "usdc") => Asset::Usdc,
		(_, "usdt") => Asset::Usdt,
		(_, other) => return Err(format!("unknown asset {other}: eth, usdc or usdt")),
	};
	Ok(Payment { chain, asset })
}

/// `text`, a decimal amount, in units of `decimals`.
fn units(text: &str, decimals: u32) -> Result<u128> {
	let (whole, fraction) = text.trim().split_once('.').unwrap_or((text.trim(), ""));
	if fraction.len() > decimals as usize || whole.is_empty() && fraction.is_empty() {
		return Err(format!("{text}: at most {decimals} decimals"));
	}
	let digits = format!("{whole}{fraction:0<width$}", width = decimals as usize);
	digits.parse::<u128>().map_err(|_| format!("{text} is not an amount"))
}

/// `value` units of `decimals` as a decimal amount.
fn decimal(value: u128, decimals: u32) -> String {
	let scale = 10u128.pow(decimals);
	let fraction = format!("{:0width$}", value % scale, width = decimals as usize);
	let fraction = fraction.trim_end_matches('0');
	if fraction.is_empty() {
		(value / scale).to_string()
	} else {
		format!("{}.{fraction}", value / scale)
	}
}

fn hex32(text: &str) -> Result<[u8; 32]> {
	let bytes = hex::decode(text.trim().trim_start_matches("0x")).map_err(|_| "expected 0x and 64 digits".to_string())?;
	bytes.try_into().map_err(|_| "expected 0x and 64 digits".to_string())
}

/// A lock as given on the command line: 0x and 32 bytes on an EVM chain, txid:vout on Bitcoin.
fn lock_named(text: &str) -> Result<(Option<Chain>, Vec<u8>)> {
	match text.trim().split_once(':') {
		Some((txid, vout)) => {
			let outpoint = OutPoint {
				txid: txid.parse().map_err(|_| format!("{txid} is not a Bitcoin transaction id"))?,
				vout: vout.parse().map_err(|_| format!("{vout} is not an output index"))?,
			};
			Ok((Some(Chain::Bitcoin), btc::outpoint_bytes(&outpoint)))
		},
		None => Ok((None, hex32(text)?.to_vec())),
	}
}

/// What a buy paid in BTC keeps to take its payment back: the script and its output.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedBitcoinLock {
	hash: String,
	maker: String,
	taker: String,
	timelock: u32,
	amount: u64,
	offer: u64,
	account: String,
	outpoint: Option<String>,
}

impl SavedBitcoinLock {
	fn lock(&self) -> Result<BitcoinLock> {
		let key = |text: &str| -> Result<CompressedPublicKey> {
			CompressedPublicKey::from_slice(&hex::decode(text).map_err(error)?).map_err(error)
		};
		let outpoint: OutPoint = self.outpoint.as_deref().ok_or("the payment was never broadcast")?.parse().map_err(error)?;
		Ok(BitcoinLock {
			outpoint,
			amount: self.amount,
			htlc: Htlc { hash: hex32(&self.hash)?, maker: key(&self.maker)?, taker: key(&self.taker)?, timelock: self.timelock },
			offer: self.offer,
			taker: parse_address(&self.account).map_err(error)?,
		})
	}
}

fn bitcoin_lock_path(w: &Wallet, hash: &[u8; 32]) -> PathBuf {
	w.dir.join("swaps").join(format!("{}.btc.json", hex::encode(hash)))
}

fn save_json(path: &PathBuf, value: &impl serde::Serialize) -> Result<()> {
	std::fs::create_dir_all(path.parent().expect("a directory; qed")).map_err(error)?;
	let partial = path.with_extension("partial");
	std::fs::write(&partial, serde_json::to_vec_pretty(value).map_err(error)?).map_err(error)?;
	std::fs::rename(&partial, path).map_err(error)
}

impl PayArgs {
	fn btc_key(&self) -> Result<BitcoinKey> {
		let path = self.btc_key_file.as_ref().ok_or("give the Bitcoin key with --btc-key-file")?;
		let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
		BitcoinKey::from_wif(text.trim()).map_err(|_| format!("{}: not a Bitcoin private key in WIF", path.display()))
	}

	/// The Bitcoin network of the Tesserax network of `client`, through its Esplora API.
	async fn bitcoin(&self, client: &Client) -> Result<Esplora> {
		let info = client.chain_info().await.map_err(error)?;
		Ok(Esplora::new(btc::network_for(info.chain != "Tesserax"), self.esplora.as_deref()))
	}

	fn key(&self) -> Result<Option<PrivateKeySigner>> {
		let Some(path) = &self.evm_key_file else { return Ok(None) };
		let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
		text.trim().parse().map(Some).map_err(|_| format!("{}: not an EVM private key", path.display()))
	}

	/// The EVM chain `chain` of the network of `client`.
	async fn connect(&self, client: &Client, chain: Chain, signing: bool) -> Result<Evm> {
		let key = self.key()?;
		if signing && key.is_none() {
			return Err("give the EVM key with --evm-key-file".into());
		}
		let mut network = network_of(client, chain).await?;
		let rpc = self.evm_rpc.iter().find_map(|r| {
			let (name, url) = r.split_once('=')?;
			chain_named(name).ok().filter(|c| *c == chain).map(|_| url.to_string())
		});
		let rpc = match &self.evm_local {
			Some(url) => {
				network.chain_id = 31_337;
				network.name = "Local EVM";
				Some(url.clone())
			},
			None => rpc,
		};
		Evm::connect(network, rpc.as_deref(), key).await.map_err(error)
	}
}

/// The EVM networks of the Tesserax network of `client`: the mainnets for the mainnet, their
/// testnets otherwise.
async fn network_of(client: &Client, chain: Chain) -> Result<evm::Network> {
	let info = client.chain_info().await.map_err(error)?;
	evm::networks(info.chain != "Tesserax")
		.into_iter()
		.find(|n| n.chain == chain)
		.ok_or_else(|| format!("{chain:?} is not an EVM chain of swaps"))
}

fn price_text(payment: Payment, price: u128) -> String {
	format!("{} {} per TESS on {:?}", decimal(price, payment.decimals()), payment.symbol(), payment.chain)
}

/// Where secrets of the buys of the wallet are kept, by hash, until their TESS are claimed.
fn secret_path(w: &Wallet, hash: &[u8; 32]) -> PathBuf {
	w.dir.join("swaps").join(format!("{}.secret", hex::encode(hash)))
}

fn save_secret(w: &Wallet, secret: &[u8; 32]) -> Result<()> {
	let path = secret_path(w, &hash_of(secret));
	std::fs::create_dir_all(path.parent().expect("a directory; qed")).map_err(error)?;
	let partial = path.with_extension("partial");
	std::fs::write(&partial, hex::encode(secret)).map_err(error)?;
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o600)).map_err(error)?;
	}
	std::fs::rename(&partial, &path).map_err(error)
}

fn load_secret(w: &Wallet, hash: &[u8; 32]) -> Result<[u8; 32]> {
	let text = std::fs::read_to_string(secret_path(w, hash)).map_err(|_| "no secret of this payment in this wallet")?;
	hex32(&text)
}

pub async fn swap(w: &mut Wallet, command: SwapCommand) -> Result<()> {
	match command {
		SwapCommand::Offers => {
			let offers = w.client().await?.offers().await.map_err(error)?;
			let json: Vec<_> = offers
				.iter()
				.map(|o| {
					serde_json::json!({
						"id": o.id,
						"maker": format_address(&o.maker),
						"chain": format!("{:?}", o.payment.chain).to_lowercase(),
						"asset": o.payment.symbol(),
						"price": decimal(o.price, o.payment.decimals()),
						"available": o.available,
						"min_fill": o.min_fill,
						"payout": format!("0x{}", hex::encode(&o.payout)),
						"fills": o.fills,
						"completed": o.completed,
					})
				})
				.collect();
			w.print(json, || {
				if offers.is_empty() {
					"No offer.".into()
				} else {
					offers
						.iter()
						.map(|o| {
							format!(
								"#{} {} TESS at {} · from {} TESS · {}/{} trades completed",
								o.id,
								o.available,
								price_text(o.payment, o.price),
								o.min_fill,
								o.completed,
								o.fills
							)
						})
						.collect::<Vec<_>>()
						.join("\n")
				}
			});
		},
		SwapCommand::Offer { account, chain, asset, price, amount, min, payout, pay } => {
			let payment = payment_named(&chain, &asset)?;
			let price = units(&price, payment.decimals())?;
			let (amount, min) = (tess(&amount)?, tess(&min)?);
			let payout = match (payment.chain, payout) {
				(Chain::Bitcoin, None) => {
					let key = pay.btc_key()?;
					CompressedPublicKey::from_private_key(&Secp256k1::new(), &key).map_err(error)?.to_bytes().to_vec()
				},
				(_, None) => return Err("give the EVM address paid with --payout".into()),
				(_, Some(payout)) => hex::decode(payout.trim().trim_start_matches("0x")).map_err(|_| "the payout is hexadecimal")?,
			};
			match payment.chain {
				Chain::Bitcoin if CompressedPublicKey::from_slice(&payout).is_err() =>
					return Err("the payout on Bitcoin is a compressed public key: 66 digits".into()),
				Chain::Bitcoin => {},
				_ if payout.len() != 20 => return Err("the payout is an EVM address: 0x and 40 digits".into()),
				_ => {},
			}
			w.confirm(&format!(
				"Offer {amount} TESS of {account} at {}, paid to 0x{}; the TESS are held until sold or withdrawn",
				price_text(payment, price),
				hex::encode(&payout)
			))?;
			let key = w.key(&account)?;
			let id = w.client().await?.create_offer(&key, payment, price, amount, min, payout).await.map_err(error)?;
			w.print(serde_json::json!({ "offer": id }), || {
				format!("Offer #{id} open. Run `tesserax-wallet swap make {account}` to fill it as payments come.")
			});
		},
		SwapCommand::Cancel { account, offer } => {
			w.confirm(&format!("Withdraw offer #{offer}"))?;
			let key = w.key(&account)?;
			let outcome = w.client().await?.cancel_offer(&key, offer).await.map_err(error)?;
			w.sent(outcome);
		},
		SwapCommand::Buy { account, offer, amount, pay } => {
			let taker = w.resolve(&account)?;
			let amount = tess(&amount)?;
			let client = w.client().await?.clone();
			let o = client.offer(offer).await.map_err(error)?.ok_or("no such offer")?;
			if amount.0 > o.available.0 {
				return Err(format!("the offer has {} TESS", o.available));
			}
			if amount.0 < o.min_fill.0 && amount.0 != o.available.0 {
				return Err(format!("the offer sells at least {} TESS", o.min_fill));
			}
			let due = payment_due(amount.0, o.price).ok_or("amount too large")?;
			if o.payment.chain == Chain::Bitcoin {
				let lock = buy_with_bitcoin(w, &client, &o, amount, due, &taker, &pay).await?;
				return claim_when_filled(w, &client, Chain::Bitcoin, &lock, &taker, Some(amount)).await;
			}
			let chain = pay.connect(&client, o.payment.chain, true).await?;
			w.confirm(&format!(
				"Lock {} {} on {} for {amount} TESS of offer #{offer}, to {}; it returns to you in 24 hours if the maker does not fill it",
				decimal(due, o.payment.decimals()),
				o.payment.symbol(),
				chain.network.name,
				format_address(&taker),
			))?;
			let (secret, hash) = new_secret().map_err(error)?;
			// The secret is saved before anything is locked: without it, the TESS cannot be claimed.
			save_secret(w, &secret)?;
			let (_, now) = chain.head().await.map_err(error)?;
			let recipient = EvmAddress::from_slice(&o.payout);
			let lock = chain
				.lock_payment(o.payment.asset, recipient, U256::from(due), hash, now + PAYMENT_LOCK_SECONDS, offer, &taker)
				.await
				.map_err(error)?;
			if !w.json {
				eprintln!("Payment locked: {lock}. Waiting for the maker to fill it…");
			}
			claim_when_filled(w, &client, o.payment.chain, lock.as_slice(), &taker, Some(amount)).await?;
		},
		SwapCommand::Claim { account, lock, pay: _ } => {
			let taker = w.resolve(&account)?;
			let (chain, lock) = lock_named(&lock)?;
			let client = w.client().await?.clone();
			let chains = match chain {
				Some(chain) => vec![chain],
				None => vec![Chain::Ethereum, Chain::Base, Chain::Arbitrum],
			};
			let mut found = None;
			for c in chains {
				if client.trade_of_lock(c, &lock).await.map_err(error)?.is_some() {
					found = Some(c);
				}
			}
			let chain = found.ok_or("the maker has not filled this payment yet")?;
			claim_when_filled(w, &client, chain, &lock, &taker, None).await?;
		},
		SwapCommand::RefundPayment { lock, pay } => {
			let (chain, bytes) = lock_named(&lock)?;
			if chain == Some(Chain::Bitcoin) {
				return refund_bitcoin(w, &bytes, &pay).await;
			}
			let lock = B256::from_slice(&bytes);
			let client = w.client().await?.clone();
			let mut last = String::from("no lock with this id on the EVM chains");
			for chain in [Chain::Base, Chain::Arbitrum, Chain::Ethereum] {
				let Ok(evm_chain) = pay.connect(&client, chain, true).await else { continue };
				match evm_chain.lock_status(lock).await {
					Ok(Some(status)) if status.state == LockState::Locked => {
						let (_, now) = evm_chain.head().await.map_err(error)?;
						if now < status.timelock {
							let hours = (status.timelock - now).div_ceil(3600);
							return Err(format!("the payment can be taken back in {hours} hour(s), once its lock expires"));
						}
						let tx = evm_chain.refund(lock).await.map_err(error)?;
						// The secret of a refunded payment is of no use any more.
						let _ = std::fs::remove_file(secret_path(w, &status.hash));
						w.print(serde_json::json!({ "transaction": tx.to_string() }), || format!("Payment refunded in {tx}"));
						return Ok(());
					},
					Ok(Some(status)) => last = format!("the payment is already {:?}", status.state),
					_ => {},
				}
			}
			return Err(last);
		},
		SwapCommand::Make { account, interval, pay } => make(w, &account, interval, pay).await?,
	}
	Ok(())
}

/// Lock the payment `due` in satoshis of offer `o` from the Bitcoin key, for `amount` TESS to
/// `taker`. Returns the payment lock: the outpoint of the script.
async fn buy_with_bitcoin(
	w: &mut Wallet,
	client: &Client,
	o: &tesserax_sdk::swaps::Offer,
	amount: Amount,
	due: u128,
	taker: &AccountId,
	pay: &PayArgs,
) -> Result<Vec<u8>> {
	let key = pay.btc_key()?;
	let esplora = pay.bitcoin(client).await?;
	let refund = CompressedPublicKey::from_private_key(&Secp256k1::new(), &key).map_err(error)?;
	let maker = CompressedPublicKey::from_slice(&o.payout).map_err(|_| "the payout of this offer is not a Bitcoin key")?;
	let due = u64::try_from(due).map_err(|_| "amount too large")?;
	let from = btc::address_of(&refund, esplora.network);
	let utxos = esplora.utxos(&from).await.map_err(error)?;
	if utxos.is_empty() {
		return Err(format!("{from} has no bitcoin: send it the payment and its fee first"));
	}
	let fee_rate = esplora.fee_rate(2).await.map_err(error)?;
	let now = esplora.median_time().await.map_err(error)?;
	let timelock = u32::try_from(now + PAYMENT_LOCK_SECONDS).map_err(error)?;
	w.confirm(&format!(
		"Lock {} BTC from {from} for {amount} TESS of offer #{}, to {}, at about {fee_rate:.1} sat/vB; it returns to you after 24 hours if the maker does not fill it",
		decimal(u128::from(due), 8),
		o.id,
		format_address(taker),
	))?;
	let (secret, hash) = new_secret().map_err(error)?;
	let htlc = Htlc { hash, maker, taker: refund, timelock };
	let announcement = btc::Announcement { offer: o.id, taker: taker.clone(), hash, timelock, refund: None };
	let tx = btc::lock_transaction(&key, &utxos, &htlc, &announcement, due, fee_rate, esplora.network).map_err(error)?;
	let lock = btc::find_lock(&tx, &maker).ok_or("the payment built cannot be found again")?;
	// The secret and the script are saved before anything is broadcast: they claim the TESS and
	// take the payment back.
	save_secret(w, &secret)?;
	let saved = SavedBitcoinLock {
		hash: hex::encode(hash),
		maker: hex::encode(maker.to_bytes()),
		taker: hex::encode(refund.to_bytes()),
		timelock,
		amount: due,
		offer: o.id,
		account: format_address(taker),
		outpoint: Some(lock.outpoint.to_string()),
	};
	save_json(&bitcoin_lock_path(w, &hash), &saved)?;
	let txid = esplora.broadcast(&tx).await.map_err(error)?;
	if !w.json {
		eprintln!(
			"Payment locked in {txid}, output {}. The maker fills it after {} confirmations, about {} minutes…",
			lock.outpoint.vout,
			btc::MIN_CONFIRMATIONS,
			btc::MIN_CONFIRMATIONS * 10
		);
	}
	Ok(lock.lock_bytes())
}

/// Take back the Bitcoin payment of outpoint `bytes`, once its timelock passed.
async fn refund_bitcoin(w: &mut Wallet, bytes: &[u8], pay: &PayArgs) -> Result<()> {
	let outpoint = btc::outpoint_of(bytes).ok_or("not a Bitcoin output")?;
	let saved = std::fs::read_dir(w.dir.join("swaps"))
		.map_err(|_| "no Bitcoin payment in this wallet")?
		.filter_map(|e| e.ok())
		.filter(|e| e.file_name().to_string_lossy().ends_with(".btc.json"))
		.filter_map(|e| serde_json::from_slice::<SavedBitcoinLock>(&std::fs::read(e.path()).ok()?).ok())
		.find(|s| s.outpoint.as_deref() == Some(outpoint.to_string().as_str()))
		.ok_or("this wallet did not lock this Bitcoin payment")?;
	let lock = saved.lock()?;
	let key = pay.btc_key()?;
	let client = w.client().await?.clone();
	let esplora = pay.bitcoin(&client).await?;
	if esplora.is_spent(&outpoint).await.map_err(error)? {
		return Err("the payment is already claimed or taken back".into());
	}
	let now = esplora.median_time().await.map_err(error)?;
	if now <= u64::from(lock.htlc.timelock) {
		let hours = (u64::from(lock.htlc.timelock) + 1 - now).div_ceil(3600);
		return Err(format!("the payment can be taken back in about {hours} hour(s), once the median time of Bitcoin passes its lock"));
	}
	let fee_rate = esplora.fee_rate(6).await.map_err(error)?;
	let tx = btc::refund_transaction(&lock, &key, fee_rate, esplora.network).map_err(error)?;
	let txid = esplora.broadcast(&tx).await.map_err(error)?;
	let _ = std::fs::remove_file(secret_path(w, &lock.htlc.hash));
	let _ = std::fs::remove_file(bitcoin_lock_path(w, &lock.htlc.hash));
	w.print(serde_json::json!({ "transaction": txid.to_string() }), || format!("Payment taken back in {txid}"));
	Ok(())
}

/// Wait until the maker fills the payment lock `lock`, check the trade, and claim it with the
/// secret saved for its hash.
async fn claim_when_filled(
	w: &mut Wallet,
	client: &Client,
	chain: Chain,
	lock: &[u8],
	taker: &AccountId,
	expected: Option<Amount>,
) -> Result<()> {
	let trade = loop {
		if let Some(id) = client.trade_of_lock(chain, lock).await.map_err(error)? {
			match client.trade(id).await.map_err(error)? {
				Some(trade) => break trade,
				None => return Err(format!("trade #{id} of this payment is already claimed or refunded")),
			}
		}
		tokio::time::sleep(Duration::from_secs(10)).await;
	};
	if trade.taker != *taker {
		return Err(format!("the maker filled for {}, not for {}: do not claim, take the payment back after 24 hours", format_address(&trade.taker), format_address(taker)));
	}
	if let Some(expected) = expected {
		if trade.amount.0 < expected.0 {
			return Err(format!("the maker filled {} TESS instead of {expected}: take the payment back after 24 hours", trade.amount));
		}
	}
	let secret = load_secret(w, &trade.hash)?;
	let outcome = client.claim_trade(trade.id, secret).await.map_err(error)?;
	if !w.json {
		eprintln!("Claimed {} TESS of trade #{}", trade.amount, trade.id);
	}
	w.sent(outcome);
	let _ = std::fs::remove_file(secret_path(w, &trade.hash));
	let _ = std::fs::remove_file(bitcoin_lock_path(w, &trade.hash));
	Ok(())
}

/// What the maker has read: the last block of each chain and of Tesserax, and its trades waiting
/// for their claim, with the chain and the lock of their payment.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct MakerState {
	evm_blocks: BTreeMap<String, u64>,
	tesserax_block: u32,
	trades: BTreeMap<u64, (String, String)>,
	/// Payments to collect: their trade, chain, lock and the secret its claim revealed.
	collect: BTreeMap<u64, (String, String, String)>,
	/// Bitcoin transactions to the maker already read: filled, or not a payment that fits.
	#[serde(default)]
	bitcoin_seen: std::collections::BTreeSet<String>,
}

fn maker_state_path(w: &Wallet, account: &str) -> PathBuf {
	w.dir.join("swaps").join(format!("maker-{account}.json"))
}

async fn make(w: &mut Wallet, account: &str, interval: u64, args: PayArgs) -> Result<()> {
	let key = w.key(account)?;
	let maker = key.account_id();
	let client = w.client().await?.clone();
	let path = maker_state_path(w, account);
	let mut state: MakerState =
		std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
	if state.tesserax_block == 0 {
		state.tesserax_block = client.best_block_number().await.map_err(error)?;
	}
	let mut chains: BTreeMap<String, Evm> = BTreeMap::new();
	eprintln!("Maker of {} running: filling offers, collecting payments, refunding expired trades.", format_address(&maker));
	loop {
		if let Err(e) = round(&client, &key, &maker, &args, &mut chains, &mut state).await {
			eprintln!("{e}");
		}
		std::fs::create_dir_all(path.parent().expect("a directory; qed")).map_err(error)?;
		std::fs::write(&path, serde_json::to_vec(&state).map_err(error)?).map_err(error)?;
		tokio::time::sleep(Duration::from_secs(interval)).await;
	}
}

async fn round(
	client: &Client,
	key: &tesserax_sdk::Keypair,
	maker: &AccountId,
	args: &PayArgs,
	chains: &mut BTreeMap<String, Evm>,
	state: &mut MakerState,
) -> Result<()> {
	let offers: Vec<_> = client.offers().await.map_err(error)?.into_iter().filter(|o| o.maker == *maker).collect();

	// Payments locked for the offers: fill them.
	for chain in [Chain::Ethereum, Chain::Base, Chain::Arbitrum] {
		let mine: Vec<_> = offers.iter().filter(|o| o.payment.chain == chain).collect();
		if mine.is_empty() {
			continue;
		}
		let name = format!("{chain:?}");
		if !chains.contains_key(&name) {
			match args.connect(client, chain, true).await {
				Ok(evm) => chains.insert(name.clone(), evm),
				Err(e) => {
					eprintln!("{chain:?} offers: {e}");
					continue;
				},
			};
		}
		let evm = &chains[&name];
		let (head, now) = match evm.head().await {
			Ok(head) => head,
			Err(e) => {
				eprintln!("{chain:?}: {e}");
				continue;
			},
		};
		let from = state.evm_blocks.get(&name).map_or(head.saturating_sub(1_000), |b| b + 1);
		if from > head {
			continue;
		}
		let mut payouts: Vec<_> = mine.iter().map(|o| o.payout.clone()).collect();
		payouts.dedup();
		for payout in payouts {
			let recipient = EvmAddress::from_slice(&payout);
			for lock in evm.locks(from, head, Some(recipient)).await.map_err(error)? {
				let Some(offer) = mine.iter().find(|o| o.id == lock.offer && o.payout == payout) else { continue };
				let Some(paid) = lock.amount_of(&evm.network, offer.payment) else { continue };
				if lock.state != LockState::Locked || lock.timelock < now + MIN_REMAINING_SECONDS {
					continue;
				}
				if client.trade_of_lock(chain, &lock.id.0).await.map_err(error)?.is_some() {
					continue;
				}
				let amount = tess_for(paid, offer.price);
				let current = client.offer(offer.id).await.map_err(error)?;
				let Some(current) = current else { continue };
				if amount == 0 || amount > current.available.0 || (amount < current.min_fill.0 && amount != current.available.0) {
					eprintln!("Payment {} does not fit offer #{}: left for its sender to take back", lock.id, offer.id);
					continue;
				}
				match client.fill_offer(key, offer.id, &lock.taker, Amount(amount), lock.hash, lock.id.0.to_vec()).await {
					Ok(trade) => {
						state.trades.insert(trade, (name.clone(), lock.id.to_string()));
						eprintln!(
							"Filled trade #{trade}: {} TESS for {} {} from {}",
							Amount(amount),
							decimal(paid, offer.payment.decimals()),
							offer.payment.symbol(),
							lock.sender
						)
					},
					Err(e) => eprintln!("Filling payment {}: {e}", lock.id),
				}
			}
		}
		state.evm_blocks.insert(name, head);
	}

	let bitcoin: Vec<_> = offers.iter().filter(|o| o.payment.chain == Chain::Bitcoin).collect();
	if !bitcoin.is_empty() {
		if let Err(e) = fill_bitcoin(client, key, args, &bitcoin, state).await {
			eprintln!("Bitcoin offers: {e}");
		}
	}

	// Secrets revealed by claims: collect the payments. Trades expired: take them back.
	let best = client.best_block_number().await.map_err(error)?;
	for trade in client.trades().await.map_err(error)? {
		if trade.maker == *maker && trade.expires < best {
			match client.refund_trade(key, trade.id).await {
				Ok(_) => eprintln!("Trade #{} expired unclaimed: its TESS are back", trade.id),
				Err(e) => eprintln!("Refunding trade #{}: {e}", trade.id),
			}
		}
	}
	for number in state.tesserax_block + 1..=best {
		for event in client.swap_events(number).await.map_err(error)? {
			match event {
				SwapEvent::Filled(trade) if trade.maker == *maker => {
					let chain = format!("{:?}", trade.payment.chain);
					state.trades.entry(trade.id).or_insert((chain, format!("0x{}", hex::encode(&trade.payment_lock))));
				},
				SwapEvent::Claimed { trade, secret } => {
					if let Some((chain, lock)) = state.trades.remove(&trade) {
						state.collect.insert(trade, (chain, lock, format!("0x{}", hex::encode(secret))));
					}
				},
				SwapEvent::Refunded { trade } => {
					state.trades.remove(&trade);
				},
				_ => {},
			}
		}
		state.tesserax_block = number;
	}

	// Collected until they succeed: the lock of a payment lasts hours after the claim.
	for (trade, (chain, lock, secret)) in state.collect.clone() {
		if chain == "Bitcoin" {
			match collect_bitcoin(client, args, &lock, hex32(&secret)?).await {
				Ok(Some(txid)) => {
					state.collect.remove(&trade);
					eprintln!("Collected the payment of trade #{trade} in {txid}");
				},
				Ok(None) => {
					state.collect.remove(&trade);
					eprintln!("The payment of trade #{trade} is already spent");
				},
				Err(e) => eprintln!("Collecting the payment of trade #{trade}, again next round: {e} (secret {secret})"),
			}
			continue;
		}
		if !chains.contains_key(&chain) {
			let Some(c) = [Chain::Ethereum, Chain::Base, Chain::Arbitrum].into_iter().find(|c| format!("{c:?}") == chain)
			else {
				continue;
			};
			match args.connect(client, c, true).await {
				Ok(evm) => chains.insert(chain.clone(), evm),
				Err(e) => {
					eprintln!("Collecting the payment of trade #{trade}, again next round: {e}");
					continue;
				},
			};
		}
		let evm = &chains[&chain];
		let id = B256::from(hex32(&lock)?);
		// Anyone may collect a payment for its maker.
		if let Ok(Some(status)) = evm.lock_status(id).await {
			if status.state != LockState::Locked {
				state.collect.remove(&trade);
				eprintln!("The payment of trade #{trade} is already {:?}", status.state);
				continue;
			}
		}
		match evm.claim(id, hex32(&secret)?).await {
			Ok(tx) => {
				state.collect.remove(&trade);
				eprintln!("Collected the payment of trade #{trade} in {tx}");
			},
			Err(e) => eprintln!("Collecting the payment of trade #{trade}, again next round: {e} (secret {secret})"),
		}
	}
	Ok(())
}

/// Fill the Bitcoin offers `mine` for the payments locked for them: the maker reads the
/// transactions to its own address, which every payment notifies, once confirmed.
async fn fill_bitcoin(
	client: &Client,
	key: &tesserax_sdk::Keypair,
	args: &PayArgs,
	mine: &[&tesserax_sdk::swaps::Offer],
	state: &mut MakerState,
) -> Result<()> {
	let btc_key = args.btc_key().map_err(|e| format!("Bitcoin offers: {e}"))?;
	let own = CompressedPublicKey::from_private_key(&Secp256k1::new(), &btc_key).map_err(error)?;
	let esplora = args.bitcoin(client).await?;
	let height = esplora.height().await.map_err(error)?;
	let now = esplora.median_time().await.map_err(error)?;
	let mut payouts: Vec<_> = mine.iter().map(|o| o.payout.clone()).collect();
	payouts.dedup();
	for payout in payouts {
		if payout != own.to_bytes() {
			eprintln!("Bitcoin offers paid to 0x{} need its key: --btc-key-file holds another one", hex::encode(&payout));
			continue;
		}
		let address = btc::address_of(&own, esplora.network);
		let mut after = None;
		// The latest transactions, back to those older than a payment lock.
		for _ in 0..8 {
			let txs = esplora.address_txs(&address, after).await.map_err(error)?;
			let mut older = false;
			for t in &txs {
				if t.time.is_some_and(|time| time + PAYMENT_LOCK_SECONDS < now) {
					older = true;
					continue;
				}
				let txid = t.txid.to_string();
				let Some(block) = t.height else { continue };
				if state.bitcoin_seen.contains(&txid) || height.saturating_sub(block) + 1 < btc::MIN_CONFIRMATIONS {
					continue;
				}
				let tx = esplora.transaction(&t.txid).await.map_err(error)?;
				let Some(lock) = btc::find_lock(&tx, &own) else {
					state.bitcoin_seen.insert(txid);
					continue;
				};
				let Some(offer) = mine.iter().find(|o| o.id == lock.offer && o.payout == payout) else {
					state.bitcoin_seen.insert(txid);
					continue;
				};
				let lock_bytes = lock.lock_bytes();
				if u64::from(lock.htlc.timelock) < now + MIN_REMAINING_SECONDS
					|| esplora.is_spent(&lock.outpoint).await.map_err(error)?
					|| client.trade_of_lock(Chain::Bitcoin, &lock_bytes).await.map_err(error)?.is_some()
				{
					state.bitcoin_seen.insert(txid);
					continue;
				}
				let amount = tess_for(u128::from(lock.amount), offer.price);
				let Some(current) = client.offer(offer.id).await.map_err(error)? else { continue };
				if amount == 0 || amount > current.available.0 || (amount < current.min_fill.0 && amount != current.available.0) {
					eprintln!("Payment {} does not fit offer #{}: left for its sender to take back", lock.outpoint, offer.id);
					state.bitcoin_seen.insert(txid);
					continue;
				}
				match client.fill_offer(key, offer.id, &lock.taker, Amount(amount), lock.htlc.hash, lock_bytes.clone()).await {
					Ok(trade) => {
						state.trades.insert(trade, ("Bitcoin".into(), format!("0x{}", hex::encode(&lock_bytes))));
						state.bitcoin_seen.insert(txid);
						eprintln!("Filled trade #{trade}: {} TESS for {} BTC in {}", Amount(amount), decimal(u128::from(lock.amount), 8), lock.outpoint);
					},
					Err(e) => eprintln!("Filling payment {}: {e}", lock.outpoint),
				}
			}
			// Esplora gives 25 confirmed transactions a page.
			after = txs.iter().rev().find(|t| t.height.is_some()).map(|t| t.txid);
			if older || txs.len() < 25 || after.is_none() {
				break;
			}
		}
	}
	Ok(())
}

/// Claim the Bitcoin payment of outpoint `lock` with `secret`, to the address of the maker.
/// Returns the transaction, or nothing if the payment is already spent.
async fn collect_bitcoin(client: &Client, args: &PayArgs, lock: &str, secret: [u8; 32]) -> Result<Option<btc::Txid>> {
	let bytes = hex::decode(lock.trim_start_matches("0x")).map_err(error)?;
	let outpoint = btc::outpoint_of(&bytes).ok_or("not a Bitcoin output")?;
	let key = args.btc_key()?;
	let own = CompressedPublicKey::from_private_key(&Secp256k1::new(), &key).map_err(error)?;
	let esplora = args.bitcoin(client).await?;
	if esplora.is_spent(&outpoint).await.map_err(error)? {
		return Ok(None);
	}
	let tx = esplora.transaction(&outpoint.txid).await.map_err(error)?;
	let lock = btc::find_lock(&tx, &own).filter(|l| l.outpoint == outpoint).ok_or("this payment is not locked for the Bitcoin key")?;
	let fee_rate = esplora.fee_rate(2).await.map_err(error)?;
	let claim = btc::claim_transaction(&lock, secret, &key, fee_rate, esplora.network).map_err(error)?;
	Ok(Some(esplora.broadcast(&claim).await.map_err(error)?))
}
