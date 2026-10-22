//! Weights of `pallet_agent_registry`, measured by `scripts/benchmark.sh`.
//!
//! Benchmark CLI 62.0.0, 2026-10-22, on `Apple M1`: 50 steps, 20 repeats,
//! Compiled WebAssembly. Measure them again on the reference hardware before a release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_agent_registry`.
pub trait WeightInfo {
	fn register() -> Weight;
	fn update_profile() -> Weight;
	fn set_owner() -> Weight;
	fn confirm_owner() -> Weight;
	fn set_active() -> Weight;
	fn deregister() -> Weight;
	fn pay() -> Weight;
	fn rate() -> Weight;
}

/// Weights measured on the runtime, with the database weights of the runtime.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `AgentRegistry::Agents` (r:1 w:1)
	/// Proof: `AgentRegistry::Agents` (`max_values`: None, `max_size`: Some(1503), added: 3978, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn register() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `146`
		//  Estimated: `4968`
		// Minimum execution time: 56_000_000 picoseconds.
		Weight::from_parts(58_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `AgentRegistry::Agents` (r:1 w:1)
	/// Proof: `AgentRegistry::Agents` (`max_values`: None, `max_size`: Some(1503), added: 3978, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn update_profile() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `304`
		//  Estimated: `4968`
		// Minimum execution time: 59_000_000 picoseconds.
		Weight::from_parts(61_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `AgentRegistry::Agents` (r:1 w:1)
	/// Proof: `AgentRegistry::Agents` (`max_values`: None, `max_size`: Some(1503), added: 3978, mode: `MaxEncodedLen`)
	fn set_owner() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1537`
		//  Estimated: `4968`
		// Minimum execution time: 15_000_000 picoseconds.
		Weight::from_parts(16_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `AgentRegistry::Agents` (r:1 w:1)
	/// Proof: `AgentRegistry::Agents` (`max_values`: None, `max_size`: Some(1503), added: 3978, mode: `MaxEncodedLen`)
	fn confirm_owner() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1537`
		//  Estimated: `4968`
		// Minimum execution time: 15_000_000 picoseconds.
		Weight::from_parts(16_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `AgentRegistry::Agents` (r:1 w:1)
	/// Proof: `AgentRegistry::Agents` (`max_values`: None, `max_size`: Some(1503), added: 3978, mode: `MaxEncodedLen`)
	fn set_active() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1537`
		//  Estimated: `4968`
		// Minimum execution time: 15_000_000 picoseconds.
		Weight::from_parts(16_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
	/// Storage: `AgentRegistry::Agents` (r:1 w:1)
	/// Proof: `AgentRegistry::Agents` (`max_values`: None, `max_size`: Some(1503), added: 3978, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn deregister() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1733`
		//  Estimated: `4968`
		// Minimum execution time: 48_000_000 picoseconds.
		Weight::from_parts(51_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `AgentRegistry::Agents` (r:1 w:0)
	/// Proof: `AgentRegistry::Agents` (`max_values`: None, `max_size`: Some(1503), added: 3978, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `AgentRegistry::Payers` (r:1 w:1)
	/// Proof: `AgentRegistry::Payers` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
	/// Storage: `AgentRegistry::Reputations` (r:1 w:1)
	/// Proof: `AgentRegistry::Reputations` (`max_values`: None, `max_size`: Some(92), added: 2567, mode: `MaxEncodedLen`)
	fn pay() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `1780`
		//  Estimated: `6196`
		// Minimum execution time: 60_000_000 picoseconds.
		Weight::from_parts(63_000_000, 6196)
			.saturating_add(T::DbWeight::get().reads(5_u64))
			.saturating_add(T::DbWeight::get().writes(4_u64))
	}
	/// Storage: `AgentRegistry::Payers` (r:1 w:1)
	/// Proof: `AgentRegistry::Payers` (`max_values`: None, `max_size`: Some(104), added: 2579, mode: `MaxEncodedLen`)
	/// Storage: `AgentRegistry::Reputations` (r:1 w:1)
	/// Proof: `AgentRegistry::Reputations` (`max_values`: None, `max_size`: Some(92), added: 2567, mode: `MaxEncodedLen`)
	fn rate() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `285`
		//  Estimated: `3569`
		// Minimum execution time: 15_000_000 picoseconds.
		Weight::from_parts(17_000_000, 3569)
			.saturating_add(T::DbWeight::get().reads(2_u64))
			.saturating_add(T::DbWeight::get().writes(2_u64))
	}
}

/// The same weights with the database weights of RocksDB, for tests.
impl WeightInfo for () {
	fn register() -> Weight {
		Weight::from_parts(58_000_000, 4968)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn update_profile() -> Weight {
		Weight::from_parts(61_000_000, 4968)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn set_owner() -> Weight {
		Weight::from_parts(16_000_000, 4968)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn confirm_owner() -> Weight {
		Weight::from_parts(16_000_000, 4968)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn set_active() -> Weight {
		Weight::from_parts(16_000_000, 4968)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
	fn deregister() -> Weight {
		Weight::from_parts(51_000_000, 4968)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn pay() -> Weight {
		Weight::from_parts(63_000_000, 6196)
			.saturating_add(RocksDbWeight::get().reads(5_u64))
			.saturating_add(RocksDbWeight::get().writes(4_u64))
	}
	fn rate() -> Weight {
		Weight::from_parts(17_000_000, 3569)
			.saturating_add(RocksDbWeight::get().reads(2_u64))
			.saturating_add(RocksDbWeight::get().writes(2_u64))
	}
}
