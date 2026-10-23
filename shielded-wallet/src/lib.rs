//! Client of the Tesserax shielded pool (`docs/privacy.md`).
//!
//! A [`Wallet`] holds the spending key of a shielded account, the note tree of the chain and the
//! notes the account owns. It follows the chain by ingesting, in order, the `Shielded.Transacted`
//! events of every block ([`Wallet::ingest`]), and builds proven transactions:
//!
//! - [`Wallet::shield`]: TESS of a transparent account into the pool, submitted with
//!   `Shielded.shield` signed by that account;
//! - [`Wallet::send`]: a private transfer to a shielded address;
//! - [`Wallet::unshield`]: TESS of the pool to a transparent account.
//!
//! Private transfers and unshields are submitted without signer, so that they are linked to no
//! account: [`transact_extrinsic`] encodes them.
//!
//! The shielded key of an account derives from the 32-byte seed of its ML-DSA key
//! ([`spending_key`]): restoring an account from its recovery phrase restores its shielded notes.

use codec::{Decode, Encode};
use tesserax_shielded::{
	encryption::{decrypt, encrypt},
	note::{nullifier, output_rho, random_randomness},
	proof::{prove, InputWitness, OutputWitness, PublicInputs, Witness},
	tree::{MerklePath, Tree, TREE_DEPTH},
};
pub use tesserax_shielded::{
	keys::{Address, SpendingKey},
	Digest, EncryptedNote, Note, ENCRYPTED_NOTE_SIZE, MAX_VALUE,
};

/// BLAKE2b-256, the hash of the runtime.
fn blake2_256(data: &[u8]) -> [u8; 32] {
	use blake2::{digest::consts::U32, Blake2b, Digest as _};
	Blake2b::<U32>::digest(data).into()
}

/// Prefix of shielded addresses, which are long: an ML-KEM-768 key is 1184 bytes.
pub const ADDRESS_PREFIX: &str = "zte";

/// Context of the binding hash of a transaction, as the runtime computes it.
const BINDING_CONTEXT: &[u8] = b"tesserax-shielded-binding";

/// Version of the encoded state of a wallet.
const STATE_VERSION: u8 = 1;

/// Errors of the shielded wallet.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
	#[error("not a shielded address")]
	InvalidAddress,
	#[error("insufficient shielded balance: {available} planck available, {required} required")]
	InsufficientFunds { available: u64, required: u64 },
	#[error(
		"the amount needs more than two notes: send {max} planck at most, or first send yourself your whole balance in steps"
	)]
	TooManyNotes { max: u64 },
	#[error("amount out of range")]
	AmountOutOfRange,
	#[error("events out of order: expected the commitment at position {expected}, got {got}")]
	OutOfOrder { expected: u64, got: u64 },
	#[error("the proof could not be generated: {0}")]
	Proof(String),
	#[error("invalid wallet state: {0}")]
	State(String),
}

/// The shielded spending key of the account whose ML-DSA key has the 32-byte seed `seed`.
pub fn spending_key(seed: &[u8; 32]) -> SpendingKey {
	let mut data = b"tesserax-shielded-seed".to_vec();
	data.extend_from_slice(seed);
	SpendingKey::from_seed(blake2_256(&data))
}

fn checksum(bytes: &[u8]) -> [u8; 4] {
	let mut data = b"tesserax-shielded-address".to_vec();
	data.extend_from_slice(bytes);
	blake2_256(&data)[..4].try_into().expect("4 bytes; qed")
}

/// A shielded address as text: `zte` and the Base58 of its bytes and a checksum.
pub fn encode_address(address: &Address) -> String {
	let mut bytes = address.to_bytes();
	let check = checksum(&bytes);
	bytes.extend_from_slice(&check);
	format!("{ADDRESS_PREFIX}{}", bs58::encode(bytes).into_string())
}

/// A shielded address from its text.
pub fn decode_address(text: &str) -> Result<Address, Error> {
	let body = text.trim().strip_prefix(ADDRESS_PREFIX).ok_or(Error::InvalidAddress)?;
	let bytes = bs58::decode(body).into_vec().map_err(|_| Error::InvalidAddress)?;
	let (address, check) = bytes.split_at(bytes.len().checked_sub(4).ok_or(Error::InvalidAddress)?);
	if checksum(address) != check {
		return Err(Error::InvalidAddress);
	}
	Address::from_bytes(address).ok_or(Error::InvalidAddress)
}

