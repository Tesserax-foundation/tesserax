//! Weights of `pallet_transaction_payment` in the Tesserax runtime, measured by `scripts/benchmark.sh`.
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
// pallet_transaction_payment
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
// runtime/src/weights/pallet_transaction_payment.rs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::Weight};
use core::marker::PhantomData;

/// Weight functions for `pallet_transaction_payment`.
pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> pallet_transaction_payment::WeightInfo for WeightInfo<T> {
	/// Storage: `System::Account` (r:2 w:2)
	/// Proof: `System::Account` (`max_values`: None, `max_size`: Some(128), added: 2603, mode: `MaxEncodedLen`)
	/// Storage: `Revive::OriginalAccount` (r:1 w:1)
	/// Proof: `Revive::OriginalAccount` (`max_values`: None, `max_size`: Some(52), added: 2527, mode: `MaxEncodedLen`)
	fn charge_transaction_payment() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `618`
		//  Estimated: `6196`
		// Minimum execution time: 71_000_000 picoseconds.
		Weight::from_parts(73_000_000, 0)
			.saturating_add(Weight::from_parts(0, 6196))
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}
}
