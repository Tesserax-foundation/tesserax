//! Weights of the shielded pool pallet.
//!
//! Measured with `tessera benchmark pallet --pallet pallet_shielded --extrinsic '*' --steps 2
//! --repeat 20` on the compiled WebAssembly runtime (Apple M1). Must be measured again on the
//! reference hardware before launch.

#![allow(unused_parens)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

/// Weight functions of the pallet.
pub trait WeightInfo {
	/// Verifying a proof and checking a transaction against the state.
	fn authorize_transact() -> Weight;
	/// Applying a checked transaction.
	fn transact() -> Weight;
	/// Burning the shielded amount from the signer.
	fn shield_burn() -> Weight;

	/// Checking, verifying and applying a shielding transaction.
	fn shield() -> Weight {
		Self::authorize_transact()
			.saturating_add(Self::transact())
			.saturating_add(Self::shield_burn())
	}
}

/// Weights measured with the runtime benchmarks.
pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Shielded::Nullifiers` (r:2 w:0)
	/// Proof: `Shielded::Nullifiers` (`max_values`: None, `max_size`: Some(48), added: 2523, mode: `MaxEncodedLen`)
	/// Storage: `Shielded::Roots` (r:1 w:0)
	/// Proof: `Shielded::Roots` (`max_values`: Some(1), `max_size`: Some(3202), added: 3697, mode: `MaxEncodedLen`)
	/// Storage: `Shielded::Tree` (r:1 w:0)
	/// Proof: `Shielded::Tree` (`max_values`: Some(1), `max_size`: Some(1032), added: 1527, mode: `MaxEncodedLen`)
	/// Storage: `Shielded::PoolBalance` (r:1 w:0)
	/// Proof: `Shielded::PoolBalance` (`max_values`: Some(1), `max_size`: Some(16), added: 511, mode: `MaxEncodedLen`)
	fn authorize_transact() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `4324`
		//  Estimated: `6036`
		// Minimum execution time: 10_534_000_000 picoseconds.
		Weight::from_parts(13_445_000_000, 6036).saturating_add(T::DbWeight::get().reads(5))
	}

	/// Storage: `Shielded::Nullifiers` (r:2 w:2)
	/// Storage: `Shielded::Roots` (r:1 w:1)
	/// Storage: `Shielded::Tree` (r:1 w:1)
	/// Storage: `Shielded::PoolBalance` (r:1 w:1)
	/// Storage: `Shielded::RootsUpdatedAt` (r:1 w:1)
	/// Storage: `System::Account` (r:2 w:2): recipient and block author, not covered by the
	/// benchmark and added with the cost of a balance update each.
	fn transact() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `4324`
		//  Estimated: `6036`
		// Minimum execution time: 376_000_000 picoseconds.
		Weight::from_parts(714_000_000 + 2 * BALANCE_UPDATE_REF_TIME, 6036 + 2 * ACCOUNT_PROOF_SIZE)
			.saturating_add(T::DbWeight::get().reads(8))
			.saturating_add(T::DbWeight::get().writes(8))
	}

	/// Storage: `System::Account` (r:1 w:1)
	fn shield_burn() -> Weight {
		// Minimum execution time: 18_000_000 picoseconds.
		Weight::from_parts(BALANCE_UPDATE_REF_TIME, ACCOUNT_PROOF_SIZE)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}
}

impl WeightInfo for () {
	fn authorize_transact() -> Weight {
		Weight::from_parts(13_445_000_000, 6036)
	}

	fn transact() -> Weight {
		Weight::from_parts(714_000_000 + 2 * BALANCE_UPDATE_REF_TIME, 6036 + 2 * ACCOUNT_PROOF_SIZE)
	}

	fn shield_burn() -> Weight {
		Weight::from_parts(BALANCE_UPDATE_REF_TIME, ACCOUNT_PROOF_SIZE)
	}
}

/// Execution time of a balance update, in picoseconds (measured with `shield_burn`).
const BALANCE_UPDATE_REF_TIME: u64 = 20_000_000;

/// Proof size of an account, in bytes.
const ACCOUNT_PROOF_SIZE: u64 = 3_593;