/// A shielded transaction, encoded as the runtime decodes `pallet_shielded::ShieldedTransaction`
/// with 32-byte accounts.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct ShieldedTransaction {
	pub anchor: Digest,
	pub nullifiers: [Digest; 2],
	pub commitments: [Digest; 2],
	pub value_in: u64,
	pub value_out: u64,
	pub fee: u64,
	pub recipient: Option<[u8; 32]>,
	pub notes: [EncryptedNote; 2],
	pub proof: Vec<u8>,
}

impl ShieldedTransaction {
	/// Hash of the transparent accounts and the encrypted notes, which the proof commits to.
	pub fn binding(&self, source: Option<&[u8; 32]>) -> Digest {
		let data = (BINDING_CONTEXT, source, &self.recipient, &self.notes).encode();
		Digest::from_bytes_reduced(&blake2_256(&data))
	}

	/// Public inputs of the proof.
	pub fn public_inputs(&self, source: Option<&[u8; 32]>) -> PublicInputs {
		PublicInputs {
			anchor: self.anchor,
			nullifiers: self.nullifiers,
			commitments: self.commitments,
			value_in: self.value_in,
			value_out: self.value_out,
			fee: self.fee,
			binding: self.binding(source),
		}
	}

	/// Whether the proof verifies, as the runtime checks it.
	pub fn verify(&self, source: Option<&[u8; 32]>) -> bool {
		tesserax_shielded::proof::verify(&self.proof, &self.public_inputs(source))
	}
}

/// Call data of `Shielded.<call>(transaction)`: the index of the pallet and of the call in the
/// metadata of the runtime, then the transaction.
pub fn call_data(pallet: u8, call: u8, transaction: &ShieldedTransaction) -> Vec<u8> {
	let mut data = vec![pallet, call];
	transaction.encode_to(&mut data);
	data
}

/// A `Shielded.transact` extrinsic without signer: a general transaction (extrinsic version 5)
/// authorized by its proof. Its explicit transaction extension data are an immortal era, nonce 0,
/// tip 0 and no metadata hash.
pub fn transact_extrinsic(pallet: u8, call: u8, transaction: &ShieldedTransaction) -> Vec<u8> {
	// Version 5, general transaction; extension version 0; era, nonce, tip, metadata hash mode.
	let mut body = vec![0b0100_0000 | 5, 0, 0, 0, 0, 0];
	body.extend(call_data(pallet, call, transaction));
	let mut extrinsic = codec::Compact(body.len() as u32).encode();
	extrinsic.extend(body);
	extrinsic
}

/// A `Shielded.Transacted` event.
#[derive(Clone, Debug)]
pub struct Transacted {
	/// Position of the first commitment in the tree.
	pub position: u64,
	pub nullifiers: [Digest; 2],
	pub commitments: [Digest; 2],
	pub notes: [Vec<u8>; 2],
}

/// A note of the wallet.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct OwnedNote {
	pub note: Note,
	pub position: u64,
	pub nullifier: Digest,
	/// Spent on chain.
	pub spent: bool,
	/// Spent by a transaction built by the wallet, not yet seen on chain.
	pub pending: bool,
}

/// A shielded account: its key, the note tree and its notes.
pub struct Wallet {
	key: SpendingKey,
	address: Address,
	tree: Tree,
	notes: Vec<OwnedNote>,
}

impl Wallet {
	pub fn new(key: SpendingKey) -> Self {
		let address = key.address();
		Self { key, address, tree: Tree::default(), notes: Vec::new() }
	}

	pub fn address(&self) -> &Address {
		&self.address
	}

	/// Number of commitments ingested: the position of the next one.
	pub fn position(&self) -> u64 {
		self.tree.len()
	}

	/// Root of the note tree.
	pub fn root(&self) -> Digest {
		self.tree.root()
	}

	pub fn notes(&self) -> &[OwnedNote] {
		&self.notes
	}

	/// Spendable balance: unspent notes not spent by a pending transaction.
	pub fn balance(&self) -> u64 {
		self.spendable().map(|n| n.note.value).sum()
	}

	/// Balance spent by transactions not yet seen on chain.
	pub fn pending(&self) -> u64 {
		self.notes.iter().filter(|n| !n.spent && n.pending).map(|n| n.note.value).sum()
	}

