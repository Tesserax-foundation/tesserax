//! Weights of `frame_system` in the Tesserax runtime, measured by `scripts/benchmark.sh`.
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
// frame_system
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
// runtime/src/weights/frame_system.rs

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::Weight};
use core::marker::PhantomData;

/// Weight functions for `frame_system`.
pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> frame_system::WeightInfo for WeightInfo<T> {
	/// The range of component `b` is `[0, 3932160]`.
	fn remark(b: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 2_000_000 picoseconds.
		Weight::from_parts(17_824_130, 0)
			.saturating_add(Weight::from_parts(0, 0))
			// Standard Error: 0
			.saturating_add(Weight::from_parts(255, 0).saturating_mul(b.into()))
	}
	/// The range of component `b` is `[0, 3932160]`.
	fn remark_with_event(b: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 5_000_000 picoseconds.
		Weight::from_parts(26_442_149, 0)
			.saturating_add(Weight::from_parts(0, 0))
			// Standard Error: 1
			.saturating_add(Weight::from_parts(1_293, 0).saturating_mul(b.into()))
	}
	/// Storage: UNKNOWN KEY `0x3a686561707061676573` (r:0 w:1)
	/// Proof: UNKNOWN KEY `0x3a686561707061676573` (r:0 w:1)
	fn set_heap_pages() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 3_000_000 picoseconds.
		Weight::from_parts(4_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
			.saturating_add(T::DbWeight::get().writes(1))
	}
	/// Storage: UNKNOWN KEY `0x3a636f6465` (r:0 w:1)
	/// Proof: UNKNOWN KEY `0x3a636f6465` (r:0 w:1)
	fn set_code() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 82_837_000_000 picoseconds.
		Weight::from_parts(84_053_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
			.saturating_add(T::DbWeight::get().writes(1))
	}
	/// Storage: `Skipped::Metadata` (r:0 w:0)
	/// Proof: `Skipped::Metadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
	/// The range of component `i` is `[0, 1000]`.
	fn set_storage(i: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 2_000_000 picoseconds.
		Weight::from_parts(2_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
			// Standard Error: 1_929
			.saturating_add(Weight::from_parts(892_095, 0).saturating_mul(i.into()))
			.saturating_add(T::DbWeight::get().writes((1_u64).saturating_mul(i.into())))
	}
	/// Storage: `Skipped::Metadata` (r:0 w:0)
	/// Proof: `Skipped::Metadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
	/// The range of component `i` is `[0, 1000]`.
	fn kill_storage(i: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 2_000_000 picoseconds.
		Weight::from_parts(2_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
			// Standard Error: 955
			.saturating_add(Weight::from_parts(632_353, 0).saturating_mul(i.into()))
			.saturating_add(T::DbWeight::get().writes((1_u64).saturating_mul(i.into())))
	}
	/// Storage: `Skipped::Metadata` (r:0 w:0)
	/// Proof: `Skipped::Metadata` (`max_values`: None, `max_size`: None, mode: `Measured`)
	/// The range of component `p` is `[0, 1000]`.
	fn kill_prefix(p: u32, ) -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `53 + p * (69 ±0)`
		//  Estimated: `54 + p * (70 ±0)`
		// Minimum execution time: 4_000_000 picoseconds.
		Weight::from_parts(4_000_000, 0)
			.saturating_add(Weight::from_parts(0, 54))
			// Standard Error: 1_692
			.saturating_add(Weight::from_parts(1_155_912, 0).saturating_mul(p.into()))
			.saturating_add(T::DbWeight::get().reads((1_u64).saturating_mul(p.into())))
			.saturating_add(T::DbWeight::get().writes((1_u64).saturating_mul(p.into())))
			.saturating_add(Weight::from_parts(0, 70).saturating_mul(p.into()))
	}
	/// Storage: `System::AuthorizedUpgrade` (r:0 w:1)
	/// Proof: `System::AuthorizedUpgrade` (`max_values`: Some(1), `max_size`: Some(33), added: 528, mode: `MaxEncodedLen`)
	fn authorize_upgrade() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `0`
		//  Estimated: `0`
		// Minimum execution time: 7_000_000 picoseconds.
		Weight::from_parts(14_000_000, 0)
			.saturating_add(Weight::from_parts(0, 0))
			.saturating_add(T::DbWeight::get().writes(1))
	}
	/// Storage: `System::AuthorizedUpgrade` (r:1 w:1)
	/// Proof: `System::AuthorizedUpgrade` (`max_values`: Some(1), `max_size`: Some(33), added: 528, mode: `MaxEncodedLen`)
	/// Storage: UNKNOWN KEY `0x3a636f6465` (r:0 w:1)
	/// Proof: UNKNOWN KEY `0x3a636f6465` (r:0 w:1)
	fn apply_authorized_upgrade() -> Weight {
		// Proof Size summary in bytes:
		//  Measured:  `22`
		//  Estimated: `1518`
		// Minimum execution time: 87_178_000_000 picoseconds.
		Weight::from_parts(89_323_000_000, 0)
			.saturating_add(Weight::from_parts(0, 1518))
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(2))
	}
}
