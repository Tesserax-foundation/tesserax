//! Weights of `pallet_shielded`, measured by `scripts/benchmark.sh`.
//!
//! Benchmark CLI 62.0.0, 2026-10-22, on `Apple M1`: 50 steps, 20 repeats,
//! Compiled WebAssembly. Measure them again on the reference hardware before a release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_shielded`.
pub trait WeightInfo {
	fn authorize_transact() -> Weight;
	fn transact() -> Weight;
	fn shield_burn() -> Weight;
}

/// Weights measured on the runtime, with the database weights of the runtime.
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
		// Minimum execution time: 10_643_000_000 picoseconds.
		Weight::from_parts(11_838_000_000, 6036)
			.saturating_add(T::DbWeight::get().reads(5_u64))
	}
	/// Storage: `Shielded::Nullifiers` (r:2 w:2)
	/// Proof: `Shielded::Nullifiers` (`max_values`: None, `max_size`: Some(48), added: 2523, mode: `MaxEncodedLen`)
	/// Storage: `Shielded::Roots` (r:1 w:1)
	/// Proof: `Shielded::Roots` (`max_values`: Some(1), `max_size`: Some(3202), added: 3697, mode: `MaxEncodedLen`)
	/// Storage: `Shielded::Tree` (r:1 w:1)
	/// Proof: `Shielded::Tree` (`max_values`: Some(1), `max_size`: Some(1032), added: 1527, mode: `MaxEncodedLen`)
	/// Storage: `Shielded::PoolBalance` (r:1 w:1)
	/// Proof: `Shielded::PoolBalance` (`max_values`: Some(1), `max_size`: Some(16), added: 511, mode: `MaxEncodedLen`)
	/// Storage: `Shielded::RootsUpdatedAt` (r:1 w:1)
	/// Proof: `Shielded::RootsUpdatedAt` (`max_values`: Some(1), `max_size`: Some(4), added: 499, mode: `MaxEncodedLen`)
	fn transact() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `4324`
		//  Estimated: `6036`
		// Minimum execution time: 383_000_000 picoseconds.
		Weight::from_parts(385_000_000, 6036)
			.saturating_add(T::DbWeight::get().reads(6_u64))
			.saturating_add(T::DbWeight::get().writes(6_u64))
	}
	fn shield_burn() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 19_000_000 picoseconds.
		Weight::from_parts(20_000_000, 0)
	}
}

/// The same weights with the database weights of RocksDB, for tests.
impl WeightInfo for () {
	fn authorize_transact() -> Weight {
		Weight::from_parts(11_838_000_000, 6036)
			.saturating_add(RocksDbWeight::get().reads(5_u64))
	}
	fn transact() -> Weight {
		Weight::from_parts(385_000_000, 6036)
			.saturating_add(RocksDbWeight::get().reads(6_u64))
			.saturating_add(RocksDbWeight::get().writes(6_u64))
	}
	fn shield_burn() -> Weight {
		Weight::from_parts(20_000_000, 0)
	}
}
