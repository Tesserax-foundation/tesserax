//! Benchmarks of the tasks pallet.

use super::*;
use alloc::vec;
use frame_benchmarking::v2::*;
use frame_support::traits::{fungible::{Inspect, Mutate}, Get};
use frame_system::RawOrigin;
use sp_runtime::{traits::Bounded, Percent};

fn funded<T: Config>(name: &'static str) -> T::AccountId {
	let who: T::AccountId = account(name, 0, 0);
	T::Currency::set_balance(&who, BalanceOf::<T>::max_value() / 8u32.into());
	who
}

fn text() -> Text {
	vec![b'a'; MAX_TEXT as usize].try_into().expect("within the bound; qed")
}

/// Task 0 with the longest texts, an arbiter and a bond, created by a funded client.
fn task<T: Config>() -> Result<(T::AccountId, T::AccountId, T::AccountId), BenchmarkError> {
	let (client, worker, arbiter) = (funded::<T>("client"), funded::<T>("worker"), funded::<T>("arbiter"));
	let amount = T::MinReward::get().max(T::Currency::minimum_balance());
	NextTaskId::<T>::put(0);
	Pallet::<T>::create(
		RawOrigin::Signed(client.clone()).into(),
		text(),
		Some(vec![b'c'; MAX_CAPABILITY as usize].try_into().expect("within the bound; qed")),
		amount,
		None,
		Some(Arbiter { account: arbiter.clone(), fee: amount }),
		amount,
		T::MinPeriod::get(),
		T::MinPeriod::get(),
	)?;
	Ok((client, worker, arbiter))
}

#[benchmarks]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn create() {
		let client = funded::<T>("client");
		let arbiter: T::AccountId = account("arbiter", 0, 0);
		let amount = T::MinReward::get().max(T::Currency::minimum_balance());

		#[extrinsic_call]
		_(
			RawOrigin::Signed(client),
			text(),
			Some(vec![b'c'; MAX_CAPABILITY as usize].try_into().expect("within the bound; qed")),
			amount,
			None,
			Some(Arbiter { account: arbiter, fee: amount }),
			amount,
			T::MinPeriod::get(),
			T::MinPeriod::get(),
		);

		assert!(Tasks::<T>::contains_key(0));
	}

	#[benchmark]
	fn cancel() -> Result<(), BenchmarkError> {
		let (client, _, _) = task::<T>()?;

		#[extrinsic_call]
		_(RawOrigin::Signed(client), 0);

		assert!(!Tasks::<T>::contains_key(0));
		Ok(())
	}

	#[benchmark]
	fn accept() -> Result<(), BenchmarkError> {
		let (_, worker, _) = task::<T>()?;

		#[extrinsic_call]
		_(RawOrigin::Signed(worker), 0);

		Ok(())
	}

	#[benchmark]
	fn deliver() -> Result<(), BenchmarkError> {
		let (_, worker, _) = task::<T>()?;
		Pallet::<T>::accept(RawOrigin::Signed(worker.clone()).into(), 0)?;

		#[extrinsic_call]
		_(RawOrigin::Signed(worker), 0, text());

		Ok(())
	}

	#[benchmark]
	fn dispute() -> Result<(), BenchmarkError> {
		let (client, worker, _) = task::<T>()?;
		Pallet::<T>::accept(RawOrigin::Signed(worker.clone()).into(), 0)?;
		Pallet::<T>::deliver(RawOrigin::Signed(worker).into(), 0, text())?;

		#[extrinsic_call]
		_(RawOrigin::Signed(client), 0);

		Ok(())
	}

	/// A dispute decided with a share: payments to the worker, the client and the arbiter.
	#[benchmark]
	fn complete() -> Result<(), BenchmarkError> {
		let (client, worker, arbiter) = task::<T>()?;
		Pallet::<T>::accept(RawOrigin::Signed(worker.clone()).into(), 0)?;
		Pallet::<T>::deliver(RawOrigin::Signed(worker).into(), 0, text())?;
		Pallet::<T>::dispute(RawOrigin::Signed(client).into(), 0)?;

		#[extrinsic_call]
		resolve(RawOrigin::Signed(arbiter), 0, Percent::from_percent(50));

		assert!(!Tasks::<T>::contains_key(0));
		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
