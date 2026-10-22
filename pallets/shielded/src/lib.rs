//! # Shielded pool pallet
//!
//! Private transfers of TESS. Value in the pool lives in notes: the chain only stores their
//! commitments in an append-only Merkle tree, and the nullifiers of spent notes. A transaction
//! spends two notes and creates two notes; a zero-knowledge STARK proves that it is valid
//! without revealing the notes, their owners or their values. See `docs/privacy.md`.
//!
//! Two calls:
//!
//! - [`Pallet::transact`] is submitted without signer, so that it is linked to no account. It
//!   cannot take value from a transparent account, may pay `value_out` to a transparent recipient,
//!   and pays its `fee` from the pool to the block author. It is authorized by its proof: the
//!   proof is verified when the transaction enters the pool and when the block is imported.
//! - [`Pallet::shield`] is signed by the account paying `value_in` into the pool.
//!
//! Amounts moving in and out of the pool are burned and minted: the total supply is the total
//! issuance of the balances pallet plus [`PoolBalance`]. The pool balance can never become
//! negative, so that a flaw in the proof system could not create TESS outside the pool.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{traits::ConstU32, BoundedVec};
use scale_info::TypeInfo;
use tesserax_shielded::{
	proof::{PublicInputs, MAX_PROOF_SIZE},
	Digest, EncryptedNote, MAX_VALUE,
};

/// An encoded STARK proof.
pub type Proof = BoundedVec<u8, ConstU32<{ MAX_PROOF_SIZE as u32 }>>;

/// Context of the binding hash.
const BINDING_CONTEXT: &[u8] = b"tesserax-shielded-binding";

/// A shielded transaction: public inputs, encrypted output notes and proof.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub struct ShieldedTransaction<AccountId> {
	/// Root of the note tree the inputs belong to: one of the recent roots.
	pub anchor: Digest,
	/// Nullifiers of the two spent notes.
	pub nullifiers: [Digest; 2],
	/// Commitments of the two created notes.
	pub commitments: [Digest; 2],
	/// Amount paid into the pool by the signer of [`Pallet::shield`].
	pub value_in: u64,
	/// Amount paid from the pool to `recipient`.
	pub value_out: u64,
	/// Fee paid from the pool to the block author.
	pub fee: u64,
	/// Transparent recipient of `value_out`; set if and only if `value_out` is not zero.
	pub recipient: Option<AccountId>,
	/// The created notes, encrypted for their recipients.
	pub notes: [EncryptedNote; 2],
	/// Proof of validity.
	pub proof: Proof,
}

impl<AccountId: Encode> ShieldedTransaction<AccountId> {
	/// Hash of the data the proof commits to besides its public inputs: the account paying
	/// `value_in`, the recipient of `value_out` and the encrypted notes. A relayer can neither
	/// redirect the funds nor replace the ciphertexts.
	pub fn binding(&self, source: Option<&AccountId>) -> Digest {
		let data = (BINDING_CONTEXT, source, &self.recipient, &self.notes).encode();
		Digest::from_bytes_reduced(&sp_io::hashing::blake2_256(&data))
	}

