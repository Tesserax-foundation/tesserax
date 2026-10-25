//! Atomic swaps: offers of TESS, buying them with a payment locked on an EVM chain, and the maker
//! that fills offers and collects their payments (`docs/swaps.md`).

use super::{error, tess, Result, Wallet};
use clap::Subcommand;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tesserax_sdk::{
	format_address,
	swaps::{
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
	/// Offer TESS of an account for a payment on an EVM chain.
	Offer {
		account: String,
		/// ethereum, base or arbitrum.
		#[arg(long)]
		chain: String,
		/// eth, usdc or usdt.
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
		/// EVM address the payments go to.
		#[arg(long)]
		payout: String,
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
		evm: EvmArgs,
	},
	/// Claim the TESS of a payment already locked, once the maker filled it.
	Claim {
		account: String,
		/// The id of the payment lock, 0x...
		lock: String,
		#[command(flatten)]
		evm: EvmArgs,
	},
	/// Take back a payment the maker never filled, once its lock expired.
	RefundPayment {
		/// The id of the payment lock, 0x...
		lock: String,
		#[command(flatten)]
		evm: EvmArgs,
	},
	/// Run the maker of an account: fill its offers for the payments locked for them, collect the
	/// payments once their secrets are revealed, and take back trades that expired.
	Make {
		account: String,
		/// Seconds between two rounds.
		#[arg(long, default_value_t = 20)]
		interval: u64,
		#[command(flatten)]
		evm: EvmArgs,
	},
}

#[derive(clap::Args)]
pub struct EvmArgs {
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
}

fn chain_named(name: &str) -> Result<Chain> {
	match name.to_lowercase().as_str() {
		"ethereum" => Ok(Chain::Ethereum),
		"base" => Ok(Chain::Base),
		"arbitrum" => Ok(Chain::Arbitrum),
		"bitcoin" => Err("Bitcoin offers come with the Bitcoin support of swaps".into()),
		other => Err(format!("unknown chain {other}: ethereum, base or arbitrum")),
	}
}

fn asset_named(name: &str) -> Result<Asset> {
	match name.to_lowercase().as_str() {
		"eth" => Ok(Asset::Native),
		"usdc" => Ok(Asset::Usdc),
		"usdt" => Ok(Asset::Usdt),
		other => Err(format!("unknown asset {other}: eth, usdc or usdt")),
	}
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

impl EvmArgs {
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
		SwapCommand::Offer { account, chain, asset, price, amount, min, payout } => {
			let payment = Payment { chain: chain_named(&chain)?, asset: asset_named(&asset)? };
			let price = units(&price, payment.decimals())?;
			let (amount, min) = (tess(&amount)?, tess(&min)?);
			let payout = hex::decode(payout.trim().trim_start_matches("0x")).map_err(|_| "the payout is an EVM address, 0x...")?;
			if payout.len() != 20 {
				return Err("the payout is an EVM address: 0x and 40 digits".into());
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
		SwapCommand::Buy { account, offer, amount, evm } => {
			let taker = w.resolve(&account)?;
			let amount = tess(&amount)?;
			let client = w.client().await?.clone();
			let o = client.offer(offer).await.map_err(error)?.ok_or("no such offer")?;
			if o.payment.chain == Chain::Bitcoin {
				return Err("Bitcoin offers come with the Bitcoin support of swaps".into());
			}
			if amount.0 > o.available.0 {
				return Err(format!("the offer has {} TESS", o.available));
			}
			if amount.0 < o.min_fill.0 && amount.0 != o.available.0 {
				return Err(format!("the offer sells at least {} TESS", o.min_fill));
			}
			let due = payment_due(amount.0, o.price).ok_or("amount too large")?;
			let chain = evm.connect(&client, o.payment.chain, true).await?;
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
			claim_when_filled(w, &client, o.payment.chain, lock.0, &taker, Some(amount)).await?;
		},
		SwapCommand::Claim { account, lock, evm: _ } => {
			let taker = w.resolve(&account)?;
			let lock = hex32(&lock)?;
			let client = w.client().await?.clone();
			let chain = [Chain::Ethereum, Chain::Base, Chain::Arbitrum];
			let mut found = None;
			for c in chain {
				if client.trade_of_lock(c, &lock).await.map_err(error)?.is_some() {
					found = Some(c);
				}
			}
			let chain = found.ok_or("the maker has not filled this payment yet")?;
			claim_when_filled(w, &client, chain, lock, &taker, None).await?;
		},
		SwapCommand::RefundPayment { lock, evm } => {
			let lock = B256::from(hex32(&lock)?);
			let client = w.client().await?.clone();
			let mut last = String::from("no lock with this id on the EVM chains");
			for chain in [Chain::Base, Chain::Arbitrum, Chain::Ethereum] {
				let Ok(evm_chain) = evm.connect(&client, chain, true).await else { continue };
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
		SwapCommand::Make { account, interval, evm } => make(w, &account, interval, evm).await?,
	}
	Ok(())
}

/// Wait until the maker fills the payment lock `lock`, check the trade, and claim it with the
/// secret saved for its hash.
async fn claim_when_filled(
	w: &mut Wallet,
	client: &Client,
	chain: Chain,
	lock: [u8; 32],
	taker: &AccountId,
	expected: Option<Amount>,
) -> Result<()> {
	let trade = loop {
		if let Some(id) = client.trade_of_lock(chain, &lock).await.map_err(error)? {
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
}

fn maker_state_path(w: &Wallet, account: &str) -> PathBuf {
	w.dir.join("swaps").join(format!("maker-{account}.json"))
}

async fn make(w: &mut Wallet, account: &str, interval: u64, args: EvmArgs) -> Result<()> {
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
	args: &EvmArgs,
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
			chains.insert(name.clone(), args.connect(client, chain, true).await?);
		}
		let evm = &chains[&name];
		let (head, now) = evm.head().await.map_err(error)?;
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
		let Some(evm) = chains.get(&chain) else { continue };
		match evm.claim(B256::from(hex32(&lock)?), hex32(&secret)?).await {
			Ok(tx) => {
				state.collect.remove(&trade);
				eprintln!("Collected the payment of trade #{trade} in {tx}");
			},
			Err(e) => eprintln!("Collecting the payment of trade #{trade}, again next round: {e} (secret {secret})"),
		}
	}
	Ok(())
}
