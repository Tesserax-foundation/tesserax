//! Weights of the payment channels pallet.
//!
//! Measured with `tesserax benchmark pallet --pallet pallet_channels --extrinsic '*' --steps 2
//! --repeat 30` on the compiled WebAssembly runtime (Apple M1). Must be measured again on the
//! reference hardware before launch.

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

/// Weight functions of the pallet.
pub trait WeightInfo {
	fn open() -> Weight;
	fn top_up() -> Weight;
	fn claim() -> Weight;
	fn close() -> Weight;
	fn request_close() -> Weight;
}

/// Weights measured with the runtime benchmarks.
pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Channels::NextChannelId`, `Channels::Channels`, `System::Account`, `Balances::Holds`
	fn open() -> Weight {
		// Minimum execution time: 52_000_000 picoseconds.
		Weight::from_parts(53_000_000, 3622)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(4))
	}

	/// Storage: `Channels::Channels`, `System::Account`, `Balances::Holds`
	fn top_up() -> Weight {
		// Minimum execution time: 56_000_000 picoseconds.
		Weight::from_parts(57_000_000, 5558)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Verification of the ML-DSA-87 voucher, then a transfer from the hold.
	/// Storage: `Channels::Channels`, `System::BlockHash`, `System::Account` (r:2 w:2), `Balances::Holds`
	fn claim() -> Weight {
		// Minimum execution time: 908_000_000 picoseconds.
		Weight::from_parts(914_000_000, 6198)
			.saturating_add(T::DbWeight::get().reads(5))
			.saturating_add(T::DbWeight::get().writes(4))
	}

	/// Storage: `Channels::Channels`, `System::Account`, `Balances::Holds`
	fn close() -> Weight {
		// Minimum execution time: 47_000_000 picoseconds.
		Weight::from_parts(48_000_000, 5558)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Storage: `Channels::Channels`
	fn request_close() -> Weight {
		// Minimum execution time: 13_000_000 picoseconds.
		Weight::from_parts(15_000_000, 5558)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}
}

impl WeightInfo for () {
	fn open() -> Weight {
		Weight::from_parts(53_000_000, 3622)
	}
	fn top_up() -> Weight {
		Weight::from_parts(57_000_000, 5558)
	}
	fn claim() -> Weight {
		Weight::from_parts(914_000_000, 6198)
	}
	fn close() -> Weight {
		Weight::from_parts(48_000_000, 5558)
	}
	fn request_close() -> Weight {
		Weight::from_parts(15_000_000, 5558)
	}
}
