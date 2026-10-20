//! # Allocations pallet
//!
//! Creates the genesis allocations (team and foundation) and locks them with vesting schedules.
//!
//! An allocation of `amount` has a `liquid` part, spendable from genesis, and a vested part
//! released linearly over `duration` blocks from genesis. Before the `cliff` block nothing of the
//! vested part is released; at the cliff, the share accrued since genesis (`cliff / duration`)
//! is released at once, then the rest linearly until `duration`. Without cliff the release is
//! linear from genesis.
//!
//! The schedules are enforced by the vesting pallet: locked funds cannot be moved, not even to pay
//! transaction fees. What has vested is released by `vest`, or by `vest_other` sent from any
//! account, for example when the beneficiary holds nothing spendable yet.
//!
//! Allocations are recorded in [`Allocations`] so that anyone can check them on chain.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

use alloc::vec::Vec;
use codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_runtime::traits::AtLeast32BitUnsigned;

/// A genesis allocation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct Allocation<Balance, BlockNumber> {
	/// Total amount.
	pub amount: Balance,
	/// Part spendable from genesis.
	pub liquid: Balance,
	/// Block before which nothing of the vested part is released.
	pub cliff: BlockNumber,
	/// Block at which everything is released.
	pub duration: BlockNumber,
}

/// A vesting schedule: `locked` released by `per_block` per block after `start`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Schedule<Balance, BlockNumber> {
	pub locked: Balance,
	pub per_block: Balance,
	pub start: BlockNumber,
}

impl<Balance, BlockNumber> Allocation<Balance, BlockNumber>
where
	Balance: AtLeast32BitUnsigned + Copy,
	BlockNumber: AtLeast32BitUnsigned + Copy,
{
	/// Whether the parameters are consistent.
	pub fn is_valid(&self) -> bool {
		!self.amount.is_zero() &&
			self.liquid <= self.amount &&
			self.cliff <= self.duration &&
			(self.liquid == self.amount || !self.duration.is_zero())
	}

	/// Vesting schedules releasing the vested part. Their sum unlocks exactly the vested part:
	/// nothing before the cliff, the share accrued since genesis at the cliff, then linearly
	/// until `duration`.
	pub fn schedules(&self) -> Vec<Schedule<Balance, BlockNumber>> {
		let vested: u128 = (self.amount - self.liquid).unique_saturated_into();
		if vested == 0 {
			return Vec::new();
		}
		let cliff: u128 = self.cliff.unique_saturated_into();
		let duration: u128 = self.duration.unique_saturated_into();
		let balance = |x: u128| Balance::unique_saturated_from(x);

		let mut schedules = Vec::new();
		// Share accrued at the cliff, released in the block of the cliff.
		let at_cliff = if cliff == 0 { 0 } else { mul_div(vested, cliff, duration) };
		if at_cliff > 0 {
			schedules.push(Schedule {
				locked: balance(at_cliff),
				per_block: balance(at_cliff),
				start: self.cliff - 1u32.into(),
			});
		}
		// The rest, linearly from the cliff until `duration`, rounding up so that it ends there.
		let rest = vested - at_cliff;
		if rest > 0 {
			let blocks = duration - cliff;
			schedules.push(Schedule {
				locked: balance(rest),
				per_block: balance(rest.div_ceil(blocks.max(1))),
				start: self.cliff,
			});
		}
		schedules
	}
}

/// `a * b / c` without overflow for the amounts and block numbers of the chain.
fn mul_div(a: u128, b: u128, c: u128) -> u128 {
	let (q, r) = (a / c, a % c);
	q * b + r * b / c
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{Currency, VestingSchedule},
	};
	use frame_system::pallet_prelude::*;

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The native currency.
		type Currency: Currency<Self::AccountId>;

		/// Vesting schedules locking the allocations.
		type Vesting: VestingSchedule<
			Self::AccountId,
			Moment = BlockNumberFor<Self>,
			Currency = Self::Currency,
		>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Genesis allocations by beneficiary.
	#[pallet::storage]
	pub type Allocations<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Allocation<BalanceOf<T>, BlockNumberFor<T>>,
		OptionQuery,
	>;

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		/// `(beneficiary, amount, liquid, cliff, duration)`.
		pub allocations:
			Vec<(T::AccountId, BalanceOf<T>, BalanceOf<T>, BlockNumberFor<T>, BlockNumberFor<T>)>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			for (who, amount, liquid, cliff, duration) in &self.allocations {
				let allocation = Allocation {
					amount: *amount,
					liquid: *liquid,
					cliff: *cliff,
					duration: *duration,
				};
				assert!(allocation.is_valid(), "invalid allocation {allocation:?}");
				assert!(!Allocations::<T>::contains_key(who), "duplicate allocation");

				let _ = T::Currency::deposit_creating(who, allocation.amount);
				assert!(
					T::Currency::free_balance(who) >= allocation.amount,
					"the allocation could not be credited"
				);
				for s in allocation.schedules() {
					T::Vesting::add_vesting_schedule(who, s.locked, s.per_block, s.start)
						.expect("the schedules of an allocation are valid; qed");
				}
				Allocations::<T>::insert(who, allocation);
			}
		}
	}
}
