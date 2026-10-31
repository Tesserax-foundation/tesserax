//! Atomic swaps of TESS against BTC, ETH, USDC and USDT (`docs/swaps.md`).
//!
//! A maker offers TESS; a taker locks its payment on the other chain under the SHA-256 hash of a
//! secret; the maker checks it and fills the offer, which locks the TESS for the taker under the
//! same hash; the taker claims them with the secret, without fee, and the secret pays the maker on
//! the other chain.
//!
//! A buyer bids for TESS; a seller locks TESS for it under the hash of its own secret; the buyer
//! locks the payment for the seller under the same hash and records it; the seller claims the
//! payment with the secret, which then claims the TESS for the buyer, without fee.
//!
//! This module is the Tesserax side; with the `swaps` feature, [`evm`] is the side of the EVM
//! chains and [`bitcoin`] the side of Bitcoin.

#[cfg(feature = "swaps")]
pub mod bitcoin;
#[cfg(feature = "swaps")]
pub mod evm;

use crate::{
	client::TxOutcome,
	runtime::{self, runtime_types::pallet_swaps as rt},
	AccountId, Amount, Client, Error, Keypair, Result,
};
use subxt::ext::codec::{Compact, Encode};

/// Planck in one TESS: prices are for one TESS.
pub const PLANCK_PER_TESS: u128 = 1_000_000_000_000;

/// The chain a payment is made on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Chain {
	Bitcoin,
	Ethereum,
	Base,
	Arbitrum,
}

/// The asset of a payment: BTC on Bitcoin, ETH on the other chains, or a stablecoin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Asset {
	Native,
	Usdc,
	Usdt,
}

/// What an offer is paid in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Payment {
	pub chain: Chain,
	pub asset: Asset,
}

impl Payment {
	/// Decimals of the asset: prices count in its smallest unit.
	pub fn decimals(&self) -> u32 {
		match (self.chain, self.asset) {
			(Chain::Bitcoin, Asset::Native) => 8,
			(_, Asset::Native) => 18,
			_ => 6,
		}
	}

	/// Symbol of the asset.
	pub fn symbol(&self) -> &'static str {
		match (self.chain, self.asset) {
			(Chain::Bitcoin, _) => "BTC",
			(_, Asset::Native) => "ETH",
			(_, Asset::Usdc) => "USDC",
			(_, Asset::Usdt) => "USDT",
		}
	}

	fn to_runtime(self) -> rt::Payment {
		rt::Payment {
			chain: match self.chain {
				Chain::Bitcoin => rt::Chain::Bitcoin,
				Chain::Ethereum => rt::Chain::Ethereum,
				Chain::Base => rt::Chain::Base,
				Chain::Arbitrum => rt::Chain::Arbitrum,
			},
			asset: match self.asset {
				Asset::Native => rt::Asset::Native,
				Asset::Usdc => rt::Asset::Usdc,
				Asset::Usdt => rt::Asset::Usdt,
			},
		}
	}

	fn from_runtime(payment: &rt::Payment) -> Self {
		Payment {
			chain: match payment.chain {
				rt::Chain::Bitcoin => Chain::Bitcoin,
				rt::Chain::Ethereum => Chain::Ethereum,
				rt::Chain::Base => Chain::Base,
				rt::Chain::Arbitrum => Chain::Arbitrum,
			},
			asset: match payment.asset {
				rt::Asset::Native => Asset::Native,
				rt::Asset::Usdc => Asset::Usdc,
				rt::Asset::Usdt => Asset::Usdt,
			},
		}
	}

	fn chain_to_runtime(chain: Chain) -> rt::Chain {
		Payment { chain, asset: Asset::Native }.to_runtime().chain
	}
}

/// The payment, in the smallest unit of its asset, of `amount` planck at `price` units per TESS,
/// rounded up, as the chain computes it.
pub fn payment_due(amount: u128, price: u128) -> Option<u128> {
	let product = amount.checked_mul(price)?;
	Some(product / PLANCK_PER_TESS + u128::from(product % PLANCK_PER_TESS != 0))
}

