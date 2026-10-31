//! Atomic swaps: offers of TESS, buying them with a payment locked on Bitcoin or an EVM chain, and
//! the maker that fills offers and collects their payments; bids for TESS, selling into them, and
//! the same maker paying the sales of its bids (`docs/swaps.md`).

use super::{error, tess, Result, Wallet};
use clap::Subcommand;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tesserax_sdk::{
	format_address, parse_address,
	swaps::{
		bitcoin::{self as btc, BitcoinKey, BitcoinLock, CompressedPublicKey, Esplora, Htlc, OutPoint, Secp256k1},
		evm::{self, Evm, EvmAddress, LockState, PrivateKeySigner, B256, U256},
		hash_of, new_secret, payment_due, tess_for, Asset, Chain, Payment, Sale, SwapEvent,
	},
	AccountId, Amount, Client,
};

/// Seconds a payment stays locked for the maker: the maker must fill and claim within it.
const PAYMENT_LOCK_SECONDS: u64 = 24 * 3600;

/// Seconds a payment lock must still last for the maker to fill it: the six hours the taker has
/// to claim the TESS, and six more for the maker to claim the payment.
const MIN_REMAINING_SECONDS: u64 = 12 * 3600;

/// Seconds the payment of a sale stays locked for its seller: the seller claims it within them,
/// and the sale, which lasts 24 hours, leaves the buyer the time to claim the TESS after.
const SALE_PAYMENT_SECONDS: u64 = 6 * 3600;

/// Blocks a sale must still last for its buyer to pay it, 12 hours of 18 seconds: the six hours
/// of the payment, and six more to claim the TESS.
const MIN_SALE_BLOCKS: u32 = 2_400;

/// Seconds the payment of a sale must still last for its seller to claim it.
const MIN_CLAIM_SECONDS: u64 = 3600;

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
	/// The open bids: TESS wanted, paid on Bitcoin or an EVM chain.
	Bids,
	/// Bid for TESS, paid on Bitcoin or an EVM chain: run `swap make` to pay the sales.
	Bid {
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
		/// TESS wanted.
		#[arg(long)]
		amount: String,
		/// Smallest sale in TESS.
		#[arg(long, default_value = "1")]
		min: String,
	},
	/// Withdraw a bid: its deposit returns to the account.
	CancelBid { account: String, bid: u64 },
	/// Sell TESS into a bid: lock them for its buyer, wait for its payment, check it and claim it.
	Sell {
		account: String,
		bid: u64,
		/// TESS to sell.
		amount: String,
		/// EVM address paid, that of --evm-key-file by default; on Bitcoin, the key of
		/// --btc-key-file claims the payment.
		#[arg(long)]
		payout: Option<String>,
		#[command(flatten)]
		pay: PayArgs,
	},
	/// Claim the payment of a sale made by this wallet, once its buyer locked it.
	CollectSale {
		sale: u64,
		#[command(flatten)]
		pay: PayArgs,
	},
	/// Take back the TESS of a sale its buyer never paid, once it expired.
	RefundSale { account: String, sale: u64 },
	/// Run the maker of an account: fill its offers for the payments locked for them, collect the
	/// payments once their secrets are revealed, and take back trades that expired; pay the sales
	/// into its bids, claim their TESS, and take back payments never claimed.
	Make {
		account: String,
		/// Seconds between two rounds.
		#[arg(long, default_value_t = 20)]
		interval: u64,
		/// Keep an offer open, `chain:asset:price:amount:min[:payout]` such as
		/// `bitcoin:btc:0.00001:1000:10`: when the open offers of the account for this payment
		/// hold less than `min` TESS, offer `amount` TESS again. The payout is that of the Bitcoin
		/// key on Bitcoin, an EVM address otherwise. Repeated for several payments.
		#[arg(long = "keep-offer")]
		keep_offers: Vec<String>,
		/// Keep a bid open, `chain:asset:price:amount:min` such as `base:usdc:0.05:1000:10`: when
		/// the open bids of the account for this payment want less than `min` TESS, bid for
		/// `amount` TESS again. Repeated for several payments.
		#[arg(long = "keep-bid")]
		keep_bids: Vec<String>,
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
	/// Create the key of --btc-key-file if the file does not exist, readable by its owner only.
	#[arg(long)]
	create_btc_key: bool,
	/// Esplora API of the Bitcoin network, instead of the one of mempool.space.
	#[arg(long)]
	esplora: Option<String>,
	/// Bitcoin network, instead of the one of the Tesserax network (Bitcoin for the mainnet,
	/// testnet4 otherwise): regtest, for tests with --esplora (swaps/bitcoin/esplora-regtest.mjs).
	#[arg(long)]
	btc_network: Option<String>,
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

/// A new Bitcoin key in WIF at `path`, written once and readable by its owner only.
fn create_btc_key(path: &PathBuf, mainnet: bool) -> Result<()> {
	use std::io::Write;
	let secret = loop {
		let mut bytes = [0u8; 32];
		getrandom::fill(&mut bytes).map_err(error)?;
		if let Ok(secret) = btc::SecretKey::from_slice(&bytes) {
			break secret;
		}
	};
	let network = if mainnet { btc::BitcoinNetwork::Bitcoin } else { btc::BitcoinNetwork::Testnet4 };
	if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
		std::fs::create_dir_all(dir).map_err(error)?;
	}
	let mut options = std::fs::OpenOptions::new();
	options.write(true).create_new(true);
	#[cfg(unix)]
	std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
	let mut file = options.open(path).map_err(|e| format!("{}: {e}", path.display()))?;
	writeln!(file, "{}", BitcoinKey::new(secret, network).to_wif()).map_err(error)?;
	eprintln!("Created the Bitcoin key {}", path.display());
	Ok(())
}

