//! The payment side of swaps on Bitcoin: a P2WSH output locked by a hash time-locked script.
//!
//! The taker locks the payment in one transaction of three outputs and its change:
//!
//! 0. the P2WSH output of the script below, paying the maker with the secret of `hash`, or the
//!    taker once `timelock` (a unix time) passed;
//! 1. [`NOTICE_SATS`] to the P2WPKH address of the maker: a maker only watches its own address to
//!    find the payments locked for it;
//! 2. an OP_RETURN output announcing the offer, the Tesserax account of the taker, the hash, the
//!    timelock, and the refund key of the taker ([`Announcement`]).
//!
//! The refund key comes in the announcement (`TR`, 111 bytes), so that a wallet that only pays,
//! such as the Bitcoin account of a browser wallet, needs not sign the refund. A shorter
//! announcement (`TX`, 78 bytes) leaves it out: the refund key is then the public key of the first
//! input, a P2WPKH input. The maker rebuilds the script from the announcement, its own key and the
//! refund key, and checks it is the one of output 0. Every amount is in satoshis.
//!
//! ```text
//! OP_IF
//!     OP_SIZE 32 OP_EQUALVERIFY OP_SHA256 <hash> OP_EQUALVERIFY <maker>
//! OP_ELSE
//!     <timelock> OP_CHECKLOCKTIMEVERIFY OP_DROP <taker>
//! OP_ENDIF
//! OP_CHECKSIG
//! ```

use crate::{AccountId, Error, Result};
use bitcoin::{
	absolute::LockTime,
	consensus,
	ecdsa,
	hashes::Hash,
	opcodes::all::{OP_CHECKSIG, OP_CLTV, OP_DROP, OP_ELSE, OP_ENDIF, OP_EQUALVERIFY, OP_IF, OP_SHA256, OP_SIZE},
	script::{Builder, Instruction, PushBytesBuf},
	secp256k1::Message,
	sighash::{EcdsaSighashType, SighashCache},
	transaction::Version,
	Amount as Sats, KnownHrp, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
};
use serde::Deserialize;

pub use bitcoin::{
	secp256k1::Secp256k1, Address as BitcoinAddress, CompressedPublicKey, Network as BitcoinNetwork, OutPoint, PrivateKey as BitcoinKey,
	Transaction as BitcoinTransaction, Txid,
};

/// Satoshis sent to the address of the maker to notify it, above the dust limit of a P2WPKH
/// output (294 satoshis).
pub const NOTICE_SATS: u64 = 330;

/// First bytes of an announcement without refund key, and of one with it.
pub const MAGIC: [u8; 2] = *b"TX";
pub const MAGIC_WITH_REFUND: [u8; 2] = *b"TR";

/// Length of an announcement: magic, offer, taker, hash, timelock, and the refund key if given.
pub const ANNOUNCEMENT_LEN: usize = 2 + 8 + 32 + 32 + 4;
pub const ANNOUNCEMENT_WITH_REFUND_LEN: usize = ANNOUNCEMENT_LEN + 33;

/// Confirmations a maker waits for before filling a payment: an unconfirmed transaction can be
/// replaced.
pub const MIN_CONFIRMATIONS: u32 = 2;

fn bitcoin_error(error: impl std::fmt::Display) -> Error {
	Error::Bitcoin(error.to_string())
}

/// The Bitcoin network of a Tesserax network: Bitcoin for the mainnet, testnet4 for the testnet,
/// the test network browser wallets such as MetaMask support.
pub fn network_for(testnet: bool) -> BitcoinNetwork {
	if testnet {
		BitcoinNetwork::Testnet4
	} else {
		BitcoinNetwork::Bitcoin
	}
}

/// A public Esplora API of `network`.
pub fn default_esplora(network: BitcoinNetwork) -> &'static str {
	match network {
		BitcoinNetwork::Bitcoin => "https://mempool.space/api",
		BitcoinNetwork::Testnet => "https://mempool.space/testnet/api",
		BitcoinNetwork::Testnet4 => "https://mempool.space/testnet4/api",
		// For tests on one's own machine: swaps/bitcoin/esplora-regtest.mjs.
		BitcoinNetwork::Regtest => "http://127.0.0.1:3002",
		_ => "https://mempool.space/signet/api",
	}
}

/// The parameters of a hash time-locked script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Htlc {
	pub hash: [u8; 32],
	/// Key of the maker, which claims with the secret.
	pub maker: CompressedPublicKey,
	/// Key of the taker, which takes the payment back after `timelock`.
	pub taker: CompressedPublicKey,
	/// Unix time from which the taker may take it back.
	pub timelock: u32,
}