/// The TESS, in planck, a payment of `payment` units buys at `price`: the most whose payment due
/// is at most `payment`.
pub fn tess_for(payment: u128, price: u128) -> u128 {
	if price == 0 {
		return 0;
	}
	payment.saturating_mul(PLANCK_PER_TESS) / price
}

/// A new secret and its SHA-256 hash.
pub fn new_secret() -> Result<([u8; 32], [u8; 32])> {
	let mut secret = [0u8; 32];
	getrandom::fill(&mut secret).map_err(|e| Error::InvalidInput(format!("no randomness: {e}")))?;
	Ok((secret, sp_crypto_hashing::sha2_256(&secret)))
}

/// SHA-256 of a secret.
pub fn hash_of(secret: &[u8; 32]) -> [u8; 32] {
	sp_crypto_hashing::sha2_256(secret)
}

/// An offer of TESS.
#[derive(Clone, Debug)]
pub struct Offer {
	pub id: u64,
	pub maker: AccountId,
	pub payment: Payment,
	/// Smallest units of the asset for one TESS.
	pub price: u128,
	pub available: Amount,
	pub min_fill: Amount,
	/// The EVM address the maker is paid to, or its public key on Bitcoin.
	pub payout: Vec<u8>,
	pub fills: u32,
	pub completed: u32,
}

/// TESS locked for a taker.
#[derive(Clone, Debug)]
pub struct Trade {
	pub id: u64,
	pub offer: u64,
	pub maker: AccountId,
	pub taker: AccountId,
	pub amount: Amount,
	pub price: u128,
	pub payment: Payment,
	pub hash: [u8; 32],
	/// The lock of the payment on its chain.
	pub payment_lock: Vec<u8>,
	/// Last block the taker may claim at.
	pub expires: u32,
}

/// A bid for TESS.
#[derive(Clone, Debug)]
pub struct Bid {
	pub id: u64,
	pub buyer: AccountId,
	pub payment: Payment,
	/// Smallest units of the asset for one TESS.
	pub price: u128,
	/// TESS still wanted.
	pub wanted: Amount,
	pub min_sale: Amount,
	/// Sales made into the bid, and sales the buyer paid and received.
	pub sales: u32,
	pub completed: u32,
}

/// TESS a seller locked for the buyer of a bid.
#[derive(Clone, Debug)]
pub struct Sale {
	pub id: u64,
	pub bid: u64,
	pub seller: AccountId,
	pub buyer: AccountId,
	pub amount: Amount,
	pub price: u128,
	pub payment: Payment,
	pub hash: [u8; 32],
	/// The EVM address the seller is paid to, or its public key on Bitcoin.
	pub payout: Vec<u8>,
	/// The lock of the payment on its chain, empty until the buyer records it.
	pub payment_lock: Vec<u8>,
	/// Last block the buyer may claim at.
	pub expires: u32,
}

/// What happened to trades and sales in a block.
#[derive(Clone, Debug)]
pub enum SwapEvent {
	Filled(Trade),
	/// Claimed with `secret`, which releases the payment.
	Claimed { trade: u64, secret: [u8; 32] },
	Refunded { trade: u64 },
	Sold(Sale),
	SalePaid { sale: u64, payment_lock: Vec<u8> },
	/// Claimed for the buyer with `secret`, which the seller revealed by claiming its payment.
	SaleClaimed { sale: u64, secret: [u8; 32] },
	SaleRefunded { sale: u64 },
}

fn offer_of(id: u64, o: rt::Offer<AccountId, u128, u32>) -> Offer {
	Offer {
		id,
		maker: o.maker,
		payment: Payment::from_runtime(&o.payment),
		price: o.price,
		available: Amount(o.available),
		min_fill: Amount(o.min_fill),
		payout: o.payout.0,
		fills: o.fills,
		completed: o.completed,
	}
}

