
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 62.0.0
//! DATE: 2026-10-22 (Y/M/D)
//!
//! SHORT-NAME: `extrinsic`, LONG-NAME: `ExtrinsicBase`, RUNTIME: `Tesserax Development`
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
	/// Weight of executing a NO-OP extrinsic, for example `System::remark`.
	/// Calculated by multiplying the *Average* with `1.0` and adding `0`.
	///
	/// Stats nanoseconds:
	///   Min, Max: 970_806, 1_159_220
	///   Average:  980_876
	///   Median:   977_277
	///   Std-Dev:  19541.21
	///
	/// Percentiles nanoseconds:
	///   99th: 1_026_437
	///   95th: 991_377
	///   75th: 980_135
	pub const ExtrinsicBaseWeight: Weight =
		Weight::from_parts(WEIGHT_REF_TIME_PER_NANOS.saturating_mul(980_876), 0);
}

#[cfg(test)]
mod test_weights {
	use sp_weights::constants;

	/// Checks that the weight exists and is sane.
	// NOTE: If this test fails but you are sure that the generated values are fine,
	// you can delete it.
	#[test]
	fn sane() {
		let w = super::ExtrinsicBaseWeight::get();

		// At least 10 µs.
		assert!(
			w.ref_time() >= 10u64 * constants::WEIGHT_REF_TIME_PER_MICROS,
			"Weight should be at least 10 µs."
		);
		// At most 1 ms.
		assert!(
			w.ref_time() <= constants::WEIGHT_REF_TIME_PER_MILLIS,
			"Weight should be at most 1 ms."
		);
	}
}