/// An offer the maker keeps open (`--keep-offer`).
struct KeptOffer {
	payment: Payment,
	price: u128,
	amount: Amount,
	min: Amount,
	payout: Vec<u8>,
}

fn kept_offer(spec: &str, pay: &PayArgs) -> Result<KeptOffer> {
	let usage = || format!("{spec}: chain:asset:price:amount:min[:payout], such as bitcoin:btc:0.00001:1000:10");
	let parts: Vec<_> = spec.split(':').collect();
	let (chain, asset, price, amount, min) = match parts[..] {
		[chain, asset, price, amount, min] | [chain, asset, price, amount, min, _] => (chain, asset, price, amount, min),
		_ => return Err(usage()),
	};
	let payment = payment_named(chain, asset)?;
	let payout = match (payment.chain, parts.get(5)) {
		(Chain::Bitcoin, None) =>
			CompressedPublicKey::from_private_key(&Secp256k1::new(), &pay.btc_key()?).map_err(error)?.to_bytes().to_vec(),
		(Chain::Bitcoin, Some(_)) => return Err(format!("{spec}: on Bitcoin, the payout is the key of --btc-key-file")),
		(_, Some(payout)) => match hex::decode(payout.trim_start_matches("0x")) {
			Ok(address) if address.len() == 20 => address,
			_ => return Err(format!("{spec}: the payout is an EVM address, 0x and 40 digits")),
		},
		(_, None) => return Err(usage()),
	};
	let (amount, min) = (tess(amount)?, tess(min)?);
	if min.0 == 0 || min.0 > amount.0 {
		return Err(format!("{spec}: the smallest trade is above zero and at most the amount"));
	}
	Ok(KeptOffer { payment, price: units(price, payment.decimals())?, amount, min, payout })
}

/// A bid the maker keeps open (`--keep-bid`).
struct KeptBid {
	payment: Payment,
	price: u128,
	amount: Amount,
	min: Amount,
}

