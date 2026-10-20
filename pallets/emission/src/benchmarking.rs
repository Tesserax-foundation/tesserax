//! Benchmarks of the emission pallet.

use super::*;
use frame_benchmarking::v2::*;
use frame_support::traits::Hooks;
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::traits::Zero;

#[benchmarks]
mod benchmarks {
	use super::*;

	/// Reward of a block mined by a new account: the most expensive case.
	#[benchmark]
	fn on_initialize_with_author() {
		let author: T::AccountId = account("miner", 0, 0);
		let digest = frame_support::sp_runtime::Digest {
			logs: alloc::vec![frame_support::sp_runtime::DigestItem::PreRuntime(
				*b"pow_",
				codec::Encode::encode(&author),
			)],
		};
		let n: BlockNumberFor<T> = 1u32.into();
		frame_system::Pallet::<T>::initialize(&n, &Default::default(), &digest);

		#[block]
		{
			Pallet::<T>::on_initialize(n);
		}

		assert!(!TotalMinted::<T>::get().is_zero() || Pallet::<T>::block_reward(n).is_zero());
	}

	#[benchmark]
	fn on_initialize_without_author() {
		let n: BlockNumberFor<T> = 1u32.into();
		frame_system::Pallet::<T>::initialize(&n, &Default::default(), &Default::default());

		#[block]
		{
			Pallet::<T>::on_initialize(n);
		}

		assert!(TotalMinted::<T>::get().is_zero());
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
