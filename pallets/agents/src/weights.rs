//! Weights of `pallet_agents`, measured by `scripts/benchmark.sh`.
//!
//! Benchmark CLI 62.0.0, 2026-10-22, on `Apple M1`: 50 steps, 20 repeats,
//! Compiled WebAssembly. Measure them again on the reference hardware before a release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_agents`.
pub trait WeightInfo {
	fn authorize(t: u32, ) -> Weight;
	fn revoke() -> Weight;
	fn execute() -> Weight;
}

/// Weights measured on the runtime, with the database weights of the runtime.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Agents::Delegations` (r:1 w:1)
	/// Proof: `Agents::Delegations` (`max_values`: None, `max_size`: Some(1200), added: 3675, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// The range of component `t` is `[0, 32]`.
	fn authorize(t: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `109`
		//  Estimated: `4665`
		// Minimum execution time: 52_000_000 picoseconds.
		Weight::from_parts(57_271_630, 4665)
			// Standard Error: 2_246
			.saturating_add(Weight::from_parts(37_284, 0).saturating_mul(t.into()))
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `Agents::Delegations` (r:1 w:1)
	/// Proof: `Agents::Delegations` (`max_values`: None, `max_size`: Some(1200), added: 3675, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn revoke() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1389`
		//  Estimated: `4665`
		// Minimum execution time: 44_000_000 picoseconds.
		Weight::from_parts(48_000_000, 4665)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `Agents::Delegations` (r:1 w:1)
	/// Proof: `Agents::Delegations` (`max_values`: None, `max_size`: Some(1200), added: 3675, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	fn execute() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1435`
		//  Estimated: `6196`
		// Minimum execution time: 60_000_000 picoseconds.
		Weight::from_parts(62_000_000, 6196)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
}

/// The same weights with the database weights of RocksDB, for tests.
impl WeightInfo for () {
	fn authorize(t: u32, ) -> Weight {
		Weight::from_parts(57_271_630, 4665)
			.saturating_add(Weight::from_parts(37_284, 0).saturating_mul(t.into()))
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn revoke() -> Weight {
		Weight::from_parts(48_000_000, 4665)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn execute() -> Weight {
		Weight::from_parts(62_000_000, 6196)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
}