fn kept_bid(spec: &str) -> Result<KeptBid> {
	let parts: Vec<_> = spec.split(':').collect();
	let [chain, asset, price, amount, min] = parts[..] else {
		return Err(format!("{spec}: chain:asset:price:amount:min, such as base:usdc:0.05:1000:10"));
	};
	let payment = payment_named(chain, asset)?;
	let (amount, min) = (tess(amount)?, tess(min)?);
	if min.0 == 0 || min.0 > amount.0 {
		return Err(format!("{spec}: the smallest sale is above zero and at most the amount"));
	}
	Ok(KeptBid { payment, price: units(price, payment.decimals())?, amount, min })
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
		if self.create_btc_key && !path.exists() {
			create_btc_key(path, self.btc_network.as_deref() == Some("bitcoin"))?;
		}
		let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
		BitcoinKey::from_wif(text.trim()).map_err(|_| format!("{}: not a Bitcoin private key in WIF", path.display()))
	}

	/// The Bitcoin network of the Tesserax network of `client`, through its Esplora API.
	async fn bitcoin(&self, client: &Client) -> Result<Esplora> {
		let network = match self.btc_network.as_deref() {
			Some("bitcoin") => btc::BitcoinNetwork::Bitcoin,
			Some("testnet4") => btc::BitcoinNetwork::Testnet4,
			Some("regtest") => btc::BitcoinNetwork::Regtest,
			Some(other) => return Err(format!("unknown Bitcoin network {other}: bitcoin, testnet4 or regtest")),
			None => btc::network_for(client.chain_info().await.map_err(error)?.chain != "Tesserax"),
		};
		Ok(Esplora::new(network, self.esplora.as_deref()))
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
		SwapCommand::Bids => {
			let bids = w.client().await?.bids().await.map_err(error)?;
			let json: Vec<_> = bids
				.iter()
				.map(|b| {
					serde_json::json!({
						"id": b.id,
						"buyer": format_address(&b.buyer),
						"chain": format!("{:?}", b.payment.chain).to_lowercase(),
						"asset": b.payment.symbol(),
						"price": decimal(b.price, b.payment.decimals()),
						"wanted": b.wanted,
						"min_sale": b.min_sale,
						"sales": b.sales,
						"completed": b.completed,
					})
				})
				.collect();
			w.print(json, || {
				if bids.is_empty() {
					"No bid.".into()
				} else {
					bids.iter()
						.map(|b| {
							format!(
								"#{} {} TESS wanted at {} · from {} TESS · {}/{} sales paid",
								b.id,
								b.wanted,
								price_text(b.payment, b.price),
								b.min_sale,
								b.completed,
								b.sales
							)
						})
						.collect::<Vec<_>>()
						.join("\n")
				}
			});
		},
		SwapCommand::Bid { account, chain, asset, price, amount, min } => {
			let payment = payment_named(&chain, &asset)?;
			let price = units(&price, payment.decimals())?;
			let (amount, min) = (tess(&amount)?, tess(&min)?);
			w.confirm(&format!(
				"Bid for {amount} TESS at {} from {account}; only a deposit is held, and `swap make` pays each sale",
				price_text(payment, price)
			))?;
			let key = w.key(&account)?;
			let id = w.client().await?.create_bid(&key, payment, price, amount, min).await.map_err(error)?;
			w.print(serde_json::json!({ "bid": id }), || {
				format!("Bid #{id} open. Run `tesserax-wallet swap make {account}` to pay its sales as they come.")
			});
		},
		SwapCommand::CancelBid { account, bid } => {
			w.confirm(&format!("Withdraw bid #{bid}"))?;
			let key = w.key(&account)?;
			let outcome = w.client().await?.cancel_bid(&key, bid).await.map_err(error)?;
			w.sent(outcome);
		},
		SwapCommand::Sell { account, bid, amount, payout, pay } => {
			let amount = tess(&amount)?;
			let client = w.client().await?.clone();
			let b = client.bid(bid).await.map_err(error)?.ok_or("no such bid")?;
			if amount.0 > b.wanted.0 {
				return Err(format!("the bid wants {} TESS", b.wanted));
			}
			if amount.0 < b.min_sale.0 && amount.0 != b.wanted.0 {
				return Err(format!("the bid buys at least {} TESS", b.min_sale));
			}
			let due = payment_due(amount.0, b.price).ok_or("amount too large")?;
			let payout = match (b.payment.chain, payout) {
				(Chain::Bitcoin, _) => {
					CompressedPublicKey::from_private_key(&Secp256k1::new(), &pay.btc_key()?).map_err(error)?.to_bytes().to_vec()
				},
				(_, Some(payout)) => match hex::decode(payout.trim().trim_start_matches("0x")) {
					Ok(address) if address.len() == 20 => address,
					_ => return Err("the payout is an EVM address: 0x and 40 digits".into()),
				},
				(_, None) => pay
					.key()?
					.ok_or("give the EVM key with --evm-key-file: it claims the payment, and pays its gas")?
					.address()
					.to_vec(),
			};
			w.confirm(&format!(
				"Sell {amount} TESS of {account} to bid #{bid} for {} {} on {:?}, paid to 0x{}; the TESS are locked for the buyer, and return to you in 24 hours if it does not pay",
				decimal(due, b.payment.decimals()),
				b.payment.symbol(),
				b.payment.chain,
				hex::encode(&payout),
			))?;
			let key = w.key(&account)?;
			let (secret, hash) = new_secret().map_err(error)?;
			// The secret is saved before the TESS are locked: without it, the payment cannot be claimed.
			save_secret(w, &secret)?;
			let sale = client.sell(&key, bid, amount, hash, payout).await.map_err(error)?;
			if !w.json {
				eprintln!("Sale #{sale} made: {amount} TESS locked for the buyer. Waiting for its payment…");
			}
			collect_sale(w, &client, sale, &pay).await?;
		},
		SwapCommand::CollectSale { sale, pay } => {
			let client = w.client().await?.clone();
			collect_sale(w, &client, sale, &pay).await?;
		},
		SwapCommand::RefundSale { account, sale } => {
			let key = w.key(&account)?;
			let client = w.client().await?.clone();
			let s = client.sale(sale).await.map_err(error)?.ok_or("no such sale: already claimed or refunded")?;
			let best = client.best_block_number().await.map_err(error)?;
			if best <= s.expires {
				let hours = (u64::from(s.expires - best) * 18).div_ceil(3600);
				return Err(format!("sale #{sale} can be taken back in about {hours} hour(s), once it expires"));
			}
			let outcome = client.refund_sale(&key, sale).await.map_err(error)?;
			let _ = std::fs::remove_file(secret_path(w, &s.hash));
			w.sent(outcome);
		},
		SwapCommand::Make { account, interval, keep_offers, keep_bids, pay } => {
			let keep = keep_offers.iter().map(|spec| kept_offer(spec, &pay)).collect::<Result<Vec<_>>>()?;
			let bids = keep_bids.iter().map(|spec| kept_bid(spec)).collect::<Result<Vec<_>>>()?;
			make(w, &account, interval, &keep, &bids, pay).await?
		},
	}
	Ok(())
}

