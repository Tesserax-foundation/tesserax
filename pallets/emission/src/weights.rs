//! Weights of `pallet_emission`, measured by `scripts/benchmark.sh`.
//!
//! Benchmark CLI 62.0.0, 2026-10-22, on `Apple M1`: 50 steps, 20 repeats,
//! Compiled WebAssembly. Measure them again on the reference hardware before a release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_emission`.
pub trait WeightInfo {
	fn on_initialize_with_author() -> Weight;
	fn on_initialize_without_author() -> Weight;
}

/// Weights measured on the runtime, with the database weights of the runtime.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Emission::TotalMinted` (r:1 w:1)
	/// Proof: `Emission::TotalMinted` (`max_values`: Some(1), `max_size`: Some(16), added: 511, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:1)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn on_initialize_with_author() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `452`
		//  Estimated: `3593`
		// Minimum execution time: 34_000_000 picoseconds.
		Weight::from_parts(35_000_000, 3593)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	fn on_initialize_without_author() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 0_000 picoseconds.
		Weight::from_parts(1_000_000, 0)
	}
}

/// The same weights with the database weights of RocksDB, for tests.
impl WeightInfo for () {
	fn on_initialize_with_author() -> Weight {
		Weight::from_parts(35_000_000, 3593)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn on_initialize_without_author() -> Weight {
		Weight::from_parts(1_000_000, 0)
	}
}