impl Htlc {
	pub fn script(&self) -> ScriptBuf {
		Builder::new()
			.push_opcode(OP_IF)
			.push_opcode(OP_SIZE)
			.push_int(32)
			.push_opcode(OP_EQUALVERIFY)
			.push_opcode(OP_SHA256)
			.push_slice(self.hash)
			.push_opcode(OP_EQUALVERIFY)
			.push_slice(self.maker.to_bytes())
			.push_opcode(OP_ELSE)
			.push_int(i64::from(self.timelock))
			.push_opcode(OP_CLTV)
			.push_opcode(OP_DROP)
			.push_slice(self.taker.to_bytes())
			.push_opcode(OP_ENDIF)
			.push_opcode(OP_CHECKSIG)
			.into_script()
	}

	pub fn script_pubkey(&self) -> ScriptBuf {
		ScriptBuf::new_p2wsh(&self.script().wscript_hash())
	}
}

/// What the OP_RETURN output of a payment announces to its maker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
	pub offer: u64,
	pub taker: AccountId,
	pub hash: [u8; 32],
	pub timelock: u32,
	/// The refund key of the taker, if not the key of the first input.
	pub refund: Option<CompressedPublicKey>,
}

impl Announcement {
	pub fn encode(&self) -> Vec<u8> {
		let mut data = Vec::with_capacity(ANNOUNCEMENT_WITH_REFUND_LEN);
		data.extend_from_slice(if self.refund.is_some() { &MAGIC_WITH_REFUND } else { &MAGIC });
		data.extend_from_slice(&self.offer.to_le_bytes());
		data.extend_from_slice(self.taker.as_ref());
		data.extend_from_slice(&self.hash);
		data.extend_from_slice(&self.timelock.to_le_bytes());
		if let Some(refund) = &self.refund {
			data.extend_from_slice(&refund.to_bytes());
		}
		data
	}

	pub fn decode(data: &[u8]) -> Option<Self> {
		let with_refund = match (data.len(), data.get(..2)?) {
			(ANNOUNCEMENT_LEN, magic) if magic == MAGIC => false,
			(ANNOUNCEMENT_WITH_REFUND_LEN, magic) if magic == MAGIC_WITH_REFUND => true,
			_ => return None,
		};
		Some(Announcement {
			offer: u64::from_le_bytes(data[2..10].try_into().ok()?),
			taker: AccountId::from(<[u8; 32]>::try_from(&data[10..42]).ok()?),
			hash: data[42..74].try_into().ok()?,
			timelock: u32::from_le_bytes(data[74..78].try_into().ok()?),
			refund: if with_refund { Some(CompressedPublicKey::from_slice(&data[78..]).ok()?) } else { None },
		})
	}

	fn script(&self) -> ScriptBuf {
		ScriptBuf::new_op_return(PushBytesBuf::try_from(self.encode()).expect("111 bytes at most fit a push; qed"))
	}
}

/// A payment locked for a maker, as a transaction shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitcoinLock {
	pub outpoint: OutPoint,
	pub amount: u64,
	pub htlc: Htlc,
	pub offer: u64,
	pub taker: AccountId,
}

impl BitcoinLock {
	/// The payment lock of the trade on Tesserax: the outpoint, 36 bytes.
	pub fn lock_bytes(&self) -> Vec<u8> {
		outpoint_bytes(&self.outpoint)
	}
}

/// The outpoint as Tesserax stores it: the transaction id in its internal order, then the output
/// index, little endian.
pub fn outpoint_bytes(outpoint: &OutPoint) -> Vec<u8> {
	consensus::serialize(outpoint)
}

pub fn outpoint_of(bytes: &[u8]) -> Option<OutPoint> {
	consensus::deserialize(bytes).ok()
}

/// The P2WPKH address of `key` on `network`: where a maker is notified and paid.
pub fn address_of(key: &CompressedPublicKey, network: BitcoinNetwork) -> BitcoinAddress {
	BitcoinAddress::p2wpkh(key, KnownHrp::from(network))
}

