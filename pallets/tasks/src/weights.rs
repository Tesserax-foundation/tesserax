//! Weights of `pallet_tasks`, measured by `scripts/benchmark.sh`.
//!
//! Benchmark CLI 62.0.0, 2026-10-22, on `Apple M1`: 50 steps, 20 repeats,
//! Compiled WebAssembly. Measure them again on the reference hardware before a release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_tasks`.
pub trait WeightInfo {
	fn create() -> Weight;
	fn cancel() -> Weight;
	fn accept() -> Weight;
	fn deliver() -> Weight;
	fn dispute() -> Weight;
	fn complete() -> Weight;
}

/// Weights measured on the runtime, with the database weights of the runtime.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `Tasks::NextTaskId` (r:1 w:1)
	/// Proof: `Tasks::NextTaskId` (`max_values`: Some(1), `max_size`: Some(8), added: 503, mode: `MaxEncodedLen`)
	/// Storage: `Tasks::Tasks` (r:0 w:1)
	/// Proof: `Tasks::Tasks` (`max_values`: None, `max_size`: Some(781), added: 3256, mode: `MaxEncodedLen`)
	fn create() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `109`
		//  Estimated: `3640`
		// Minimum execution time: 51_000_000 picoseconds.
		Weight::from_parts(54_000_000, 3640)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(4_u64))
	}
	/// Storage: `Tasks::Tasks` (r:1 w:1)
	/// Proof: `Tasks::Tasks` (`max_values`: None, `max_size`: Some(781), added: 3256, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn cancel() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `675`
		//  Estimated: `4246`
		// Minimum execution time: 45_000_000 picoseconds.
		Weight::from_parts(47_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `Tasks::Tasks` (r:1 w:1)
	/// Proof: `Tasks::Tasks` (`max_values`: None, `max_size`: Some(781), added: 3256, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn accept() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `675`
		//  Estimated: `4246`
		// Minimum execution time: 54_000_000 picoseconds.
		Weight::from_parts(55_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `Tasks::Tasks` (r:1 w:1)
	/// Proof: `Tasks::Tasks` (`max_values`: None, `max_size`: Some(781), added: 3256, mode: `MaxEncodedLen`)
	fn deliver() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `552`
		//  Estimated: `4246`
		// Minimum execution time: 12_000_000 picoseconds.
		Weight::from_parts(13_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Tasks::Tasks` (r:1 w:1)
	/// Proof: `Tasks::Tasks` (`max_values`: None, `max_size`: Some(781), added: 3256, mode: `MaxEncodedLen`)
	fn dispute() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `810`
		//  Estimated: `4246`
		// Minimum execution time: 12_000_000 picoseconds.
		Weight::from_parts(13_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `Tasks::Tasks` (r:1 w:1)
	/// Proof: `Tasks::Tasks` (`max_values`: None, `max_size`: Some(781), added: 3256, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:2 w:2)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:3 w:3)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `AgentRegistry::Reputations` (r:1 w:1)
	/// Proof: `AgentRegistry::Reputations` (`max_values`: None, `max_size`: Some(92), added: 2567, mode: `MaxEncodedLen`)
	fn complete() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1259`
		//  Estimated: `8799`
		// Minimum execution time: 151_000_000 picoseconds.
		Weight::from_parts(157_000_000, 8799)
			.saturating_add(T::DbWeight::get().reads(7_u64))
			.saturating_add(T::DbWeight::get().writes(7_u64))
	}
}

/// The same weights with the database weights of RocksDB, for tests.
impl WeightInfo for () {
	fn create() -> Weight {
		Weight::from_parts(54_000_000, 3640)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(4_u64))
	}
	fn cancel() -> Weight {
		Weight::from_parts(47_000_000, 4246)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn accept() -> Weight {
		Weight::from_parts(55_000_000, 4246)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn deliver() -> Weight {
		Weight::from_parts(13_000_000, 4246)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn dispute() -> Weight {
		Weight::from_parts(13_000_000, 4246)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn complete() -> Weight {
		Weight::from_parts(157_000_000, 8799)
			.saturating_add(RocksDbWeight::get().reads(7_u64))
			.saturating_add(RocksDbWeight::get().writes(7_u64))
	}
}
