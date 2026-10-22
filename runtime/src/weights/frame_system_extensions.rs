//! Weights of `frame_system_extensions` in the Tesserax runtime, measured by `scripts/benchmark.sh`.
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
// frame_system_extensions
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
// runtime/src/weights/frame_system_extensions.rs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::Weight};
use core::marker::PhantomData;

/// Weight functions for `frame_system_extensions`.
pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> frame_system::ExtensionsWeightInfo for WeightInfo<T> {
	fn check_genesis() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 1_000_000 picoseconds.
		Weight::from_parts(2_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	fn check_mortality_mortal_transaction() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 3_000_000 picoseconds.
		Weight::from_parts(4_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	fn check_mortality_immortal_transaction() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 3_000_000 picoseconds.
		Weight::from_parts(4_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	fn check_non_zero_sender() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 0_000 picoseconds.
		Weight::from_parts(1_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	/// Storage: `System::Account` (r:1 w:1)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	fn check_nonce() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `139`
		//  Estimated: `3593`
		// Minimum execution time: 8_000_000 picoseconds.
		Weight::from_parts(9_000_000, 0)
			.saturating_add(Weight::from_parts(0, 3593))
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}
	fn check_spec_version() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 0_000 picoseconds.
		Weight::from_parts(0, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	fn check_tx_version() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 0_000 picoseconds.
		Weight::from_parts(0, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	fn check_weight() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 4_000_000 picoseconds.
		Weight::from_parts(5_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
	fn weight_reclaim() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 2_000_000 picoseconds.
		Weight::from_parts(3_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
	}
}