fn trade_of(id: u64, t: rt::Trade<AccountId, u128, u32>) -> Trade {
	Trade {
		id,
		offer: t.offer,
		maker: t.maker,
		taker: t.taker,
		amount: Amount(t.amount),
		price: t.price,
		payment: Payment::from_runtime(&t.payment),
		hash: t.hash,
		payment_lock: t.payment_lock.0,
		expires: t.expires,
	}
}

fn bid_of(id: u64, b: rt::Bid<AccountId, u128, u32>) -> Bid {
	Bid {
		id,
		buyer: b.buyer,
		payment: Payment::from_runtime(&b.payment),
		price: b.price,
		wanted: Amount(b.wanted),
		min_sale: Amount(b.min_sale),
		sales: b.sales,
		completed: b.completed,
	}
}

fn sale_of(id: u64, s: rt::Sale<AccountId, u128, u32>) -> Sale {
	Sale {
		id,
		bid: s.bid,
		seller: s.seller,
		buyer: s.buyer,
		amount: Amount(s.amount),
		price: s.price,
		payment: Payment::from_runtime(&s.payment),
		hash: s.hash,
		payout: s.payout.0,
		payment_lock: s.payment_lock.0,
		expires: s.expires,
	}
}

/// The id ending a key of a map hashed with Blake2_128Concat.
fn id_of(key: &[u8]) -> Result<u64> {
	let tail = key.get(key.len().saturating_sub(8)..).ok_or_else(|| Error::NotFound("malformed key".into()))?;
	Ok(u64::from_le_bytes(tail.try_into().map_err(|_| Error::NotFound("malformed key".into()))?))
}

fn bounded(bytes: Vec<u8>) -> Result<runtime::runtime_types::bounded_collections::bounded_vec::BoundedVec<u8>> {
	if bytes.len() > 64 {
		return Err(Error::InvalidInput("at most 64 bytes".into()));
	}
	Ok(runtime::runtime_types::bounded_collections::bounded_vec::BoundedVec(bytes))
}

impl Client {
	/// Open offers.
	pub async fn offers(&self) -> Result<Vec<Offer>> {
		let storage = self.storage().await?;
		let mut entries = storage.iter(runtime::storage().swaps().offers_iter()).await?;
		let mut found = Vec::new();
		while let Some(entry) = entries.next().await {
			let entry = entry?;
			found.push(offer_of(id_of(&entry.key_bytes)?, entry.value));
		}
		found.sort_by_key(|o| o.id);
		Ok(found)
	}

	/// The offer `id`, if open.
	pub async fn offer(&self, id: u64) -> Result<Option<Offer>> {
		let query = runtime::storage().swaps().offers(id);
		Ok(self.storage().await?.fetch(&query).await?.map(|o| offer_of(id, o)))
	}

	/// Trades waiting for their claim or their refund.
	pub async fn trades(&self) -> Result<Vec<Trade>> {
		let storage = self.storage().await?;
		let mut entries = storage.iter(runtime::storage().swaps().trades_iter()).await?;
		let mut found = Vec::new();
		while let Some(entry) = entries.next().await {
			let entry = entry?;
			found.push(trade_of(id_of(&entry.key_bytes)?, entry.value));
		}
		found.sort_by_key(|t| t.id);
		Ok(found)
	}

	/// The trade `id`, if not claimed nor refunded yet.
	pub async fn trade(&self, id: u64) -> Result<Option<Trade>> {
		let query = runtime::storage().swaps().trades(id);
		Ok(self.storage().await?.fetch(&query).await?.map(|t| trade_of(id, t)))
	}

	/// The trade the payment lock `lock` on `chain` filled, if any.
	pub async fn trade_of_lock(&self, chain: Chain, lock: &[u8]) -> Result<Option<u64>> {
		let query = runtime::storage().swaps().filled_locks(Payment::chain_to_runtime(chain), bounded(lock.to_vec())?);
		Ok(self.storage().await?.fetch(&query).await?)
	}