/// Wait until the buyer of sale `id` locks and records its payment, check it, and claim it with the
/// secret saved for the hash of the sale; then claim the TESS for the buyer, which costs nothing.
async fn collect_sale(w: &mut Wallet, client: &Client, id: u64, pay: &PayArgs) -> Result<()> {
	let sale = loop {
		let Some(sale) = client.sale(id).await.map_err(error)? else {
			return Err(format!("sale #{id} is already claimed or refunded"));
		};
		if !sale.payment_lock.is_empty() {
			break sale;
		}
		let best = client.best_block_number().await.map_err(error)?;
		if best > sale.expires {
			return Err(format!("sale #{id} expired unpaid: take your TESS back with `swap refund-sale`"));
		}
		tokio::time::sleep(Duration::from_secs(10)).await;
	};
	let secret = load_secret(w, &sale.hash)?;
	let due = payment_due(sale.amount.0, sale.price).ok_or("amount too large")?;
	let claimed = if sale.payment.chain == Chain::Bitcoin {
		claim_bitcoin_sale(w, client, &sale, due, secret, pay).await?
	} else {
		let evm = pay.connect(client, sale.payment.chain, true).await?;
		let lock = B256::from_slice(&sale.payment_lock);
		let status = evm.lock_status(lock).await.map_err(error)?.ok_or("the payment recorded is not a lock")?;
		let (_, now) = evm.head().await.map_err(error)?;
		let paid = (evm.network.asset(status.token) == Some(sale.payment.asset))
			.then(|| u128::try_from(status.amount).ok())
			.flatten()
			.unwrap_or(0);
		// Any mismatch: the secret stays private, and the TESS return after the sale expires.
		let problem = if status.state != LockState::Locked {
			Some(format!("the payment is already {:?}", status.state))
		} else if status.hash != sale.hash {
			Some("the payment is locked under another hash".into())
		} else if status.recipient.as_slice() != sale.payout.as_slice() {
			Some(format!("the payment goes to {}, not to you", status.recipient))
		} else if paid < due {
			Some(format!(
				"the payment holds {} {} instead of {}",
				decimal(paid, sale.payment.decimals()),
				sale.payment.symbol(),
				decimal(due, sale.payment.decimals())
			))
		} else if status.timelock < now + MIN_CLAIM_SECONDS {
			Some("the payment expires in less than an hour".into())
		} else {
			None
		};
		if let Some(problem) = problem {
			return Err(format!("{problem}: not claimed. The TESS return to you once sale #{id} expires (`swap refund-sale`)"));
		}
		let tx = evm.claim(lock, secret).await.map_err(error)?;
		if !w.json {
			eprintln!("Payment of {} {} claimed in {tx}", decimal(paid, sale.payment.decimals()), sale.payment.symbol());
		}
		tx.to_string()
	};
	// The secret is public now: deliver the TESS to the buyer.
	if let Err(e) = client.claim_sale(id, secret).await {
		eprintln!("The buyer claims its TESS itself: {e}");
	}
	let _ = std::fs::remove_file(secret_path(w, &sale.hash));
	w.print(serde_json::json!({ "sale": id, "payment": claimed }), || format!("Sale #{id} done: payment claimed in {claimed}"));
	Ok(())
}