	fn spendable(&self) -> impl Iterator<Item = &OwnedNote> {
		self.notes.iter().filter(|n| !n.spent && !n.pending && n.note.value > 0)
	}

	/// Ingest the next `Shielded.Transacted` event of the chain. Returns the notes it pays to this
	/// wallet.
	pub fn ingest(&mut self, event: &Transacted) -> Result<Vec<Note>, Error> {
		if event.position != self.tree.len() {
			return Err(Error::OutOfOrder { expected: self.tree.len(), got: event.position });
		}
		let mut received = Vec::new();
		for (commitment, encrypted) in event.commitments.iter().zip(&event.notes) {
			let position = self.tree.append(*commitment);
			if let Some(note) = decrypt(encrypted, commitment, &self.key) {
				let nullifier = nullifier(&self.key.nullifier_key(), commitment);
				self.notes.push(OwnedNote { note, position, nullifier, spent: false, pending: false });
				received.push(note);
			}
		}
		for spent in &event.nullifiers {
			if let Some(note) = self.notes.iter_mut().find(|n| n.nullifier == *spent) {
				note.spent = true;
				note.pending = false;
			}
		}
		Ok(received)
	}

	/// Make the notes of transactions that were not included spendable again.
	pub fn release_pending(&mut self) {
		for note in &mut self.notes {
			note.pending = false;
		}
	}

	/// Mark the notes `transaction` spends as pending, once it is submitted.
	pub fn mark_pending(&mut self, transaction: &ShieldedTransaction) {
		for note in &mut self.notes {
			if transaction.nullifiers.contains(&note.nullifier) {
				note.pending = true;
			}
		}
	}

	/// The notes to spend for `amount`: one note if one is enough, the smallest such; otherwise
	/// the two largest.
	fn select(&self, amount: u64) -> Result<Vec<OwnedNote>, Error> {
		let mut notes: Vec<&OwnedNote> = self.spendable().collect();
		let available: u64 = notes.iter().map(|n| n.note.value).sum();
		if available < amount {
			return Err(Error::InsufficientFunds { available, required: amount });
		}
		notes.sort_by_key(|n| n.note.value);
		if let Some(one) = notes.iter().find(|n| n.note.value >= amount) {
			return Ok(vec![(*one).clone()]);
		}
		let two: Vec<OwnedNote> = notes.iter().rev().take(2).map(|n| (*n).clone()).collect();
		let max: u64 = two.iter().map(|n| n.note.value).sum();
		if max < amount {
			return Err(Error::TooManyNotes { max });
		}
		Ok(two)
	}

	fn input(&self, owned: &OwnedNote) -> Result<InputWitness, Error> {
		let path = self.tree.path(owned.position).ok_or_else(|| Error::State("note outside the tree".into()))?;
		Ok(InputWitness { nk: self.key.nullifier_key(), note: owned.note, path })
	}

	/// A dummy input: a note of value 0 owned by a fresh key.
	fn dummy_input() -> InputWitness {
		let key = SpendingKey::generate();
		InputWitness {
			nk: key.nullifier_key(),
			note: Note { value: 0, tag: key.tag(), rho: Digest::ZERO, r: random_randomness() },
			path: MerklePath { position: 0, siblings: vec![Digest::ZERO; TREE_DEPTH] },
		}
	}

	/// Inputs paying `amount`, padded with a dummy input, and the change.
	fn inputs(&self, amount: u64) -> Result<([InputWitness; 2], u64), Error> {
		let selected = self.select(amount)?;
		let total: u64 = selected.iter().map(|n| n.note.value).sum();
		let mut inputs = selected.iter().map(|n| self.input(n)).collect::<Result<Vec<_>, _>>()?;
		while inputs.len() < 2 {
			inputs.push(Self::dummy_input());
		}
		let inputs: [InputWitness; 2] = inputs.try_into().map_err(|_| Error::State("two inputs".into()))?;
		Ok((inputs, total - amount))
	}

