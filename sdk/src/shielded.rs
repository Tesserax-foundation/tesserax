//! Private transactions with the shielded pool (`docs/privacy.md`), with the `shielded` feature.
//!
//! A [`ShieldedAccount`] is the shielded side of an account: its key derives from the account's
//! seed, so its recovery phrase restores it. It follows the chain block after block from its last
//! synchronization, and its state can be stored and restored.
//!
//! ```no_run
//! # async fn example() -> Result<(), tesserax_sdk::Error> {
//! use tesserax_sdk::{shielded::ShieldedAccount, Amount, Client, Keypair};
//!
//! let client = Client::connect("wss://testnet-rpc.tesserax.app:21207").await?;
//! let alice = Keypair::from_uri("<24 words>")?;
//! let mut private = ShieldedAccount::new(&alice);
//! private.sync(&client).await?;
//! private.shield(&client, &alice, Amount::from_tess("10")?).await?;
//! let bob = tesserax_sdk::shielded::decode_address("zte...")?;
//! private.send(&client, &bob, Amount::from_tess("4")?).await?;
//! # Ok(()) }
//! ```

use crate::{client::TxOutcome, Amount, Client, Error, Keypair, Result};
use codec::{Decode, Encode};
pub use tesserax_shielded_wallet::{
	decode_address as decode, encode_address, Address, ShieldedTransaction, Transacted, Wallet,
};
use tesserax_shielded_wallet::{spending_key, transact_extrinsic, Digest, EncryptedNote};

/// A shielded address from its text, `zte...`.
pub fn decode_address(text: &str) -> Result<Address> {
	decode(text).map_err(|e| Error::InvalidAddress(e.to_string()))
}

fn shielded(error: tesserax_shielded_wallet::Error) -> Error {
	Error::InvalidInput(error.to_string())
}

/// Fields of `Shielded.Transacted`, as the runtime encodes them.
#[derive(Decode)]
struct TransactedEvent {
	nullifiers: [Digest; 2],
	commitments: [Digest; 2],
	position: u64,
	notes: [EncryptedNote; 2],
	_value_in: u64,
	_value_out: u64,
	_fee: u64,
}

/// A call of the shielded pallet with its encoded arguments.
struct ShieldedCall {
	call: &'static str,
	arguments: Vec<u8>,
}

impl subxt::tx::Payload for ShieldedCall {
	fn encode_call_data_to(
		&self,
		metadata: &subxt::Metadata,
		out: &mut Vec<u8>,
	) -> core::result::Result<(), subxt::ext::subxt_core::Error> {
		use subxt::ext::subxt_core::error::{Error as CoreError, MetadataError};
		let pallet = metadata
			.pallet_by_name("Shielded")
			.ok_or_else(|| CoreError::Metadata(MetadataError::PalletNameNotFound("Shielded".into())))?;
		let call = pallet
			.call_variant_by_name(self.call)
			.ok_or_else(|| CoreError::Metadata(MetadataError::CallNameNotFound(self.call.into())))?
			.index;
		let pallet = pallet.index();
		out.extend([pallet, call]);
		out.extend(&self.arguments);
		Ok(())
	}
}

/// Indices of the shielded pallet and of `call` in the metadata.
fn indices(metadata: &subxt::Metadata, call: &str) -> Result<(u8, u8)> {
	let pallet = metadata.pallet_by_name("Shielded").ok_or_else(|| Error::NotFound("the shielded pallet".into()))?;
	let variant = pallet.call_variant_by_name(call).ok_or_else(|| Error::NotFound(format!("Shielded.{call}")))?;
	Ok((pallet.index(), variant.index))
}

impl Client {
	/// The smallest fee of a private transaction, in planck.
	pub async fn shielded_minimum_fee(&self) -> Result<u64> {
		Ok(self.api().constants().at(&crate::runtime::constants().shielded().minimum_fee())?)
	}

	/// The `Shielded.Transacted` events of the block `number` of the best chain, and its hash.
	pub async fn shielded_events(&self, number: u32) -> Result<Option<([u8; 32], Vec<Transacted>)>> {
		let Some(hash) = self.block_hash(number).await? else { return Ok(None) };
		let mut found = Vec::new();
		for event in self.api().events().at(hash).await?.iter() {
			let event = event?;
			if event.pallet_name() != "Shielded" || event.variant_name() != "Transacted" {
				continue;
			}
			let fields = TransactedEvent::decode(&mut event.field_bytes())
				.map_err(|e| Error::InvalidInput(format!("Shielded.Transacted: {e}")))?;
			found.push(Transacted {
				position: fields.position,
				nullifiers: fields.nullifiers,
				commitments: fields.commitments,
				notes: fields.notes.map(|n| n.to_vec()),
			});
		}
		Ok(Some((hash.0, found)))
	}

	/// Submit `transaction`, which moves TESS of `signer` into the pool, signed by it.
	pub async fn submit_shield(&self, signer: &Keypair, transaction: &ShieldedTransaction) -> Result<TxOutcome> {
		self.submit(&ShieldedCall { call: "shield", arguments: transaction.encode() }, signer).await
	}