/// The payment `tx` locks for the maker of key `maker`, if it is a payment of swaps: its
/// announcement, the P2WSH output of the script it describes, and a P2WPKH first input.
pub fn find_lock(tx: &Transaction, maker: &CompressedPublicKey) -> Option<BitcoinLock> {
	let announcement = tx.output.iter().find_map(|o| {
		if !o.script_pubkey.is_op_return() {
			return None;
		}
		match o.script_pubkey.instructions().nth(1)? {
			Ok(Instruction::PushBytes(data)) => Announcement::decode(data.as_bytes()),
			_ => None,
		}
	})?;
	let taker = match announcement.refund {
		Some(refund) => refund,
		None => {
			let witness = &tx.input.first()?.witness;
			if witness.len() != 2 {
				return None;
			}
			CompressedPublicKey::from_slice(witness.nth(1)?).ok()?
		},
	};
	let htlc = Htlc { hash: announcement.hash, maker: *maker, taker, timelock: announcement.timelock };
	let script_pubkey = htlc.script_pubkey();
	let (vout, output) = tx.output.iter().enumerate().find(|(_, o)| o.script_pubkey == script_pubkey)?;
	Some(BitcoinLock {
		outpoint: OutPoint { txid: tx.compute_txid(), vout: vout as u32 },
		amount: output.value.to_sat(),
		htlc,
		offer: announcement.offer,
		taker: announcement.taker,
	})
}

/// An output a key can spend.
#[derive(Clone, Copy, Debug)]
pub struct Utxo {
	pub outpoint: OutPoint,
	pub amount: u64,
}

fn sign_p2wpkh(tx: &mut Transaction, key: &BitcoinKey, amounts: &[u64]) -> Result<()> {
	let secp = Secp256k1::new();
	let public = CompressedPublicKey::from_private_key(&secp, key).map_err(bitcoin_error)?;
	let script_pubkey = ScriptBuf::new_p2wpkh(&public.wpubkey_hash());
	let mut cache = SighashCache::new(tx.clone());
	for (index, amount) in amounts.iter().enumerate() {
		let sighash = cache
			.p2wpkh_signature_hash(index, &script_pubkey, Sats::from_sat(*amount), EcdsaSighashType::All)
			.map_err(bitcoin_error)?;
		let signature = secp.sign_ecdsa(&Message::from_digest(sighash.to_byte_array()), &key.inner);
		let signature = ecdsa::Signature { signature, sighash_type: EcdsaSighashType::All };
		tx.input[index].witness = Witness::p2wpkh(&signature, &public.0);
	}
	Ok(())
}

/// Signed transactions are measured with their signature: a fee of `rate` satoshis per virtual
/// byte of the transaction `build(fee)` returns, signed.
fn with_fee(rate: f64, build: impl Fn(u64) -> Result<Transaction>) -> Result<Transaction> {
	let measured = build(0)?;
	// A signature may be one byte longer than the one measured.
	let fee = ((measured.vsize() as f64 + 1.0) * rate).ceil() as u64;
	build(fee)
}

/// The transaction locking `amount` for the maker of `htlc`, from `utxos` of `key`, whose first
/// input gives the refund key: its outputs are the script, the notice of the maker, the
/// announcement and the change back to `key`.
pub fn lock_transaction(
	key: &BitcoinKey,
	utxos: &[Utxo],
	htlc: &Htlc,
	announcement: &Announcement,
	amount: u64,
	fee_rate: f64,
	network: BitcoinNetwork,
) -> Result<Transaction> {
	let secp = Secp256k1::new();
	let public = CompressedPublicKey::from_private_key(&secp, key).map_err(bitcoin_error)?;
	if announcement.refund.unwrap_or(public) != htlc.taker {
		return Err(Error::InvalidInput("the refund key of the script is neither announced nor the key paying".into()));
	}
	let notice = ScriptBuf::new_p2wpkh(&htlc.maker.wpubkey_hash());
	let change = address_of(&public, network).script_pubkey();
	let total: u64 = utxos.iter().map(|u| u.amount).sum();
	with_fee(fee_rate, |fee| {
		let spent = amount + NOTICE_SATS + fee;
		let left = total.checked_sub(spent).ok_or_else(|| {
			Error::InvalidAmount(format!("{total} satoshis available, {spent} needed with the fee"))
		})?;
		let mut output = vec![
			TxOut { value: Sats::from_sat(amount), script_pubkey: htlc.script_pubkey() },
			TxOut { value: Sats::from_sat(NOTICE_SATS), script_pubkey: notice.clone() },
			TxOut { value: Sats::ZERO, script_pubkey: announcement.script() },
		];
		// Change below the dust limit goes to the fee.
		if left >= 546 {
			output.push(TxOut { value: Sats::from_sat(left), script_pubkey: change.clone() });
		}
		let mut tx = Transaction {
			version: Version::TWO,
			lock_time: LockTime::ZERO,
			input: utxos
				.iter()
				.map(|u| TxIn { previous_output: u.outpoint, sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, ..Default::default() })
				.collect(),
			output,
		};
		sign_p2wpkh(&mut tx, key, &utxos.iter().map(|u| u.amount).collect::<Vec<_>>())?;
		Ok(tx)
	})
}