	/// Offer `amount` TESS at `price` units of `payment` for one TESS, paid to `payout`, in
	/// trades of at least `min_fill`. Returns the id of the offer.
	pub async fn create_offer(
		&self,
		maker: &Keypair,
		payment: Payment,
		price: u128,
		amount: Amount,
		min_fill: Amount,
		payout: Vec<u8>,
	) -> Result<u64> {
		let call = runtime::tx().swaps().offer(payment.to_runtime(), price, amount.0, min_fill.0, bounded(payout)?);
		let (_, events) = self.submit_events(&call, maker).await?;
		Ok(events
			.find_first::<runtime::swaps::events::Offered>()?
			.ok_or_else(|| Error::NotFound("the offer created".into()))?
			.offer)
	}

	/// Change the price and the smallest fill of offer `id`, adding `add` TESS and withdrawing
	/// `remove`.
	pub async fn update_offer(
		&self,
		maker: &Keypair,
		id: u64,
		price: u128,
		min_fill: Amount,
		add: Amount,
		remove: Amount,
	) -> Result<TxOutcome> {
		let call = runtime::tx().swaps().update_offer(id, price, min_fill.0, add.0, remove.0);
		self.submit(&call, maker).await
	}

	/// Withdraw offer `id`.
	pub async fn cancel_offer(&self, maker: &Keypair, id: u64) -> Result<TxOutcome> {
		self.submit(&runtime::tx().swaps().cancel_offer(id), maker).await
	}

	/// Lock `amount` TESS of offer `id` for `taker` under `hash`, for the payment locked by
	/// `payment_lock`, as the maker, once it checked the payment. Returns the id of the trade.
	pub async fn fill_offer(
		&self,
		maker: &Keypair,
		id: u64,
		taker: &AccountId,
		amount: Amount,
		hash: [u8; 32],
		payment_lock: Vec<u8>,
	) -> Result<u64> {
		let call = runtime::tx().swaps().fill(id, taker.clone(), amount.0, hash, bounded(payment_lock)?);
		let (_, events) = self.submit_events(&call, maker).await?;
		Ok(events
			.find_first::<runtime::swaps::events::Filled>()?
			.ok_or_else(|| Error::NotFound("the trade filled".into()))?
			.trade)
	}

	/// Claim trade `id` for its taker with `secret`: submitted without signer and without fee, so
	/// anyone can submit it, a taker without TESS first.
	pub async fn claim_trade(&self, id: u64, secret: [u8; 32]) -> Result<TxOutcome> {
		self.submit_with_secret("claim", id, secret).await
	}

	/// Call `Swaps.<name>(id, secret)` as a general transaction without signer: the secret
	/// authorizes it, and no fee is paid.
	async fn submit_with_secret(&self, name: &str, id: u64, secret: [u8; 32]) -> Result<TxOutcome> {
		let metadata = self.api().metadata();
		let pallet = metadata.pallet_by_name("Swaps").ok_or_else(|| Error::NotFound("the swaps pallet".into()))?;
		let call =
			pallet.call_variant_by_name(name).ok_or_else(|| Error::NotFound(format!("Swaps.{name}")))?.index;
		// Version 5, general transaction; extension version 0; era, nonce, tip, metadata hash mode.
		let mut body = vec![0b0100_0000 | 5, 0, 0, 0, 0, 0, pallet.index(), call];
		(id, secret).encode_to(&mut body);
		let mut bytes = Compact(body.len() as u32).encode();
		bytes.extend(body);
		let progress =
			subxt::tx::SubmittableTransaction::from_bytes(self.api().clone(), bytes).submit_and_watch().await?;
		Ok(self.included(progress).await?.0)
	}

