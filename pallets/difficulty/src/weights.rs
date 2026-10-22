//! Weights of `pallet_difficulty`, measured by `scripts/benchmark.sh`.
//!
//! Benchmark CLI 62.0.0, 2026-10-22, on `Apple M1`: 50 steps, 20 repeats,
//! Compiled WebAssembly. Measure them again on the reference hardware before a release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_difficulty`.
pub trait WeightInfo {
	fn on_finalize() -> Weight;
}

/// Weights measured on the runtime, with the database weights of the runtime.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Timestamp::Now` (r:1 w:0)
	/// Proof: `Timestamp::Now` (`max_values`: Some(1), `max_size`: Some(8), added: 503, mode: `MaxEncodedLen`)
	/// Storage: `Difficulty::CurrentDifficulty` (r:1 w:1)
	/// Proof: `Difficulty::CurrentDifficulty` (`max_values`: Some(1), `max_size`: Some(32), added: 527, mode: `MaxEncodedLen`)
	/// Storage: `Difficulty::History` (r:1 w:1)
	/// Proof: `Difficulty::History` (`max_values`: Some(1), `max_size`: Some(4842), added: 5337, mode: `MaxEncodedLen`)
	fn on_finalize() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `5039`
		//  Estimated: `6327`
		// Minimum execution time: 34_000_000 picoseconds.
		Weight::from_parts(35_000_000, 6327)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
}

/// The same weights with the database weights of RocksDB, for tests.
impl WeightInfo for () {
	fn on_finalize() -> Weight {
		Weight::from_parts(35_000_000, 6327)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
}