/// Claim the Bitcoin payment of `sale`, once confirmed, with `secret`, to the address of the key.
async fn claim_bitcoin_sale(
	w: &mut Wallet,
	client: &Client,
	sale: &Sale,
	due: u128,
	secret: [u8; 32],
	pay: &PayArgs,
) -> Result<String> {
	let key = pay.btc_key()?;
	let own = CompressedPublicKey::from_private_key(&Secp256k1::new(), &key).map_err(error)?;
	if own.to_bytes().as_slice() != sale.payout.as_slice() {
		return Err("the sale is paid to another Bitcoin key than that of --btc-key-file".into());
	}
	let outpoint = btc::outpoint_of(&sale.payment_lock).ok_or("the payment recorded is not a Bitcoin output")?;
	let esplora = pay.bitcoin(client).await?;
	// An unconfirmed payment can be replaced: the secret is revealed only once it is buried.
	loop {
		let confirmations = esplora.confirmations(&outpoint.txid).await.map_err(error)?;
		if confirmations >= btc::MIN_CONFIRMATIONS {
			break;
		}
		if !w.json {
			eprintln!("Payment {outpoint}: {confirmations}/{} confirmations…", btc::MIN_CONFIRMATIONS);
		}
		tokio::time::sleep(Duration::from_secs(60)).await;
	}
	let tx = esplora.transaction(&outpoint.txid).await.map_err(error)?;
	let lock = btc::find_lock(&tx, &own)
		.filter(|l| l.outpoint == outpoint)
		.ok_or("the payment recorded is not locked for your key")?;
	let now = esplora.median_time().await.map_err(error)?;
	let problem = if lock.htlc.hash != sale.hash {
		Some("the payment is locked under another hash".to_string())
	} else if u128::from(lock.amount) < due {
		Some(format!("the payment holds {} BTC instead of {}", decimal(u128::from(lock.amount), 8), decimal(due, 8)))
	} else if u64::from(lock.htlc.timelock) < now + MIN_CLAIM_SECONDS {
		Some("the payment expires in less than an hour".into())
	} else if esplora.is_spent(&outpoint).await.map_err(error)? {
		Some("the payment is already spent".into())
	} else {
		None
	};
	if let Some(problem) = problem {
		return Err(format!("{problem}: not claimed. The TESS return to you once sale #{} expires (`swap refund-sale`)", sale.id));
	}
	let fee_rate = esplora.fee_rate(2).await.map_err(error)?;
	let claim = btc::claim_transaction(&lock, secret, &key, fee_rate, esplora.network).map_err(error)?;
	let txid = esplora.broadcast(&claim).await.map_err(error)?;
	if !w.json {
		eprintln!("Payment of {} BTC claimed in {txid}", decimal(u128::from(lock.amount), 8));
	}
	Ok(txid.to_string())
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
	/// Sales into the bids of the maker it paid: their payment, until the TESS are claimed and the
	/// payment claimed, or the payment taken back.
	#[serde(default)]
	paid_sales: BTreeMap<u64, PaidSale>,
}

/// The payment of a sale, locked by the maker for its seller.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PaidSale {
	/// Bitcoin, Ethereum, Base or Arbitrum.
	chain: String,
	/// 0x and the lock id on an EVM chain, txid:vout on Bitcoin.
	lock: String,
	/// The EVM block it was locked at, from which to look for the secret of its claim.
	block: u64,
	/// Unix time from which it can be taken back.
	timelock: u64,
	hash: String,
	/// Recorded on Tesserax with `pay`.
	recorded: bool,
	/// The sale is over on Tesserax: only the payment is left to watch.
	done: bool,
}

fn maker_state_path(w: &Wallet, account: &str) -> PathBuf {
	w.dir.join("swaps").join(format!("maker-{account}.json"))
}

