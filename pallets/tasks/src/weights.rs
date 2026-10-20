//! Weights of the tasks pallet.
//!
//! Measured with `tesserax benchmark pallet --pallet pallet_tasks --extrinsic '*' --steps 2
//! --repeat 30` on the compiled WebAssembly runtime (Apple M1). Must be measured again on the
//! reference hardware before launch.

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

/// Weight functions of the pallet.
pub trait WeightInfo {
	fn create() -> Weight;
	fn cancel() -> Weight;
	fn accept() -> Weight;
	fn deliver() -> Weight;
	fn dispute() -> Weight;
	/// Approving, resolving or finalizing: payments to the worker, the client and the arbiter.
	fn complete() -> Weight;
}

/// Weights measured with the runtime benchmarks.
pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Tasks::NextTaskId`, `Tasks::Tasks`, `System::Account`, `Balances::Holds`
	fn create() -> Weight {
		// Minimum execution time: 52_000_000 picoseconds.
		Weight::from_parts(54_000_000, 3640)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(4))
	}

	/// Storage: `Tasks::Tasks`, `System::Account`, `Balances::Holds`
	fn cancel() -> Weight {
		// Minimum execution time: 46_000_000 picoseconds.
		Weight::from_parts(47_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Storage: `Tasks::Tasks`, `System::Account`, `Balances::Holds`
	fn accept() -> Weight {
		// Minimum execution time: 55_000_000 picoseconds.
		Weight::from_parts(56_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Storage: `Tasks::Tasks`
	fn deliver() -> Weight {
		// Minimum execution time: 12_000_000 picoseconds.
		Weight::from_parts(13_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}

	/// Storage: `Tasks::Tasks`
	fn dispute() -> Weight {
		// Minimum execution time: 12_000_000 picoseconds.
		Weight::from_parts(13_000_000, 4246)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}

	/// A dispute resolved with a share: payments to the worker, the client and the arbiter, and
	/// the reputation of the worker.
	fn complete() -> Weight {
		// Minimum execution time: 155_000_000 picoseconds.
		Weight::from_parts(158_000_000, 8799)
			.saturating_add(T::DbWeight::get().reads(7))
			.saturating_add(T::DbWeight::get().writes(7))
	}
}

impl WeightInfo for () {
	fn create() -> Weight {
		Weight::from_parts(54_000_000, 3640)
	}
	fn cancel() -> Weight {
		Weight::from_parts(47_000_000, 4246)
	}
	fn accept() -> Weight {
		Weight::from_parts(56_000_000, 4246)
	}
	fn deliver() -> Weight {
		Weight::from_parts(13_000_000, 4246)
	}
	fn dispute() -> Weight {
		Weight::from_parts(13_000_000, 4246)
	}
	fn complete() -> Weight {
		Weight::from_parts(158_000_000, 8799)
	}
}