	/// Submit a private transfer or an unshield, without signer: its proof authorizes it.
	pub async fn submit_shielded(&self, transaction: &ShieldedTransaction) -> Result<TxOutcome> {
		let (pallet, call) = indices(&self.api().metadata(), "transact")?;
		let bytes = transact_extrinsic(pallet, call, transaction);
		let progress =
			subxt::tx::SubmittableTransaction::from_bytes(self.api().clone(), bytes).submit_and_watch().await?;
		Ok(self.included(progress).await?.0)
	}
}

/// What a synchronization found.
#[derive(Clone, Debug, Default)]
pub struct Synced {
	/// Number of the last block read.
	pub block: u32,
	/// Value of the notes received, in planck.
	pub received: u64,
}

/// The shielded side of an account.
pub struct ShieldedAccount {
	seed: [u8; 32],
	wallet: Wallet,
	/// Last block read, and its hash, to notice a reorganization.
	synced: Option<(u32, [u8; 32])>,
}

impl ShieldedAccount {
	/// The shielded account of `keypair`, not synchronized.
	pub fn new(keypair: &Keypair) -> Self {
		let seed = keypair.seed();
		Self { seed, wallet: Wallet::new(spending_key(&seed)), synced: None }
	}

	/// The shielded account of `keypair` from a state of [`ShieldedAccount::state`].
	pub fn restore(keypair: &Keypair, state: &[u8]) -> Result<Self> {
		let (synced, wallet) =
			<(Option<(u32, [u8; 32])>, Vec<u8>)>::decode(&mut &state[..]).map_err(|e| Error::Storage(e.to_string()))?;
		let seed = keypair.seed();
		let wallet = Wallet::restore(spending_key(&seed), &wallet).map_err(|e| Error::Storage(e.to_string()))?;
		Ok(Self { seed, wallet, synced })
	}

	/// The state to store: the note tree, the notes and the last block read.
	pub fn state(&self) -> Vec<u8> {
		(self.synced, self.wallet.state()).encode()
	}

	/// Its shielded address, `zte...`.
	pub fn address(&self) -> String {
		encode_address(self.wallet.address())
	}

	/// Spendable balance.
	pub fn balance(&self) -> Amount {
		Amount(self.wallet.balance().into())
	}

	/// Balance spent by transactions not yet seen in a block.
	pub fn pending(&self) -> Amount {
		Amount(self.wallet.pending().into())
	}

	/// The wallet, for its notes.
	pub fn wallet(&self) -> &Wallet {
		&self.wallet
	}

	/// Read the blocks of the best chain since the last synchronization. After a reorganization
	/// below the last block read, the account is read again from the genesis.
	pub async fn sync(&mut self, client: &Client) -> Result<Synced> {
		if let Some((number, hash)) = self.synced {
			if client.block_hash(number).await?.map(|h| h.0) != Some(hash) {
				self.wallet = Wallet::new(spending_key(&self.seed));
				self.synced = None;
			}
		}
		let best = client.best_block_number().await?;
		let mut synced = Synced { block: self.synced.map_or(0, |(n, _)| n), received: 0 };
		let start = self.synced.map_or(0, |(n, _)| n + 1);
		for number in start..=best {
			let Some((hash, events)) = client.shielded_events(number).await? else { break };
			for event in &events {
				let notes = self.wallet.ingest(event).map_err(shielded)?;
				synced.received += notes.iter().map(|n| n.value).sum::<u64>();
			}
			self.synced = Some((number, hash));
			synced.block = number;
		}
		Ok(synced)
	}

	fn planck(amount: Amount) -> Result<u64> {
		u64::try_from(amount.0).map_err(|_| Error::InvalidAmount("too large for a note".into()))
	}

	/// Move `amount` of `signer`'s transparent balance into this shielded account.
	pub async fn shield(&mut self, client: &Client, signer: &Keypair, amount: Amount) -> Result<TxOutcome> {
		let transaction = self.wallet.shield(signer.account_id().0, Self::planck(amount)?).map_err(shielded)?;
		client.submit_shield(signer, &transaction).await
	}

	/// Send `amount` privately to the shielded address `to`. The fee is the minimum fee, paid
	/// from the shielded balance.
	pub async fn send(&mut self, client: &Client, to: &Address, amount: Amount) -> Result<TxOutcome> {
		let fee = client.shielded_minimum_fee().await?;
		let transaction = self.wallet.send(to, Self::planck(amount)?, fee).map_err(shielded)?;
		self.submit(client, &transaction).await
	}

	/// Move `amount` out of the pool to the transparent account `to`. The fee is the minimum fee,
	/// paid from the shielded balance.
	pub async fn unshield(&mut self, client: &Client, to: &crate::AccountId, amount: Amount) -> Result<TxOutcome> {
		let fee = client.shielded_minimum_fee().await?;
		let transaction = self.wallet.unshield(to.0, Self::planck(amount)?, fee).map_err(shielded)?;
		self.submit(client, &transaction).await
	}

	async fn submit(&mut self, client: &Client, transaction: &ShieldedTransaction) -> Result<TxOutcome> {
		let outcome = client.submit_shielded(transaction).await?;
		self.wallet.mark_pending(transaction);
		Ok(outcome)
	}
}