async fn make(
	w: &mut Wallet,
	account: &str,
	interval: u64,
	keep: &[KeptOffer],
	keep_bids: &[KeptBid],
	args: PayArgs,
) -> Result<()> {
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
	eprintln!(
		"Maker of {} running: filling offers, collecting payments, refunding expired trades, paying the sales of its bids.",
		format_address(&maker)
	);
	loop {
		if let Err(e) = keep_offers(&client, &key, &maker, keep).await {
			eprintln!("Keeping the offers open: {e}");
		}
		if let Err(e) = keep_open_bids(&client, &key, &maker, keep_bids).await {
			eprintln!("Keeping the bids open: {e}");
		}
		if let Err(e) = round(&client, &key, &maker, &args, &mut chains, &mut state).await {
			eprintln!("{e}");
		}
		if let Err(e) = pay_sales(w, &client, &key, &maker, &args, &mut chains, &mut state).await {
			eprintln!("Sales: {e}");
		}
		std::fs::create_dir_all(path.parent().expect("a directory; qed")).map_err(error)?;
		std::fs::write(&path, serde_json::to_vec(&state).map_err(error)?).map_err(error)?;
		tokio::time::sleep(Duration::from_secs(interval)).await;
	}
}

/// Offer again what `keep` asks for when the open offers of `maker` for its payment run low.
async fn keep_offers(client: &Client, key: &tesserax_sdk::Keypair, maker: &AccountId, keep: &[KeptOffer]) -> Result<()> {
	if keep.is_empty() {
		return Ok(());
	}
	let offers = client.offers().await.map_err(error)?;
	for kept in keep {
		let open: u128 = offers
			.iter()
			.filter(|o| o.maker == *maker && o.payment == kept.payment && o.payout == kept.payout)
			.map(|o| o.available.0)
			.sum();
		if open >= kept.min.0 {
			continue;
		}
		let transferable = client.balance(maker).await.map_err(error)?.transferable;
		if transferable.0 <= kept.amount.0 {
			eprintln!(
				"Offering {} TESS at {}: the account has {transferable} TESS, not enough with the deposit and the fee",
				kept.amount,
				price_text(kept.payment, kept.price)
			);
			continue;
		}
		let id = client
			.create_offer(key, kept.payment, kept.price, kept.amount, kept.min, kept.payout.clone())
			.await
			.map_err(error)?;
		eprintln!("Offer #{id} open: {} TESS at {}", kept.amount, price_text(kept.payment, kept.price));
	}
	Ok(())
}

/// Bid again what `keep` asks for when the open bids of `buyer` for its payment want too little.
async fn keep_open_bids(client: &Client, key: &tesserax_sdk::Keypair, buyer: &AccountId, keep: &[KeptBid]) -> Result<()> {
	if keep.is_empty() {
		return Ok(());
	}
	let bids = client.bids().await.map_err(error)?;
	for kept in keep {
		let open: u128 =
			bids.iter().filter(|b| b.buyer == *buyer && b.payment == kept.payment).map(|b| b.wanted.0).sum();
		if open >= kept.min.0 {
			continue;
		}
		let id = client.create_bid(key, kept.payment, kept.price, kept.amount, kept.min).await.map_err(error)?;
		eprintln!("Bid #{id} open: {} TESS wanted at {}", kept.amount, price_text(kept.payment, kept.price));
	}
	Ok(())
}

/// The EVM chain `chain`, connected with the key of `args` the first time.
async fn evm_chain<'a>(client: &Client, args: &PayArgs, chains: &'a mut BTreeMap<String, Evm>, chain: Chain) -> Result<&'a Evm> {
	let name = format!("{chain:?}");
	if !chains.contains_key(&name) {
		let evm = args.connect(client, chain, true).await?;
		chains.insert(name.clone(), evm);
	}
	Ok(&chains[&name])
}