fn spend_htlc(
	lock: &BitcoinLock,
	key: &BitcoinKey,
	destination: &ScriptBuf,
	fee_rate: f64,
	lock_time: LockTime,
	sequence: Sequence,
	witness: impl Fn(Vec<u8>, &ScriptBuf) -> Witness,
) -> Result<Transaction> {
	let secp = Secp256k1::new();
	let script = lock.htlc.script();
	with_fee(fee_rate, |fee| {
		let value = lock
			.amount
			.checked_sub(fee)
			.filter(|v| *v >= 294)
			.ok_or_else(|| Error::InvalidAmount(format!("the fee takes the {} satoshis of the payment", lock.amount)))?;
		let mut tx = Transaction {
			version: Version::TWO,
			lock_time,
			input: vec![TxIn { previous_output: lock.outpoint, sequence, ..Default::default() }],
			output: vec![TxOut { value: Sats::from_sat(value), script_pubkey: destination.clone() }],
		};
		let sighash = SighashCache::new(&tx)
			.p2wsh_signature_hash(0, &script, Sats::from_sat(lock.amount), EcdsaSighashType::All)
			.map_err(bitcoin_error)?;
		let signature = secp.sign_ecdsa(&Message::from_digest(sighash.to_byte_array()), &key.inner);
		let signature = ecdsa::Signature { signature, sighash_type: EcdsaSighashType::All }.to_vec();
		tx.input[0].witness = witness(signature, &script);
		Ok(tx)
	})
}

/// The maker claims `lock` with `secret`, to its own P2WPKH address.
pub fn claim_transaction(lock: &BitcoinLock, secret: [u8; 32], key: &BitcoinKey, fee_rate: f64, network: BitcoinNetwork) -> Result<Transaction> {
	let destination = address_of(&lock.htlc.maker, network).script_pubkey();
	spend_htlc(lock, key, &destination, fee_rate, LockTime::ZERO, Sequence::ENABLE_RBF_NO_LOCKTIME, |signature, script| {
		Witness::from_slice(&[signature, secret.to_vec(), vec![1], script.to_bytes()])
	})
}

/// The taker takes `lock` back once its timelock passed, to its own P2WPKH address.
pub fn refund_transaction(lock: &BitcoinLock, key: &BitcoinKey, fee_rate: f64, network: BitcoinNetwork) -> Result<Transaction> {
	let destination = address_of(&lock.htlc.taker, network).script_pubkey();
	let lock_time = LockTime::from_time(lock.htlc.timelock).map_err(bitcoin_error)?;
	spend_htlc(lock, key, &destination, fee_rate, lock_time, Sequence::ENABLE_LOCKTIME_NO_RBF, |signature, script| {
		Witness::from_slice(&[signature, Vec::new(), script.to_bytes()])
	})
}

#[derive(Deserialize)]
struct EsploraStatus {
	confirmed: bool,
	block_height: Option<u32>,
	block_time: Option<u64>,
}

#[derive(Deserialize)]
struct EsploraUtxo {
	txid: String,
	vout: u32,
	value: u64,
}

#[derive(Deserialize)]
struct EsploraTx {
	txid: String,
	status: EsploraStatus,
}

#[derive(Deserialize)]
struct EsploraOutspend {
	spent: bool,
}

#[derive(Deserialize)]
struct EsploraBlock {
	mediantime: u64,
}

/// A transaction of an address, with its confirmation.
#[derive(Clone, Copy, Debug)]
pub struct AddressTx {
	pub txid: Txid,
	/// Height of its block, if confirmed, and the unix time of the block.
	pub height: Option<u32>,
	pub time: Option<u64>,
}

/// A Bitcoin network through an Esplora API, such as the one of mempool.space.
#[derive(Clone)]
pub struct Esplora {
	pub network: BitcoinNetwork,
	base: String,
	http: reqwest::Client,
}

impl Esplora {
	pub fn new(network: BitcoinNetwork, url: Option<&str>) -> Self {
		let base = url.unwrap_or(default_esplora(network)).trim_end_matches('/').to_string();
		Esplora { network, base, http: reqwest::Client::new() }
	}