	/// Open bids.
	pub async fn bids(&self) -> Result<Vec<Bid>> {
		let storage = self.storage().await?;
		let mut entries = storage.iter(runtime::storage().swaps().bids_iter()).await?;
		let mut found = Vec::new();
		while let Some(entry) = entries.next().await {
			let entry = entry?;
			found.push(bid_of(id_of(&entry.key_bytes)?, entry.value));
		}
		found.sort_by_key(|b| b.id);
		Ok(found)
	}

	/// The bid `id`, if open.
	pub async fn bid(&self, id: u64) -> Result<Option<Bid>> {
		let query = runtime::storage().swaps().bids(id);
		Ok(self.storage().await?.fetch(&query).await?.map(|b| bid_of(id, b)))
	}

	/// Sales waiting for their claim or their refund.
	pub async fn sales(&self) -> Result<Vec<Sale>> {
		let storage = self.storage().await?;
		let mut entries = storage.iter(runtime::storage().swaps().sales_iter()).await?;
		let mut found = Vec::new();
		while let Some(entry) = entries.next().await {
			let entry = entry?;
			found.push(sale_of(id_of(&entry.key_bytes)?, entry.value));
		}
		found.sort_by_key(|s| s.id);
		Ok(found)
	}

	/// The sale `id`, if not claimed nor refunded yet.
	pub async fn sale(&self, id: u64) -> Result<Option<Sale>> {
		let query = runtime::storage().swaps().sales(id);
		Ok(self.storage().await?.fetch(&query).await?.map(|s| sale_of(id, s)))
	}

	/// Bid for `amount` TESS at `price` units of `payment` for one TESS, in sales of at least
	/// `min_sale`. Returns the id of the bid.
	pub async fn create_bid(
		&self,
		buyer: &Keypair,
		payment: Payment,
		price: u128,
		amount: Amount,
		min_sale: Amount,
	) -> Result<u64> {
		let call = runtime::tx().swaps().bid(payment.to_runtime(), price, amount.0, min_sale.0);
		let (_, events) = self.submit_events(&call, buyer).await?;
		Ok(events
			.find_first::<runtime::swaps::events::BidPlaced>()?
			.ok_or_else(|| Error::NotFound("the bid placed".into()))?
			.bid)
	}

	/// Change the price of bid `id`, the TESS it still wants and its smallest sale.
	pub async fn update_bid(
		&self,
		buyer: &Keypair,
		id: u64,
		price: u128,
		wanted: Amount,
		min_sale: Amount,
	) -> Result<TxOutcome> {
		self.submit(&runtime::tx().swaps().update_bid(id, price, wanted.0, min_sale.0), buyer).await
	}

	/// Withdraw bid `id`.
	pub async fn cancel_bid(&self, buyer: &Keypair, id: u64) -> Result<TxOutcome> {
		self.submit(&runtime::tx().swaps().cancel_bid(id), buyer).await
	}

	/// Sell `amount` TESS into bid `id`, locked for its buyer under `hash`, paid to `payout`.
	/// Returns the id of the sale.
	pub async fn sell(&self, seller: &Keypair, id: u64, amount: Amount, hash: [u8; 32], payout: Vec<u8>) -> Result<u64> {
		let call = runtime::tx().swaps().sell(id, amount.0, hash, bounded(payout)?);
		let (_, events) = self.submit_events(&call, seller).await?;
		Ok(events
			.find_first::<runtime::swaps::events::Sold>()?
			.ok_or_else(|| Error::NotFound("the sale made".into()))?
			.sale)
	}

	/// Record `payment_lock`, the lock of the payment of sale `id`, as its buyer.
	pub async fn pay_sale(&self, buyer: &Keypair, id: u64, payment_lock: Vec<u8>) -> Result<TxOutcome> {
		self.submit(&runtime::tx().swaps().pay(id, bounded(payment_lock)?), buyer).await
	}

	/// Claim sale `id` for its buyer with `secret`: submitted without signer and without fee, so
	/// anyone can submit it, the seller once paid, or a buyer without TESS.
	pub async fn claim_sale(&self, id: u64, secret: [u8; 32]) -> Result<TxOutcome> {
		self.submit_with_secret("claim_sale", id, secret).await
	}

