
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 62.0.0
//! DATE: 2026-10-22 (Y/M/D)
//!
//! SHORT-NAME: `block`, LONG-NAME: `BlockExecution`, RUNTIME: `Tesserax Development`
//! WARMUPS: `10`, REPEAT: `100`
//! WEIGHT-PATH: `runtime/src/weights`
//! WEIGHT-METRIC: `Average`, WEIGHT-MUL: `1.0`, WEIGHT-ADD: `0`

// Executed Command:
//   target/release/tesserax
//   benchmark
//   overhead
//   --dev
//   --weight-path
//   runtime/src/weights

use sp_core::parameter_types;
use sp_weights::{constants::WEIGHT_REF_TIME_PER_NANOS, Weight};

parameter_types! {
	/// Weight of executing an empty block.
	/// Calculated by multiplying the *Average* with `1.0` and adding `0`.
	///
	/// Stats nanoseconds:
	///   Min, Max: 531_041, 706_958
	///   Average:  575_118
	///   Median:   568_667
	///   Std-Dev:  35146.02
	///
	/// Percentiles nanoseconds:
	///   99th: 696_584
	///   95th: 636_583
	///   75th: 591_250
	pub const BlockExecutionWeight: Weight =
		Weight::from_parts(WEIGHT_REF_TIME_PER_NANOS.saturating_mul(575_118), 0);
}

#[cfg(test)]
mod test_weights {
	use sp_weights::constants;

	/// Checks that the weight exists and is sane.
	// NOTE: If this test fails but you are sure that the generated values are fine,
	// you can delete it.
	#[test]
	fn sane() {
		let w = super::BlockExecutionWeight::get();

		// At least 100 µs.
		assert!(
			w.ref_time() >= 100u64 * constants::WEIGHT_REF_TIME_PER_MICROS,
			"Weight should be at least 100 µs."
		);
		// At most 50 ms.
		assert!(
			w.ref_time() <= 50u64 * constants::WEIGHT_REF_TIME_PER_MILLIS,
			"Weight should be at most 50 ms."
		);
	}
}
