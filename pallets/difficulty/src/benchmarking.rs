//! Benchmarks of the difficulty pallet.

use super::*;
use frame_benchmarking::v2::*;
use frame_support::{traits::{Get, Hooks}, BoundedVec};
use frame_system::pallet_prelude::BlockNumberFor;

#[benchmarks]
mod benchmarks {
	use super::*;

	/// The adjustment at the end of a block with a full window of history: the most expensive
	/// case.
	#[benchmark]
	fn on_finalize() {
		let target = T::TargetBlockTime::get();
		let history: alloc::vec::Vec<(u64, Difficulty)> = (0..u64::from(T::Window::get()) + 1)
			.map(|i| (i * target, Difficulty::from(1_000_000u64 + i)))
			.collect();
		History::<T>::put(BoundedVec::truncate_from(history));
		CurrentDifficulty::<T>::put(Difficulty::from(1_000_000u64));
		let n: BlockNumberFor<T> = 1u32.into();

		#[block]
		{
			Pallet::<T>::on_finalize(n);
		}

		assert!(!CurrentDifficulty::<T>::get().is_zero());
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
