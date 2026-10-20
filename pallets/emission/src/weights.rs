//! Weights of the emission pallet.
//!
//! Measured with `tessera benchmark pallet --pallet pallet_emission --extrinsic '*' --steps 2
//! --repeat 50` on the compiled WebAssembly runtime (Apple M1). Must be measured again on the
//! reference hardware before launch.

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

/// Weight functions of the pallet.
pub trait WeightInfo {
	/// Rewarding the miner of a block.
	fn on_initialize_with_author() -> Weight;
	/// A block without author.
	fn on_initialize_without_author() -> Weight;
}

/// Weights measured with the runtime benchmarks.
pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Emission::TotalMinted` (r:1 w:1)
	/// Proof: `Emission::TotalMinted` (`max_values`: Some(1), `max_size`: Some(16), added: 511, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:1): mapping of a new account for contracts.
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn on_initialize_with_author() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `263`
		//  Estimated: `3593`
		// Minimum execution time: 32_000_000 picoseconds.
		Weight::from_parts(34_000_000, 3593)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// The digest is read from memory: `System::Digest` is whitelisted.
	fn on_initialize_without_author() -> Weight {
		// Minimum execution time: 0 picoseconds.
		Weight::from_parts(1_000_000, 0)
	}
}

impl WeightInfo for () {
	fn on_initialize_with_author() -> Weight {
		Weight::from_parts(34_000_000, 3593)
	}

	fn on_initialize_without_author() -> Weight {
		Weight::from_parts(1_000_000, 0)
	}
}
