//! Weights of `pallet_channels`, measured by `scripts/benchmark.sh`.
//!
//! Benchmark CLI 62.0.0, 2026-10-22, on `Apple M1`: 50 steps, 20 repeats,
//! Compiled WebAssembly. Measure them again on the reference hardware before a release.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_channels`.
pub trait WeightInfo {
	fn open() -> Weight;
	fn top_up() -> Weight;
	fn claim() -> Weight;
	fn close() -> Weight;
	fn request_close() -> Weight;
}

/// Weights measured on the runtime, with the database weights of the runtime.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `Channels::NextChannelId` (r:1 w:1)
	/// Proof: `Channels::NextChannelId` (`max_values`: Some(1), `max_size`: Some(8), added: 503, mode: `MaxEncodedLen`)
	/// Storage: `Channels::Channels` (r:0 w:1)
	/// Proof: `Channels::Channels` (`max_values`: None, `max_size`: Some(2733), added: 5208, mode: `MaxEncodedLen`)
	fn open() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `212`
		//  Estimated: `3640`
		// Minimum execution time: 52_000_000 picoseconds.
		Weight::from_parts(54_000_000, 3640)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(4_u64))
	}
	/// Storage: `Channels::Channels` (r:1 w:1)
	/// Proof: `Channels::Channels` (`max_values`: None, `max_size`: Some(2733), added: 5208, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn top_up() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `3053`
		//  Estimated: `6198`
		// Minimum execution time: 56_000_000 picoseconds.
		Weight::from_parts(58_000_000, 6198)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `Channels::Channels` (r:1 w:1)
	/// Proof: `Channels::Channels` (`max_values`: None, `max_size`: Some(2733), added: 5208, mode: `MaxEncodedLen`)
	/// Storage: `System::BlockHash` (r:1 w:0)
	/// Proof: `System::BlockHash` (`max_values`: None, `max_size`: Some(44), added: 2519, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	fn claim() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `3210`
		//  Estimated: `6198`
		// Minimum execution time: 908_000_000 picoseconds.
		Weight::from_parts(915_000_000, 6198)
			.saturating_add(T::DbWeight::get().reads(5_u64))
			.saturating_add(T::DbWeight::get().writes(4_u64))
	}
	/// Storage: `Channels::Channels` (r:1 w:1)
	/// Proof: `Channels::Channels` (`max_values`: None, `max_size`: Some(2733), added: 5208, mode: `MaxEncodedLen`)
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	fn close() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `3053`
		//  Estimated: `6198`
		// Minimum execution time: 48_000_000 picoseconds.
		Weight::from_parts(49_000_000, 6198)
			.saturating_add(T::DbWeight::get().reads(3_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
	/// Storage: `Channels::Channels` (r:1 w:1)
	/// Proof: `Channels::Channels` (`max_values`: None, `max_size`: Some(2733), added: 5208, mode: `MaxEncodedLen`)
	fn request_close() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `2894`
		//  Estimated: `6198`
		// Minimum execution time: 14_000_000 picoseconds.
		Weight::from_parts(15_000_000, 6198)
			.saturating_add(T::DbWeight::get().reads(1_u64))
			.saturating_add(T::DbWeight::get().writes(1_u64))
	}
}

/// The same weights with the database weights of RocksDB, for tests.
impl WeightInfo for () {
	fn open() -> Weight {
		Weight::from_parts(54_000_000, 3640)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(4_u64))
	}
	fn top_up() -> Weight {
		Weight::from_parts(58_000_000, 6198)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn claim() -> Weight {
		Weight::from_parts(915_000_000, 6198)
			.saturating_add(RocksDbWeight::get().reads(5_u64))
			.saturating_add(RocksDbWeight::get().writes(4_u64))
	}
	fn close() -> Weight {
		Weight::from_parts(49_000_000, 6198)
			.saturating_add(RocksDbWeight::get().reads(3_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
	fn request_close() -> Weight {
		Weight::from_parts(15_000_000, 6198)
			.saturating_add(RocksDbWeight::get().reads(1_u64))
			.saturating_add(RocksDbWeight::get().writes(1_u64))
	}
}
