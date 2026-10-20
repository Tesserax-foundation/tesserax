//! Benchmarks of the agents pallet.

use super::*;
use alloc::{boxed::Box, vec::Vec};
use frame_benchmarking::v2::*;
use frame_support::traits::fungible::Mutate;
use frame_system::RawOrigin;
use sp_runtime::traits::Bounded;

fn funded<T: Config>(name: &'static str, index: u32) -> T::AccountId {
	let who: T::AccountId = account(name, index, 0);
	T::Currency::set_balance(&who, BalanceOf::<T>::max_value() / 4u32.into());
	who
}

fn policy<T: Config>(targets: Vec<T::AccountId>) -> PolicyOf<T> {
	Policy {
		budget: BalanceOf::<T>::max_value() / 8u32.into(),
		period: 100u32.into(),
		max_per_call: BalanceOf::<T>::max_value() / 8u32.into(),
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: true },
		targets: targets.try_into().expect("within the bound; qed"),
	}
}

#[benchmarks]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn authorize(t: Linear<0, { T::MaxTargets::get() }>) {
		let owner = funded::<T>("owner", 0);
		let agent: T::AccountId = account("agent", 0, 0);
		let targets = (0..t).map(|i| account("target", i, 0)).collect();

		#[extrinsic_call]
		_(RawOrigin::Signed(owner.clone()), agent.clone(), policy::<T>(targets));

		assert!(Delegations::<T>::contains_key(&owner, &agent));
	}

	#[benchmark]
	fn revoke() -> Result<(), BenchmarkError> {
		let owner = funded::<T>("owner", 0);
		let agent: T::AccountId = account("agent", 0, 0);
		let targets = (0..T::MaxTargets::get()).map(|i| account("target", i, 0)).collect();
		Pallet::<T>::authorize(RawOrigin::Signed(owner.clone()).into(), agent.clone(), policy::<T>(targets))?;

		#[extrinsic_call]
		_(RawOrigin::Signed(owner.clone()), agent.clone());

		assert!(!Delegations::<T>::contains_key(&owner, &agent));
		Ok(())
	}

	/// A transfer made by an agent, with the longest list of targets to search.
	#[benchmark]
	fn execute() -> Result<(), BenchmarkError> {
		let owner = funded::<T>("owner", 0);
		let agent = funded::<T>("agent", 0);
		let to = funded::<T>("target", T::MaxTargets::get() - 1);
		let targets = (0..T::MaxTargets::get()).map(|i| account("target", i, 0)).collect();
		Pallet::<T>::authorize(RawOrigin::Signed(owner.clone()).into(), agent.clone(), policy::<T>(targets))?;
		let call = T::BenchmarkHelper::transfer(to);

		#[extrinsic_call]
		_(RawOrigin::Signed(agent), owner, Box::new(call));

		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
