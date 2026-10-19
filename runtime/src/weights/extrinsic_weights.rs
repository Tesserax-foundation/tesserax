//! Generated with the Substrate benchmark CLI 62.0.0 on 2026-10-19 (Apple M1).
//!
//! SHORT-NAME: `extrinsic`, LONG-NAME: `ExtrinsicBase`, RUNTIME: `Tessera Development`
//! WARMUPS: `10`, REPEAT: `100`
//! WEIGHT-METRIC: `Average`, WEIGHT-MUL: `1.0`, WEIGHT-ADD: `0`
//!
//! Command: `tessera benchmark overhead --dev --weight-path runtime/src/weights`

use sp_core::parameter_types;
use sp_weights::{constants::WEIGHT_REF_TIME_PER_NANOS, Weight};

parameter_types! {
	/// Weight of executing a NO-OP extrinsic, for example `System::remark`.
	/// Calculated by multiplying the *Average* with `1.0` and adding `0`.
	///
	/// Stats nanoseconds:
	///   Min, Max: 645_334, 710_268
	///   Average:  653_260
	///   Median:   649_582
	///   Std-Dev:  9854.64
	///
	/// Percentiles nanoseconds:
	///   99th: 690_755
	///   95th: 670_892
	///   75th: 654_411
	pub const ExtrinsicBaseWeight: Weight =
		Weight::from_parts(WEIGHT_REF_TIME_PER_NANOS.saturating_mul(653_260), 0);
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
