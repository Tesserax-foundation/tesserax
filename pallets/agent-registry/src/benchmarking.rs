//! Benchmarks of the agent registry pallet.

use super::*;
use alloc::vec;
use frame_benchmarking::v2::*;
use frame_support::traits::{
	fungible::{Inspect, Mutate},
	Get,
};
use frame_system::RawOrigin;
use sp_runtime::traits::Bounded;

fn funded<T: Config>(name: &'static str, index: u32) -> T::AccountId {
	let who: T::AccountId = account(name, index, 0);
	T::Currency::set_balance(&who, BalanceOf::<T>::max_value() / 4u32.into());
	who
}

/// The largest profile.
fn profile() -> Profile {
	let tag = vec![b'a'; MAX_CAPABILITY as usize];
	Profile {
		name: vec![b'n'; MAX_NAME as usize].try_into().expect("within the bound; qed"),
		description: vec![b'd'; MAX_DESCRIPTION as usize].try_into().expect("within the bound; qed"),
		endpoint: vec![b'e'; MAX_ENDPOINT as usize].try_into().expect("within the bound; qed"),
		capabilities: vec![tag.try_into().expect("within the bound; qed"); MAX_CAPABILITIES as usize]
			.try_into()
			.expect("within the bound; qed"),
		metadata: Some([1; 32]),
	}
}

fn registered<T: Config>() -> Result<(T::AccountId, T::AccountId), BenchmarkError> {
	let agent = funded::<T>("agent", 0);
	let owner = funded::<T>("owner", 0);
	Pallet::<T>::register(RawOrigin::Signed(agent.clone()).into(), profile(), Some(owner.clone()))?;
	Ok((agent, owner))
}

#[benchmarks]
mod benchmarks {
	use super::*;

	#[benchmark]
	fn register() {
		let agent = funded::<T>("agent", 0);
		let owner: T::AccountId = account("owner", 0, 0);

		#[extrinsic_call]
		_(RawOrigin::Signed(agent.clone()), profile(), Some(owner));

		assert!(Agents::<T>::contains_key(&agent));
	}

	#[benchmark]
	fn update_profile() -> Result<(), BenchmarkError> {
		let agent = funded::<T>("agent", 0);
		Pallet::<T>::register(RawOrigin::Signed(agent.clone()).into(), Profile {
			name: vec![b'n'].try_into().expect("within the bound; qed"),
			..Default::default()
		}, None)?;

		#[extrinsic_call]
		_(RawOrigin::Signed(agent.clone()), profile());

		assert_eq!(Agents::<T>::get(&agent).map(|a| a.profile), Some(profile()));
		Ok(())
	}

	#[benchmark]
	fn set_owner() -> Result<(), BenchmarkError> {
		let (agent, _) = registered::<T>()?;
		let new_owner: T::AccountId = account("owner", 1, 0);

		#[extrinsic_call]
		_(RawOrigin::Signed(agent), Some(new_owner));

		Ok(())
	}

	#[benchmark]
	fn confirm_owner() -> Result<(), BenchmarkError> {
		let (agent, owner) = registered::<T>()?;

		#[extrinsic_call]
		_(RawOrigin::Signed(owner), agent.clone());

		assert!(Agents::<T>::get(&agent).is_some_and(|a| a.owner_confirmed));
		Ok(())
	}

	#[benchmark]
	fn set_active() -> Result<(), BenchmarkError> {
		let (agent, owner) = registered::<T>()?;
		Pallet::<T>::confirm_owner(RawOrigin::Signed(owner.clone()).into(), agent.clone())?;

		#[extrinsic_call]
		_(RawOrigin::Signed(owner), agent, false);

		Ok(())
	}

	#[benchmark]
	fn deregister() -> Result<(), BenchmarkError> {
		let (agent, owner) = registered::<T>()?;
		Pallet::<T>::confirm_owner(RawOrigin::Signed(owner.clone()).into(), agent.clone())?;

		#[extrinsic_call]
		_(RawOrigin::Signed(owner), agent.clone());

		assert!(!Agents::<T>::contains_key(&agent));
		Ok(())
	}

	/// A first payment: a new payer record.
	#[benchmark]
	fn pay() -> Result<(), BenchmarkError> {
		let (agent, _) = registered::<T>()?;
		let payer = funded::<T>("payer", 0);
		let amount = T::MinPayment::get().max(T::Currency::minimum_balance());

		#[extrinsic_call]
		_(RawOrigin::Signed(payer.clone()), agent.clone(), amount);

		assert!(Payers::<T>::contains_key(&agent, &payer));
		Ok(())
	}

	#[benchmark]
	fn rate() -> Result<(), BenchmarkError> {
		let (agent, _) = registered::<T>()?;
		let payer = funded::<T>("payer", 0);
		let amount = T::MinPayment::get().max(T::Currency::minimum_balance());
		Pallet::<T>::pay(RawOrigin::Signed(payer.clone()).into(), agent.clone(), amount)?;

		#[extrinsic_call]
		_(RawOrigin::Signed(payer), agent.clone(), true);

		assert_eq!(Reputations::<T>::get(&agent).positive, 1);
		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