	/// Build and prove a transaction.
	#[allow(clippy::too_many_arguments)]
	fn build(
		&self,
		inputs: [InputWitness; 2],
		outputs: [(Address, u64); 2],
		value_in: u64,
		value_out: u64,
		fee: u64,
		recipient: Option<[u8; 32]>,
		source: Option<[u8; 32]>,
	) -> Result<ShieldedTransaction, Error> {
		if [value_in, value_out, fee, outputs[0].1, outputs[1].1].iter().any(|v| *v >= MAX_VALUE) {
			return Err(Error::AmountOutOfRange);
		}
		let nullifiers: [Digest; 2] = core::array::from_fn(|i| nullifier(&inputs[i].nk, &inputs[i].note.commitment()));
		let notes: [Note; 2] = core::array::from_fn(|j| Note {
			value: outputs[j].1,
			tag: outputs[j].0.tag,
			rho: output_rho(&nullifiers, j as u64),
			r: random_randomness(),
		});
		let mut encrypted = Vec::with_capacity(2);
		for (note, (address, _)) in notes.iter().zip(&outputs) {
			let bytes = encrypt(note, address).ok_or(Error::InvalidAddress)?;
			encrypted.push(<EncryptedNote>::try_from(bytes).map_err(|_| Error::State("encrypted note size".into()))?);
		}
		let mut transaction = ShieldedTransaction {
			anchor: self.tree.root(),
			nullifiers,
			commitments: notes.map(|n| n.commitment()),
			value_in,
			value_out,
			fee,
			recipient,
			notes: encrypted.try_into().map_err(|_| Error::State("two notes".into()))?,
			proof: Vec::new(),
		};
		let witness = Witness {
			inputs,
			outputs: notes.map(|note| OutputWitness { note }),
			anchor: transaction.anchor,
			value_in,
			value_out,
			fee,
			binding: transaction.binding(source.as_ref()),
		};
		transaction.proof = prove(&witness).map_err(|e| Error::Proof(format!("{e:?}")))?.0;
		Ok(transaction)
	}

	/// Move `value` planck from the transparent account `source` into this shielded account. Submit
	/// it with `Shielded.shield`, signed by `source`.
	pub fn shield(&self, source: [u8; 32], value: u64) -> Result<ShieldedTransaction, Error> {
		if value == 0 {
			return Err(Error::AmountOutOfRange);
		}
		let outputs = [(self.address.clone(), value), (SpendingKey::generate().address(), 0)];
		self.build([Self::dummy_input(), Self::dummy_input()], outputs, value, 0, 0, None, Some(source))
	}

	/// Send `value` planck to the shielded address `to`, paying `fee` from the pool. Submit it
	/// without signer ([`transact_extrinsic`]).
	pub fn send(&self, to: &Address, value: u64, fee: u64) -> Result<ShieldedTransaction, Error> {
		let required = value.checked_add(fee).ok_or(Error::AmountOutOfRange)?;
		let (inputs, change) = self.inputs(required)?;
		self.build(inputs, [(to.clone(), value), (self.address.clone(), change)], 0, 0, fee, None, None)
	}

	/// Send `value` planck from the pool to the transparent account `recipient`, paying `fee` from
	/// the pool. Submit it without signer ([`transact_extrinsic`]).
	pub fn unshield(&self, recipient: [u8; 32], value: u64, fee: u64) -> Result<ShieldedTransaction, Error> {
		if value == 0 {
			return Err(Error::AmountOutOfRange);
		}
		let required = value.checked_add(fee).ok_or(Error::AmountOutOfRange)?;
		let (inputs, change) = self.inputs(required)?;
		let outputs = [(self.address.clone(), change), (SpendingKey::generate().address(), 0)];
		self.build(inputs, outputs, 0, value, fee, Some(recipient), None)
	}

	/// The tree and the notes, to store and restore with [`Wallet::restore`]. The key is not
	/// included: it derives from the account.
	pub fn state(&self) -> Vec<u8> {
		let leaves: Vec<Digest> = (0..self.tree.len()).filter_map(|i| self.tree.leaf(i)).collect();
		(STATE_VERSION, leaves, &self.notes).encode()
	}

	/// A wallet of `key` from a state of [`Wallet::state`].
	pub fn restore(key: SpendingKey, state: &[u8]) -> Result<Self, Error> {
		let (version, leaves, notes) =
			<(u8, Vec<Digest>, Vec<OwnedNote>)>::decode(&mut &state[..]).map_err(|e| Error::State(e.to_string()))?;
		if version != STATE_VERSION {
			return Err(Error::State(format!("version {version}")));
		}
		let mut wallet = Self::new(key);
		for leaf in leaves {
			wallet.tree.append(leaf);
		}
		wallet.notes = notes;
		Ok(wallet)
	}
}

#[cfg(test)]
mod tests;