	/// Return sale `id` to its seller once it expired. Anyone can call it.
	pub async fn refund_sale(&self, signer: &Keypair, id: u64) -> Result<TxOutcome> {
		self.submit(&runtime::tx().swaps().refund_sale(id), signer).await
	}

	/// Return trade `id` to its maker once it expired. Anyone can call it.
	pub async fn refund_trade(&self, signer: &Keypair, id: u64) -> Result<TxOutcome> {
		self.submit(&runtime::tx().swaps().refund(id), signer).await
	}

	/// What happened to trades and sales in block `number` of the best chain.
	pub async fn swap_events(&self, number: u32) -> Result<Vec<SwapEvent>> {
		let Some(hash) = self.block_hash(number).await? else { return Ok(Vec::new()) };
		let events = self.api().events().at(hash).await?;
		let mut found = Vec::new();
		for event in events.iter() {
			let event = event?;
			if let Some(e) = event.as_event::<runtime::swaps::events::Filled>()? {
				found.push(SwapEvent::Filled(Trade {
					id: e.trade,
					offer: e.offer,
					maker: e.maker,
					taker: e.taker,
					amount: Amount(e.amount),
					price: e.price,
					payment: Payment::from_runtime(&e.payment),
					hash: e.hash,
					payment_lock: e.payment_lock.0,
					expires: e.expires,
				}));
			} else if let Some(e) = event.as_event::<runtime::swaps::events::Claimed>()? {
				found.push(SwapEvent::Claimed { trade: e.trade, secret: e.secret });
			} else if let Some(e) = event.as_event::<runtime::swaps::events::Refunded>()? {
				found.push(SwapEvent::Refunded { trade: e.trade });
			} else if let Some(e) = event.as_event::<runtime::swaps::events::Sold>()? {
				found.push(SwapEvent::Sold(Sale {
					id: e.sale,
					bid: e.bid,
					seller: e.seller,
					buyer: e.buyer,
					amount: Amount(e.amount),
					price: e.price,
					payment: Payment::from_runtime(&e.payment),
					hash: e.hash,
					payout: e.payout.0,
					payment_lock: Vec::new(),
					expires: e.expires,
				}));
			} else if let Some(e) = event.as_event::<runtime::swaps::events::SalePaid>()? {
				found.push(SwapEvent::SalePaid { sale: e.sale, payment_lock: e.payment_lock.0 });
			} else if let Some(e) = event.as_event::<runtime::swaps::events::SaleClaimed>()? {
				found.push(SwapEvent::SaleClaimed { sale: e.sale, secret: e.secret });
			} else if let Some(e) = event.as_event::<runtime::swaps::events::SaleRefunded>()? {
				found.push(SwapEvent::SaleRefunded { sale: e.sale });
			}
		}
		Ok(found)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn payments_round_up_and_buy_back_the_amount() {
		// 0.05 USDC (6 decimals) per TESS.
		let price = 50_000;
		assert_eq!(payment_due(100 * PLANCK_PER_TESS, price), Some(5_000_000));
		// A fraction of a unit is paid in full.
		assert_eq!(payment_due(1, price), Some(1));
		assert_eq!(payment_due(u128::MAX, 2), None);
		for amount in [1, 999, PLANCK_PER_TESS, 123_456_789_012_345] {
			let due = payment_due(amount, price).unwrap();
			assert!(tess_for(due, price) >= amount);
			assert!(tess_for(due - 1, price) < amount);
		}
		assert_eq!(tess_for(1_000, 0), 0);
	}

	#[test]
	fn secrets_hash_with_sha256() {
		let hex: String = hash_of(&[0; 32]).iter().map(|b| format!("{b:02x}")).collect();
		assert_eq!(hex, "66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925");
		let (secret, hash) = new_secret().unwrap();
		assert_eq!(hash_of(&secret), hash);
		assert_ne!(new_secret().unwrap().0, secret);
	}
}