/// Pay the sales into the bids of `buyer`, claim their TESS once their sellers claimed the
/// payments, and take back the payments never claimed.
async fn pay_sales(
	w: &mut Wallet,
	client: &Client,
	key: &tesserax_sdk::Keypair,
	buyer: &AccountId,
	args: &PayArgs,
	chains: &mut BTreeMap<String, Evm>,
	state: &mut MakerState,
) -> Result<()> {
	let best = client.best_block_number().await.map_err(error)?;
	let sales: Vec<_> = client.sales().await.map_err(error)?.into_iter().filter(|s| s.buyer == *buyer).collect();

	// New sales: paid while they last long enough for the payment and the claim after it.
	for sale in &sales {
		if state.paid_sales.contains_key(&sale.id) || !sale.payment_lock.is_empty() || sale.expires < best + MIN_SALE_BLOCKS {
			continue;
		}
		let Some(due) = payment_due(sale.amount.0, sale.price) else { continue };
		let paid = if sale.payment.chain == Chain::Bitcoin {
			lock_bitcoin_sale(w, client, args, sale, due).await
		} else {
			match evm_chain(client, args, chains, sale.payment.chain).await {
				Ok(evm) => lock_evm_sale(evm, sale, due).await,
				Err(e) => Err(e),
			}
		};
		match paid {
			Ok(paid) => {
				eprintln!(
					"Sale #{}: {} TESS paid {} {} on {:?}, lock {}",
					sale.id,
					sale.amount,
					decimal(due, sale.payment.decimals()),
					sale.payment.symbol(),
					sale.payment.chain,
					paid.lock
				);
				state.paid_sales.insert(sale.id, paid);
			},
			Err(e) => eprintln!("Paying sale #{}: {e}", sale.id),
		}
	}

	// Recorded on Tesserax, so that the seller finds the payment: retried until it is.
	for (id, paid) in state.paid_sales.iter_mut().filter(|(_, p)| !p.recorded && !p.done) {
		let (_, lock) = lock_named(&paid.lock)?;
		match client.pay_sale(key, *id, lock).await {
			Ok(_) => paid.recorded = true,
			Err(e) => eprintln!("Recording the payment of sale #{id}, again next round: {e}"),
		}
	}

	// A sale gone from Tesserax is claimed or refunded; the others claim their TESS with the secret
	// the seller revealed on the other chain, and a payment never claimed is taken back.
	let open: std::collections::BTreeSet<u64> = sales.iter().map(|s| s.id).collect();
	for (id, paid) in state.paid_sales.iter_mut() {
		if !open.contains(id) {
			paid.done = true;
		}
	}
	for (id, paid) in state.paid_sales.clone() {
		let hash = hex32(&paid.hash)?;
		let outcome = if paid.chain == "Bitcoin" {
			settle_bitcoin_sale(w, client, args, &paid, &hash).await
		} else {
			let Some(chain) =
				[Chain::Ethereum, Chain::Base, Chain::Arbitrum].into_iter().find(|c| format!("{c:?}") == paid.chain)
			else {
				continue;
			};
			match evm_chain(client, args, chains, chain).await {
				Ok(evm) => settle_evm_sale(evm, &paid).await,
				Err(e) => Err(e),
			}
		};
		match outcome {
			Ok(Settled::Waiting) => {},
			Ok(Settled::Secret(secret)) => match client.claim_sale(id, secret).await {
				Ok(_) => {
					eprintln!("Sale #{id}: TESS claimed");
					state.paid_sales.remove(&id);
				},
				// The seller, or anyone, already submitted the same claim: it lands without us.
				Err(e) if e.to_string().contains("Already Imported") => {},
				Err(e) => eprintln!("Claiming sale #{id}, again next round: {e}"),
			},
			Ok(Settled::Refunded(tx)) => {
				eprintln!("Sale #{id}: the seller never claimed the payment, taken back in {tx}");
				state.paid_sales.remove(&id);
			},
			Ok(Settled::Over) => {
				state.paid_sales.remove(&id);
			},
			Err(e) => eprintln!("Sale #{id}: {e}"),
		}
	}
	Ok(())
}

/// Where the payment of a sale stands.
enum Settled {
	/// Locked, neither claimed nor expired; or claimed, with the TESS already received.
	Waiting,
	/// Claimed by the seller, which revealed the secret, and the TESS still to claim.
	Secret([u8; 32]),
	/// Taken back by the maker, in this transaction.
	Refunded(String),
	/// Nothing left to do.
	Over,
}

/// Lock the payment `due` of `sale` on its EVM chain, for six hours.
async fn lock_evm_sale(evm: &Evm, sale: &Sale, due: u128) -> Result<PaidSale> {
	let (head, now) = evm.head().await.map_err(error)?;
	let timelock = now + SALE_PAYMENT_SECONDS;
	let recipient = EvmAddress::from_slice(&sale.payout);
	let lock = evm
		.lock_payment(sale.payment.asset, recipient, U256::from(due), sale.hash, timelock, sale.id, &sale.buyer)
		.await
		.map_err(error)?;
	Ok(PaidSale {
		chain: format!("{:?}", sale.payment.chain),
		lock: lock.to_string(),
		block: head,
		timelock,
		hash: hex::encode(sale.hash),
		recorded: false,
		done: false,
	})
}

