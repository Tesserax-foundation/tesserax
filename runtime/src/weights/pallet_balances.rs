//! Weights of `pallet_balances` in the Tesserax runtime, measured by `scripts/benchmark.sh`.
//!
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 62.0.0
//! DATE: 2026-10-22, STEPS: `50`, REPEAT: `20`, LOW RANGE: `[]`, HIGH RANGE: `[]`
//! WORST CASE MAP SIZE: `1000000`
//! CPU: `Apple M1`
//! WASM-EXECUTION: `Compiled`, CHAIN: `None`, DB CACHE: 1024

// Executed Command:
// target/release/tesserax
// benchmark
// pallet
// --runtime
// target/release/wbuild/tesserax-runtime/tesserax_runtime.compact.compressed.wasm
// --genesis-builder-preset
// development
// --pallet
// pallet_balances
// --extrinsic
// *
// --steps
// 50
// --repeat
// 20
// --wasm-execution
// compiled
// --heap-pages
// 4096
// --template
// scripts/benchmarks/runtime.hbs
// --output
// runtime/src/weights/pallet_balances.rs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::Weight};
use core::marker::PhantomData;

/// Weight functions for `pallet_balances`.
pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> pallet_balances::WeightInfo for WeightInfo<T> {
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:2)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn transfer_allow_death() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `423`
		//  Estimated: `3640`
		// Minimum execution time: 83_000_000 picoseconds.
		Weight::from_parts(85_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3640))
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(4))
	}
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:1)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn transfer_keep_alive() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `423`
		//  Estimated: `3593`
		// Minimum execution time: 49_000_000 picoseconds.
		Weight::from_parts(50_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3593))
			.saturating_add(T::DbWeight::get().reads(2))
			.saturating_add(T::DbWeight::get().writes(2))
	}
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:1)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn force_set_balance_creating() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `423`
		//  Estimated: `3593`
		// Minimum execution time: 29_000_000 picoseconds.
		Weight::from_parts(31_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3593))
			.saturating_add(T::DbWeight::get().reads(2))
			.saturating_add(T::DbWeight::get().writes(2))
	}
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:0 w:1)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn force_set_balance_killing() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `103`
		//  Estimated: `3640`
		// Minimum execution time: 42_000_000 picoseconds.
		Weight::from_parts(43_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3640))
			.saturating_add(T::DbWeight::get().reads(2))
			.saturating_add(T::DbWeight::get().writes(3))
	}
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:2)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn force_transfer() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `559`
		//  Estimated: `6196`
		// Minimum execution time: 85_000_000 picoseconds.
		Weight::from_parts(88_000_000, 0)
			.saturating_add(Weight::from_parts(0, 6196))
			.saturating_add(T::DbWeight::get().reads(4))
			.saturating_add(T::DbWeight::get().writes(5))
	}
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:2)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn transfer_all() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `423`
		//  Estimated: `3640`
		// Minimum execution time: 80_000_000 picoseconds.
		Weight::from_parts(98_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3640))
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(4))
	}
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	fn force_unreserve() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `103`
		//  Estimated: `3593`
		// Minimum execution time: 17_000_000 picoseconds.
		Weight::from_parts(19_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3593))
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}
	/// Storage: `System::Account` (r:999 w:999)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// The range of component `u` is `[1, 1000]`.
	fn upgrade_accounts(u: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0 + u * (136 ±0)`
		//  Estimated: `990 + u * (2603 ±0)`
		// Minimum execution time: 18_000_000 picoseconds.
		Weight::from_parts(19_000_000, 0)
			.saturating_add(Weight::from_parts(0, 990))
			// Standard Error: 18_011
			.saturating_add(Weight::from_parts(15_415_680, 0).saturating_mul(u.into()))
			.saturating_add(T::DbWeight::get().reads((1_u64).saturating_mul(u.into())))
			.saturating_add(T::DbWeight::get().writes((1_u64).saturating_mul(u.into())))
			.saturating_add(Weight::from_parts(0, 2603).saturating_mul(u.into()))
	}
	fn force_adjust_total_issuance() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 6_000_000 picoseconds.
		Weight::from_parts(7_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	/// Storage: `Balances::Holds` (r:1 w:1)
	/// Proof: `Balances::Holds` (`max_values`: None, `max_size`: Some(175), added: 2650, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:0 w:1)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn burn_allow_death() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `3640`
		// Minimum execution time: 55_000_000 picoseconds.
		Weight::from_parts(56_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3640))
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(2))
	}
	fn burn_keep_alive() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 21_000_000 picoseconds.
		Weight::from_parts(22_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
}
