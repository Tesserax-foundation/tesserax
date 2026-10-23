//! The shielded wallet of Tesserax for JavaScript (`wasm-pack build --target web`).
//!
//! Values are planck as `bigint`, accounts 32-byte arrays, digests the 32 bytes of their SCALE
//! encoding, transactions the SCALE encoding of `pallet_shielded::ShieldedTransaction`.

use codec::{Decode, Encode};
use tesserax_shielded_wallet::{self as shielded, Digest, ShieldedTransaction, Transacted};
use wasm_bindgen::prelude::*;

fn js(error: impl core::fmt::Display) -> JsError {
	JsError::new(&error.to_string())
}

fn account(bytes: &[u8]) -> Result<[u8; 32], JsError> {
	bytes.try_into().map_err(|_| JsError::new("an account is 32 bytes"))
}

fn digests(bytes: &[u8]) -> Result<[Digest; 2], JsError> {
	<[Digest; 2]>::decode(&mut &bytes[..]).map_err(js)
}

fn transaction(bytes: &[u8]) -> Result<ShieldedTransaction, JsError> {
	ShieldedTransaction::decode(&mut &bytes[..]).map_err(js)
}

/// Whether `text` is a shielded address.
#[wasm_bindgen(js_name = isShieldedAddress)]
pub fn is_shielded_address(text: &str) -> bool {
	shielded::decode_address(text).is_ok()
}

/// A `Shielded.transact` extrinsic without signer.
#[wasm_bindgen(js_name = transactExtrinsic)]
pub fn transact_extrinsic(pallet: u8, call: u8, transaction_bytes: &[u8]) -> Result<Vec<u8>, JsError> {
	Ok(shielded::transact_extrinsic(pallet, call, &transaction(transaction_bytes)?))
}

/// Call data of `Shielded.<call>(transaction)`.
#[wasm_bindgen(js_name = callData)]
pub fn call_data(pallet: u8, call: u8, transaction_bytes: &[u8]) -> Result<Vec<u8>, JsError> {
	Ok(shielded::call_data(pallet, call, &transaction(transaction_bytes)?))
}

/// The shielded account of the account whose ML-DSA key has the seed `seed`.
#[wasm_bindgen]
pub struct ShieldedWallet(shielded::Wallet);

#[wasm_bindgen]
impl ShieldedWallet {
	/// The shielded account of `seed`, restored from `state` if given.
	#[wasm_bindgen(constructor)]
	pub fn new(seed: &[u8], state: Option<Vec<u8>>) -> Result<ShieldedWallet, JsError> {
		let key = shielded::spending_key(&account(seed)?);
		Ok(ShieldedWallet(match state {
			Some(state) => shielded::Wallet::restore(key, &state).map_err(js)?,
			None => shielded::Wallet::new(key),
		}))
	}

	/// Its shielded address, `zte...`.
	pub fn address(&self) -> String {
		shielded::encode_address(self.0.address())
	}

	/// Commitments ingested: the position of the next one.
	pub fn position(&self) -> u64 {
		self.0.position()
	}

	/// Root of the note tree, the 32 bytes of its SCALE encoding.
	pub fn root(&self) -> Vec<u8> {
		self.0.root().encode()
	}

	/// Spendable balance.
	pub fn balance(&self) -> u64 {
		self.0.balance()
	}

	/// Balance spent by transactions not yet seen on chain.
	pub fn pending(&self) -> u64 {
		self.0.pending()
	}

	/// Ingest the next `Shielded.Transacted` event. Returns the value it pays to this account.
	pub fn ingest(
		&mut self,
		position: u64,
		nullifiers: &[u8],
		commitments: &[u8],
		note_0: Vec<u8>,
		note_1: Vec<u8>,
	) -> Result<u64, JsError> {
		let event = Transacted {
			position,
			nullifiers: digests(nullifiers)?,
			commitments: digests(commitments)?,
			notes: [note_0, note_1],
		};
		Ok(self.0.ingest(&event).map_err(js)?.iter().map(|n| n.value).sum())
	}

	/// Shield `value` of the transparent account `source`.
	pub fn shield(&self, source: &[u8], value: u64) -> Result<Vec<u8>, JsError> {
		Ok(self.0.shield(account(source)?, value).map_err(js)?.encode())
	}

	/// Send `value` to the shielded address `to`, paying `fee` from the pool.
	pub fn send(&self, to: &str, value: u64, fee: u64) -> Result<Vec<u8>, JsError> {
		let to = shielded::decode_address(to).map_err(js)?;
		Ok(self.0.send(&to, value, fee).map_err(js)?.encode())
	}

	/// Send `value` from the pool to the transparent account `recipient`, paying `fee`.
	pub fn unshield(&self, recipient: &[u8], value: u64, fee: u64) -> Result<Vec<u8>, JsError> {
		Ok(self.0.unshield(account(recipient)?, value, fee).map_err(js)?.encode())
	}

	/// Mark the notes of a submitted transaction as spent until the chain shows it.
	#[wasm_bindgen(js_name = markPending)]
	pub fn mark_pending(&mut self, transaction_bytes: &[u8]) -> Result<(), JsError> {
		self.0.mark_pending(&transaction(transaction_bytes)?);
		Ok(())
	}

	/// Make spendable again the notes of transactions that were not included.
	#[wasm_bindgen(js_name = releasePending)]
	pub fn release_pending(&mut self) {
		self.0.release_pending();
	}

	/// The tree and the notes, to store.
	pub fn state(&self) -> Vec<u8> {
		self.0.state()
	}
}