	/// Public inputs of the proof.
	pub fn public_inputs(&self, source: Option<&AccountId>) -> PublicInputs {
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
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use alloc::{boxed::Box, vec};
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate},
			tokens::{DepositConsequence, Fortitude, Precision, Preservation, Provenance},
			FindAuthor,
		},
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::{CheckedSub, Saturating};
	use tesserax_shielded::tree::Frontier;

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The native currency.
		type Currency: Mutate<Self::AccountId, Balance: From<u64>>;

		/// Author of the current block, who receives the fees of private transactions.
		type FindAuthor: FindAuthor<Self::AccountId>;

		/// Number of recent tree roots accepted as anchors.
		#[pallet::constant]
		type RootHistory: Get<u32>;

		/// Minimum fee of a transaction submitted without signer, in planck.
		#[pallet::constant]
		type MinimumFee: Get<u64>;

		/// Number of blocks a transaction stays in the pool.
		#[pallet::constant]
		type Longevity: Get<u64>;

		/// Weights of the calls.
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Nullifiers of the spent notes.
	#[pallet::storage]
	pub type Nullifiers<T: Config> = StorageMap<_, Blake2_128Concat, Digest, (), OptionQuery>;

	/// Frontier of the note commitment tree.
	#[pallet::storage]
	pub type Tree<T: Config> = StorageValue<_, Frontier, ValueQuery>;

	/// Recent roots of the note tree, oldest first: the root at the end of each of the last
	/// blocks that changed the tree.
	#[pallet::storage]
	pub type Roots<T: Config> = StorageValue<_, BoundedVec<Digest, T::RootHistory>, ValueQuery>;

	/// Block of the last entry of [`Roots`].
	#[pallet::storage]
	pub type RootsUpdatedAt<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

	/// Total value of the notes in the pool.
	#[pallet::storage]
	pub type PoolBalance<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		#[serde(skip)]
		pub _config: core::marker::PhantomData<T>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			let root = Frontier::default().root();
			Roots::<T>::put(BoundedVec::truncate_from(vec![root]));
		}
	}

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A shielded transaction was applied. The created notes are at `position` and
		/// `position + 1` in the tree.
		Transacted {
			nullifiers: [Digest; 2],
			commitments: [Digest; 2],
			position: u64,
			notes: [EncryptedNote; 2],
			value_in: u64,
			value_out: u64,
			fee: u64,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// The anchor is not a recent root of the note tree.
		UnknownAnchor,
		/// A note was already spent.
		NullifierSpent,
		/// The transaction spends the same note twice.
		DuplicateNullifier,
		/// The proof is invalid.
		InvalidProof,
		/// An amount is not below `2^60`.
		AmountOutOfRange,
		/// The fee is below the minimum.
		FeeTooLow,
		/// The recipient is missing, or set without `value_out`.
		InvalidRecipient,
		/// The recipient cannot receive `value_out`.
		CannotReceive,
		/// A transaction without signer cannot take value from an account.
		UnexpectedValueIn,
		/// A shielding transaction must have a positive `value_in`.
		NothingToShield,
		/// The note tree is full.
		TreeFull,
		/// The pool does not hold enough value.
		PoolUnderflow,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			assert!(T::RootHistory::get() > 0, "at least one root must be accepted");
			assert!(T::MinimumFee::get() < MAX_VALUE, "the minimum fee must be a valid amount");
		}

		#[cfg(feature = "try-runtime")]
		fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
			let roots = Roots::<T>::get();
			ensure!(roots.last() == Some(&Tree::<T>::get().root()), "last root is the tree root");
			Ok(())
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Apply a shielded transaction submitted without signer.
		///
		/// The transaction is authorized by its proof, checked with the rest of the
		/// transaction when it enters the pool and when the block is imported: once authorized,
		/// the call cannot fail, so that every included transaction pays its fee.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::transact())]
		#[pallet::authorize(Pallet::<T>::authorize_transact)]
		#[pallet::weight_of_authorize(T::WeightInfo::authorize_transact())]
		pub fn transact(
			origin: OriginFor<T>,
			transaction: Box<ShieldedTransaction<T::AccountId>>,
		) -> DispatchResult {
			ensure_authorized(origin)?;
			Self::check(&transaction, None, false)?;
			Self::apply(*transaction)
		}

		/// Move `value_in` from the signer into the pool, with a shielded transaction bound to
		/// the signer.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::authorize_transact()
			.saturating_add(T::WeightInfo::transact())
			.saturating_add(T::WeightInfo::shield_burn()))]
		pub fn shield(
			origin: OriginFor<T>,
			transaction: Box<ShieldedTransaction<T::AccountId>>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			ensure!(transaction.value_in > 0, Error::<T>::NothingToShield);
			Self::check(&transaction, Some(&who), true)?;
			T::Currency::burn_from(
				&who,
				transaction.value_in.into(),
				Preservation::Preserve,
				Precision::Exact,
				Fortitude::Polite,
			)?;
			Self::apply(*transaction)
		}
	}

	impl<T: Config> Pallet<T> {
		/// Authorization of [`Pallet::transact`].
		#[allow(clippy::borrowed_box)]
		pub fn authorize_transact(
			_source: TransactionSource,
			transaction: &Box<ShieldedTransaction<T::AccountId>>,
		) -> TransactionValidityWithRefund {
			Self::check(transaction, None, true).map_err(|error| match error {
				Error::<T>::NullifierSpent => InvalidTransaction::Stale,
				Error::<T>::InvalidProof => InvalidTransaction::BadProof,
				Error::<T>::FeeTooLow => InvalidTransaction::Payment,
				error => InvalidTransaction::Custom(error.encode().first().copied().unwrap_or(0)),
			})?;
			let validity = ValidTransaction::with_tag_prefix("Shielded")
				.priority(transaction.fee)
				.and_provides(transaction.nullifiers[0])
				.and_provides(transaction.nullifiers[1])
				.longevity(T::Longevity::get())
				.propagate(true)
				.build()?;
			Ok((validity, Weight::zero()))
		}

		/// Check `transaction` against the current state. `source` is the signer paying
		/// `value_in`, if any.
		pub fn check(
			transaction: &ShieldedTransaction<T::AccountId>,
			source: Option<&T::AccountId>,
			verify_proof: bool,
		) -> Result<(), Error<T>> {
			let t = transaction;
			ensure!(
				t.value_in < MAX_VALUE && t.value_out < MAX_VALUE && t.fee < MAX_VALUE,
				Error::<T>::AmountOutOfRange
			);
			match source {
				None => {
					ensure!(t.value_in == 0, Error::<T>::UnexpectedValueIn);
					ensure!(t.fee >= T::MinimumFee::get(), Error::<T>::FeeTooLow);
				},
				Some(_) => ensure!(t.value_in > 0, Error::<T>::NothingToShield),
			}
			match &t.recipient {
				None => ensure!(t.value_out == 0, Error::<T>::InvalidRecipient),
				Some(recipient) => {
					ensure!(t.value_out > 0, Error::<T>::InvalidRecipient);
					ensure!(
						T::Currency::can_deposit(recipient, t.value_out.into(), Provenance::Minted) ==
							DepositConsequence::Success,
						Error::<T>::CannotReceive
					);
				},
			}
			ensure!(t.nullifiers[0] != t.nullifiers[1], Error::<T>::DuplicateNullifier);
			for nullifier in &t.nullifiers {
				ensure!(!Nullifiers::<T>::contains_key(nullifier), Error::<T>::NullifierSpent);
			}
			ensure!(Roots::<T>::get().contains(&t.anchor), Error::<T>::UnknownAnchor);
			ensure!(Tree::<T>::get().size + 2 <= 1u64 << tesserax_shielded::tree::TREE_DEPTH, Error::<T>::TreeFull);
			PoolBalance::<T>::get()
				.saturating_add(t.value_in.into())
				.checked_sub(&BalanceOf::<T>::from(t.value_out).saturating_add(t.fee.into()))
				.ok_or(Error::<T>::PoolUnderflow)?;
			if verify_proof {
				ensure!(
					tesserax_shielded::proof::verify(&t.proof, &t.public_inputs(source)),
					Error::<T>::InvalidProof
				);
			}
			Ok(())
		}

		/// Apply a checked transaction.
		fn apply(transaction: ShieldedTransaction<T::AccountId>) -> DispatchResult {
			let t = transaction;
			for nullifier in t.nullifiers {
				Nullifiers::<T>::insert(nullifier, ());
			}

			let mut tree = Tree::<T>::get();
			let position = tree.append(t.commitments[0]).ok_or(Error::<T>::TreeFull)?;
			tree.append(t.commitments[1]).ok_or(Error::<T>::TreeFull)?;
			Self::record_root(tree.root());
			Tree::<T>::put(tree);

			PoolBalance::<T>::try_mutate(|pool| -> DispatchResult {
				*pool = pool
					.saturating_add(t.value_in.into())
					.checked_sub(&BalanceOf::<T>::from(t.value_out).saturating_add(t.fee.into()))
					.ok_or(Error::<T>::PoolUnderflow)?;
				Ok(())
			})?;
			if let Some(recipient) = &t.recipient {
				T::Currency::mint_into(recipient, t.value_out.into())?;
			}
			if t.fee > 0 {
				// Without a known author, or if the author cannot receive it, the fee is burned.
				if let Some(author) = Self::author() {
					let _ = T::Currency::mint_into(&author, t.fee.into());
				}
			}

			Self::deposit_event(Event::Transacted {
				nullifiers: t.nullifiers,
				commitments: t.commitments,
				position,
				notes: t.notes,
				value_in: t.value_in,
				value_out: t.value_out,
				fee: t.fee,
			});
			Ok(())
		}

		/// Record `root` as the root at the end of the current block.
		fn record_root(root: Digest) {
			let now = frame_system::Pallet::<T>::block_number();
			let updated_now = RootsUpdatedAt::<T>::get() == now;
			Roots::<T>::mutate(|roots| match roots.last_mut() {
				Some(last) if updated_now => *last = root,
				_ => {
					if roots.is_full() {
						roots.remove(0);
					}
					let _ = roots.try_push(root);
				},
			});
			RootsUpdatedAt::<T>::put(now);
		}

		/// Author of the current block.
		fn author() -> Option<T::AccountId> {
			let digest = frame_system::Pallet::<T>::digest();
			T::FindAuthor::find_author(digest.logs.iter().filter_map(|item| item.as_pre_runtime()))
		}
	}
}