/// Where the EVM payment `paid` stands: its secret once claimed, taken back once expired.
async fn settle_evm_sale(evm: &Evm, paid: &PaidSale) -> Result<Settled> {
	let id = B256::from(hex32(&paid.lock)?);
	let status = evm.lock_status(id).await.map_err(error)?.ok_or("the lock is gone")?;
	match status.state {
		LockState::Claimed if paid.done => Ok(Settled::Over),
		LockState::Claimed => {
			let secret =
				evm.secret_of(id, paid.block).await.map_err(error)?.ok_or("claimed, but its secret is not found yet")?;
			Ok(Settled::Secret(secret))
		},
		LockState::Refunded => Ok(Settled::Over),
		LockState::Locked => {
			let (_, now) = evm.head().await.map_err(error)?;
			if now < status.timelock {
				return Ok(Settled::Waiting);
			}
			Ok(Settled::Refunded(evm.refund(id).await.map_err(error)?.to_string()))
		},
	}
}

/// Lock the payment `due` of `sale` in satoshis from the Bitcoin key, for six hours, saving first
/// what takes it back.
async fn lock_bitcoin_sale(w: &mut Wallet, client: &Client, args: &PayArgs, sale: &Sale, due: u128) -> Result<PaidSale> {
	let key = args.btc_key()?;
	let esplora = args.bitcoin(client).await?;
	let refund = CompressedPublicKey::from_private_key(&Secp256k1::new(), &key).map_err(error)?;
	let seller = CompressedPublicKey::from_slice(&sale.payout).map_err(|_| "the payout of this sale is not a Bitcoin key")?;
	let due = u64::try_from(due).map_err(|_| "amount too large")?;
	let from = btc::address_of(&refund, esplora.network);
	let utxos = esplora.utxos(&from).await.map_err(error)?;
	if utxos.is_empty() {
		return Err(format!("{from} has no bitcoin to pay with"));
	}
	let fee_rate = esplora.fee_rate(2).await.map_err(error)?;
	let now = esplora.median_time().await.map_err(error)?;
	let timelock = u32::try_from(now + SALE_PAYMENT_SECONDS).map_err(error)?;
	let htlc = Htlc { hash: sale.hash, maker: seller, taker: refund, timelock };
	let announcement = btc::Announcement { offer: sale.id, taker: sale.buyer.clone(), hash: sale.hash, timelock, refund: None };
	let tx = btc::lock_transaction(&key, &utxos, &htlc, &announcement, due, fee_rate, esplora.network).map_err(error)?;
	let lock = btc::find_lock(&tx, &seller).ok_or("the payment built cannot be found again")?;
	let saved = SavedBitcoinLock {
		hash: hex::encode(sale.hash),
		maker: hex::encode(seller.to_bytes()),
		taker: hex::encode(refund.to_bytes()),
		timelock,
		amount: due,
		offer: sale.id,
		account: format_address(&sale.buyer),
		outpoint: Some(lock.outpoint.to_string()),
	};
	save_json(&bitcoin_lock_path(w, &sale.hash), &saved)?;
	esplora.broadcast(&tx).await.map_err(error)?;
	Ok(PaidSale {
		chain: "Bitcoin".into(),
		lock: lock.outpoint.to_string(),
		block: 0,
		timelock: u64::from(timelock),
		hash: hex::encode(sale.hash),
		recorded: false,
		done: false,
	})
}

/// Where the Bitcoin payment `paid` stands: its secret once claimed, taken back once expired.
async fn settle_bitcoin_sale(w: &Wallet, client: &Client, args: &PayArgs, paid: &PaidSale, hash: &[u8; 32]) -> Result<Settled> {
	let outpoint: OutPoint = paid.lock.parse().map_err(error)?;
	let esplora = args.bitcoin(client).await?;
	if let Some(spender) = esplora.spender(&outpoint).await.map_err(error)? {
		let _ = std::fs::remove_file(bitcoin_lock_path(w, hash));
		return Ok(match btc::secret_in_claim(&spender, &outpoint, hash) {
			Some(_) if paid.done => Settled::Over,
			Some(secret) => Settled::Secret(secret),
			// Spent without the secret: taken back.
			None => Settled::Over,
		});
	}
	let now = esplora.median_time().await.map_err(error)?;
	if now <= paid.timelock {
		return Ok(Settled::Waiting);
	}
	let path = bitcoin_lock_path(w, hash);
	let saved: SavedBitcoinLock =
		serde_json::from_slice(&std::fs::read(&path).map_err(|_| "the script of this payment is not saved")?).map_err(error)?;
	let lock = saved.lock()?;
	let fee_rate = esplora.fee_rate(6).await.map_err(error)?;
	let tx = btc::refund_transaction(&lock, &args.btc_key()?, fee_rate, esplora.network).map_err(error)?;
	let txid = esplora.broadcast(&tx).await.map_err(error)?;
	let _ = std::fs::remove_file(path);
	Ok(Settled::Refunded(txid.to_string()))
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