	/// GET `path`, tried again twice when the API fails or is busy.
	async fn get(&self, path: &str) -> Result<reqwest::Response> {
		let mut attempt = 0;
		loop {
			attempt += 1;
			let failure = match self.http.get(format!("{}{path}", self.base)).send().await {
				Ok(response) if response.status().is_success() => return Ok(response),
				Ok(response) => {
					let status = response.status();
					let retry = status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
					let text = response.text().await.unwrap_or_default();
					let text = if text.trim_start().starts_with('<') { String::new() } else { text };
					(retry, format!("{path}: {status} {}", text.trim()))
				},
				Err(e) => (true, format!("{path}: {e}")),
			};
			if !failure.0 || attempt == 3 {
				return Err(Error::Bitcoin(failure.1));
			}
			tokio::time::sleep(std::time::Duration::from_secs(attempt)).await;
		}
	}

	async fn json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
		let text = self.get(path).await?.text().await.map_err(bitcoin_error)?;
		serde_json::from_str(&text).map_err(bitcoin_error)
	}

	/// Height of the best block.
	pub async fn height(&self) -> Result<u32> {
		self.get("/blocks/tip/height").await?.text().await.map_err(bitcoin_error)?.trim().parse().map_err(bitcoin_error)
	}

	/// Median time of the last 11 blocks: the time a timelock compares to.
	pub async fn median_time(&self) -> Result<u64> {
		let hash = self.get("/blocks/tip/hash").await?.text().await.map_err(bitcoin_error)?;
		Ok(self.json::<EsploraBlock>(&format!("/block/{}", hash.trim())).await?.mediantime)
	}

	/// Fee rate, in satoshis per virtual byte, to confirm within `blocks` blocks.
	pub async fn fee_rate(&self, blocks: u32) -> Result<f64> {
		let estimates: std::collections::BTreeMap<String, f64> = self.json("/fee-estimates").await?;
		let rate = estimates
			.iter()
			.filter_map(|(k, v)| Some((k.parse::<u32>().ok()?, *v)))
			.filter(|(target, _)| *target <= blocks.max(1))
			.min_by(|a, b| a.1.total_cmp(&b.1))
			.map(|(_, v)| v)
			.unwrap_or(2.0);
		Ok(rate.max(1.0))
	}

	pub async fn utxos(&self, address: &BitcoinAddress) -> Result<Vec<Utxo>> {
		let found: Vec<EsploraUtxo> = self.json(&format!("/address/{address}/utxo")).await?;
		found
			.into_iter()
			.map(|u| Ok(Utxo { outpoint: OutPoint { txid: u.txid.parse().map_err(bitcoin_error)?, vout: u.vout }, amount: u.value }))
			.collect()
	}

	/// Transactions of `address`, the latest first: those of the mempool, then the confirmed
	/// ones after `after`, if given, 25 at a time.
	pub async fn address_txs(&self, address: &BitcoinAddress, after: Option<Txid>) -> Result<Vec<AddressTx>> {
		let path = match after {
			Some(txid) => format!("/address/{address}/txs/chain/{txid}"),
			None => format!("/address/{address}/txs"),
		};
		let found: Vec<EsploraTx> = self.json(&path).await?;
		found
			.into_iter()
			.map(|t| {
				Ok(AddressTx {
					txid: t.txid.parse().map_err(bitcoin_error)?,
					height: t.status.block_height.filter(|_| t.status.confirmed),
					time: t.status.block_time,
				})
			})
			.collect()
	}

	pub async fn transaction(&self, txid: &Txid) -> Result<Transaction> {
		let hex = self.get(&format!("/tx/{txid}/hex")).await?.text().await.map_err(bitcoin_error)?;
		consensus::encode::deserialize_hex(hex.trim()).map_err(bitcoin_error)
	}

	/// Confirmations of `txid`: 0 while in the mempool.
	pub async fn confirmations(&self, txid: &Txid) -> Result<u32> {
		let status: EsploraStatus = self.json(&format!("/tx/{txid}/status")).await?;
		match status.block_height.filter(|_| status.confirmed) {
			Some(height) => Ok(self.height().await?.saturating_sub(height) + 1),
			None => Ok(0),
		}
	}

	/// Whether `outpoint` is spent.
	pub async fn is_spent(&self, outpoint: &OutPoint) -> Result<bool> {
		let outspend: EsploraOutspend = self.json(&format!("/tx/{}/outspend/{}", outpoint.txid, outpoint.vout)).await?;
		Ok(outspend.spent)
	}

	/// Broadcast `tx`. Returns its id.
	pub async fn broadcast(&self, tx: &Transaction) -> Result<Txid> {
		let response = self
			.http
			.post(format!("{}/tx", self.base))
			.body(consensus::encode::serialize_hex(tx))
			.send()
			.await
			.map_err(bitcoin_error)?;
		let status = response.status();
		let text = response.text().await.map_err(bitcoin_error)?;
		if !status.is_success() {
			return Err(Error::Bitcoin(format!("broadcast refused: {text}")));
		}
		text.trim().parse().map_err(bitcoin_error)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use bitcoin::{secp256k1::SecretKey, Network};

	fn key(byte: u8) -> BitcoinKey {
		BitcoinKey::new(SecretKey::from_slice(&[byte; 32]).unwrap(), Network::Regtest)
	}

	fn public(key: &BitcoinKey) -> CompressedPublicKey {
		CompressedPublicKey::from_private_key(&Secp256k1::new(), key).unwrap()
	}

	/// A payment of 50,000 satoshis locked by the taker of key 2 for the maker of key 1.
	fn locked() -> (Transaction, Htlc, Announcement, [u8; 32], u64) {
		let (maker, taker) = (key(1), key(2));
		let secret = [7u8; 32];
		let hash = bitcoin::hashes::sha256::Hash::hash(&secret).to_byte_array();
		let htlc = Htlc { hash, maker: public(&maker), taker: public(&taker), timelock: 1_800_000_000 };
		let announcement = Announcement { offer: 3, taker: AccountId::from([9u8; 32]), hash, timelock: htlc.timelock, refund: None };
		let funding = OutPoint { txid: Txid::all_zeros(), vout: 0 };
		let utxo_amount = 100_000;
		let tx = lock_transaction(&taker, &[Utxo { outpoint: funding, amount: utxo_amount }], &htlc, &announcement, 50_000, 2.0, Network::Regtest).unwrap();
		(tx, htlc, announcement, secret, utxo_amount)
	}

	/// Reads of the public Esplora API of testnet4: `cargo test -p tesserax-sdk --features swaps
	/// esplora -- --ignored`.
	#[tokio::test]
	#[ignore = "reads mempool.space"]
	async fn esplora_reads_testnet4() {
		let esplora = Esplora::new(Network::Testnet4, None);
		let height = esplora.height().await.unwrap();
		assert!(height > 100_000, "height {height}");
		let time = esplora.median_time().await.unwrap();
		assert!(time > 1_700_000_000, "median time {time}");
		assert!(esplora.fee_rate(6).await.unwrap() >= 1.0);
		let address = address_of(&public(&key(1)), Network::Testnet4);
		assert!(address.to_string().starts_with("tb1q"));
		// Key 0x01…01 is a known one: its address has transactions, read back here.
		esplora.utxos(&address).await.unwrap();
		if let Some(first) = esplora.address_txs(&address, None).await.unwrap().first() {
			let tx = esplora.transaction(&first.txid).await.unwrap();
			assert_eq!(tx.compute_txid(), first.txid);
			if first.height.is_some() {
				assert!(esplora.confirmations(&first.txid).await.unwrap() >= 1);
			}
		}
	}

	#[test]
	fn announcements_round_trip() {
		let a = Announcement { offer: u64::MAX - 1, taker: AccountId::from([5u8; 32]), hash: [6u8; 32], timelock: 1_900_000_000, refund: None };
		assert_eq!(a.encode().len(), ANNOUNCEMENT_LEN);
		assert_eq!(Announcement::decode(&a.encode()), Some(a.clone()));
		let with_refund = Announcement { refund: Some(public(&key(4))), ..a.clone() };
		assert_eq!(with_refund.encode().len(), ANNOUNCEMENT_WITH_REFUND_LEN);
		assert_eq!(&with_refund.encode()[..2], b"TR");
		assert_eq!(Announcement::decode(&with_refund.encode()), Some(with_refund.clone()));
		// A magic of the other length is refused.
		let mut mixed = with_refund.encode();
		mixed[1] = b'X';
		assert_eq!(Announcement::decode(&mixed), None);
		assert_eq!(Announcement::decode(&a.encode()[1..]), None);
		let mut other = a.encode();
		other[0] = b'Y';
		assert_eq!(Announcement::decode(&other), None);
	}

	#[test]
	fn makers_find_the_payments_locked_for_them() {
		let (tx, htlc, announcement, _, utxo_amount) = locked();
		assert_eq!(tx.output.len(), 4);
		assert_eq!(tx.output[1].value.to_sat(), NOTICE_SATS);
		assert!(tx.output[2].script_pubkey.is_op_return());
		let fee = utxo_amount - tx.output.iter().map(|o| o.value.to_sat()).sum::<u64>();
		assert!((2 * tx.vsize() as u64..=2 * tx.vsize() as u64 + 4).contains(&fee), "fee {fee} for {} vbytes", tx.vsize());

		// The inputs of the taker are signed, as Bitcoin Core verifies them.
		let funding = ScriptBuf::new_p2wpkh(&htlc.taker.wpubkey_hash());
		bitcoin::consensus::verify_script(&funding, 0, Sats::from_sat(utxo_amount), &consensus::serialize(&tx)).unwrap();

		let lock = find_lock(&tx, &htlc.maker).unwrap();
		assert_eq!(lock.htlc, htlc);
		assert_eq!(lock.amount, 50_000);
		assert_eq!((lock.offer, lock.taker.clone()), (announcement.offer, announcement.taker));
		assert_eq!(lock.outpoint, OutPoint { txid: tx.compute_txid(), vout: 0 });
		assert_eq!(outpoint_of(&lock.lock_bytes()), Some(lock.outpoint));
		assert_eq!(lock.lock_bytes().len(), 36);
		// Another maker finds nothing in it.
		assert_eq!(find_lock(&tx, &public(&key(3))), None);
	}

	/// A wallet that only pays (key 5) locks for a refund key it does not hold (key 2), announced.
	#[test]
	fn announced_refund_keys_take_payments_back() {
		let (maker, payer, refund) = (key(1), key(5), key(2));
		let secret = [7u8; 32];
		let hash = bitcoin::hashes::sha256::Hash::hash(&secret).to_byte_array();
		let htlc = Htlc { hash, maker: public(&maker), taker: public(&refund), timelock: 1_800_000_000 };
		let announcement = Announcement { offer: 4, taker: AccountId::from([9u8; 32]), hash, timelock: htlc.timelock, refund: Some(htlc.taker) };
		let utxo = [Utxo { outpoint: OutPoint::null(), amount: 100_000 }];
		let tx = lock_transaction(&payer, &utxo, &htlc, &announcement, 40_000, 2.0, Network::Regtest).unwrap();
		let lock = find_lock(&tx, &htlc.maker).unwrap();
		assert_eq!(lock.htlc, htlc);
		assert_eq!(lock.offer, 4);

		let spent = &tx.output[lock.outpoint.vout as usize];
		let verify = |spending: &Transaction| {
			bitcoin::consensus::verify_script(&spent.script_pubkey, 0, spent.value, &consensus::serialize(spending))
		};
		verify(&refund_transaction(&lock, &refund, 3.0, Network::Regtest).unwrap()).unwrap();
		assert!(verify(&refund_transaction(&lock, &payer, 3.0, Network::Regtest).unwrap()).is_err());
		verify(&claim_transaction(&lock, secret, &maker, 3.0, Network::Regtest).unwrap()).unwrap();
	}

	/// A payment locked by a browser wallet and taken back by the Tesserax wallet, both built by
	/// the TypeScript SDK (`sdk-ts/test/bitcoin.test.ts`), as Bitcoin Core verifies them.
	#[test]
	fn transactions_of_the_typescript_sdk_are_valid() {
		let vectors: serde_json::Value = serde_json::from_str(include_str!("testdata/bitcoin-ts.json")).unwrap();
		let field = |name: &str| -> Vec<u8> { bitcoin::hex::FromHex::from_hex(vectors[name].as_str().unwrap()).unwrap() };
		let maker = CompressedPublicKey::from_slice(&field("maker")).unwrap();
		let lock_tx: Transaction = consensus::deserialize(&field("lock")).unwrap();

		// The browser wallet signed its input.
		let funding = ScriptBuf::from_bytes(field("payer_script"));
		let funded = Sats::from_sat(vectors["funding_amount"].as_u64().unwrap());
		bitcoin::consensus::verify_script(&funding, 0, funded, &field("lock")).unwrap();

		// The maker finds the payment, with the announced refund key, and the same script.
		let lock = find_lock(&lock_tx, &maker).unwrap();
		assert_eq!(lock.offer, 4);
		assert_eq!(AsRef::<[u8]>::as_ref(&lock.taker), field("taker").as_slice());
		assert_eq!(lock.htlc.taker.to_bytes().to_vec(), field("refund"));
		assert_eq!(lock.htlc.script().to_bytes(), field("script"));

		// The refund signed with the key of the Tesserax wallet spends it.
		let spent = &lock_tx.output[lock.outpoint.vout as usize];
		bitcoin::consensus::verify_script(&spent.script_pubkey, 0, spent.value, &field("refund_tx")).unwrap();
	}

	/// A payment locked, claimed and taken back by the Python SDK (`sdk-python/tests/test_bitcoin.py`),
	/// as Bitcoin Core verifies it.
	#[test]
	fn transactions_of_the_python_sdk_are_valid() {
		let vectors: serde_json::Value = serde_json::from_str(include_str!("testdata/bitcoin-py.json")).unwrap();
		let field = |name: &str| -> Vec<u8> { bitcoin::hex::FromHex::from_hex(vectors[name].as_str().unwrap()).unwrap() };
		let maker = CompressedPublicKey::from_slice(&field("maker")).unwrap();
		let lock_tx: Transaction = consensus::deserialize(&field("lock")).unwrap();

		let funding = ScriptBuf::from_bytes(field("payer_script"));
		let funded = Sats::from_sat(vectors["funding_amount"].as_u64().unwrap());
		bitcoin::consensus::verify_script(&funding, 0, funded, &field("lock")).unwrap();

		let lock = find_lock(&lock_tx, &maker).unwrap();
		assert_eq!(lock.offer, 4);
		let spent = &lock_tx.output[lock.outpoint.vout as usize];
		for spending in ["claim", "refund"] {
			let tx: Transaction = consensus::deserialize(&field(spending)).unwrap();
			assert_eq!(tx.input[0].previous_output, lock.outpoint, "{spending}");
			bitcoin::consensus::verify_script(&spent.script_pubkey, 0, spent.value, &field(spending)).unwrap();
		}
	}

	#[test]
	fn a_lock_needs_enough_funds_and_the_refund_key() {
		let (_, htlc, announcement, _, _) = locked();
		let utxo = [Utxo { outpoint: OutPoint::null(), amount: 50_100 }];
		assert!(matches!(lock_transaction(&key(2), &utxo, &htlc, &announcement, 50_000, 2.0, Network::Regtest), Err(Error::InvalidAmount(_))));
		let utxo = [Utxo { outpoint: OutPoint::null(), amount: 100_000 }];
		assert!(lock_transaction(&key(4), &utxo, &htlc, &announcement, 50_000, 2.0, Network::Regtest).is_err());
	}

	/// The claim and the refund satisfy the script, as Bitcoin Core verifies it.
	#[test]
	fn claims_and_refunds_spend_the_script() {
		let (tx, htlc, _, secret, _) = locked();
		let lock = find_lock(&tx, &htlc.maker).unwrap();
		let spent = &tx.output[lock.outpoint.vout as usize];
		let verify = |spending: &Transaction| {
			bitcoin::consensus::verify_script(&spent.script_pubkey, 0, spent.value, &consensus::serialize(spending))
		};

		let claim = claim_transaction(&lock, secret, &key(1), 3.0, Network::Regtest).unwrap();
		verify(&claim).unwrap();
		assert_eq!(claim.output[0].script_pubkey, address_of(&htlc.maker, Network::Regtest).script_pubkey());
		// A wrong secret, or the key of the taker, cannot claim.
		assert!(verify(&claim_transaction(&lock, [8u8; 32], &key(1), 3.0, Network::Regtest).unwrap()).is_err());
		assert!(verify(&claim_transaction(&lock, secret, &key(2), 3.0, Network::Regtest).unwrap()).is_err());

		let refund = refund_transaction(&lock, &key(2), 3.0, Network::Regtest).unwrap();
		verify(&refund).unwrap();
		assert_eq!(refund.lock_time, LockTime::from_time(htlc.timelock).unwrap());
		assert!(verify(&refund_transaction(&lock, &key(1), 3.0, Network::Regtest).unwrap()).is_err());
		// Before its timelock, the refund is refused.
		let early = spend_htlc(
			&lock,
			&key(2),
			&refund.output[0].script_pubkey,
			3.0,
			LockTime::from_time(htlc.timelock - 1).unwrap(),
			Sequence::ENABLE_LOCKTIME_NO_RBF,
			|signature, script| Witness::from_slice(&[signature, Vec::new(), script.to_bytes()]),
		)
		.unwrap();
		assert!(verify(&early).is_err());
	}
}
