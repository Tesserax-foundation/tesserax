//! # Emission pallet
//!
//! Mints the block reward of the proof-of-work miner. The reward of block `n` is
//! `InitialReward >> ((n - 1) / HalvingInterval)`: 0.85 TESS for blocks 1 to 7 000 000, then
//! half of it for the next 7 000 000 blocks, and so on. The genesis block has no reward.
//!
//! The miner is the account in the proof-of-work pre-runtime digest of the block, which the
//! seal covers: it cannot be changed without mining the block again. A block without author
//! receives no reward.
//!
//! The total minted by this pallet is tracked in [`TotalMinted`] and can never exceed
//! `MiningSupply` (11 900 000 TESS): the halvings bound it already, the cap guards the
//! configuration. A reward the author cannot receive (an amount below the existential deposit
//! for a new account, after many halvings) is not minted.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

/// Rewards are zero from this many halvings on: a `u128` shifted by 128 bits or more is zero.
const MAX_HALVINGS: u32 = 128;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate},
			FindAuthor,
		},
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::{Saturating, UniqueSaturatedFrom, UniqueSaturatedInto, Zero};

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The native currency.
		type Currency: Mutate<Self::AccountId>;

		/// Miner of the current block.
		type FindAuthor: FindAuthor<Self::AccountId>;

		/// Reward of the blocks before the first halving.
		#[pallet::constant]
		type InitialReward: Get<BalanceOf<Self>>;

		/// Number of blocks between two halvings.
		#[pallet::constant]
		type HalvingInterval: Get<BlockNumberFor<Self>>;

		/// Maximum total minted as block rewards.
		#[pallet::constant]
		type MiningSupply: Get<BalanceOf<Self>>;

		/// Weights of the pallet.
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Total minted as block rewards.
	#[pallet::storage]
	pub type TotalMinted<T: Config> = StorageValue<_, BalanceOf<T>, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// The miner of the block received its reward.
		Rewarded { author: T::AccountId, amount: BalanceOf<T> },
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(n: BlockNumberFor<T>) -> Weight {
			let Some(author) = Self::author() else {
				return T::WeightInfo::on_initialize_without_author();
			};
			let minted = TotalMinted::<T>::get();
			let amount = Self::block_reward(n).min(T::MiningSupply::get().saturating_sub(minted));
			if !amount.is_zero() && T::Currency::mint_into(&author, amount).is_ok() {
				TotalMinted::<T>::put(minted.saturating_add(amount));
				Self::deposit_event(Event::Rewarded { author, amount });
			}
			T::WeightInfo::on_initialize_with_author()
		}

		fn integrity_test() {
			assert!(!T::HalvingInterval::get().is_zero(), "the halving interval must not be zero");
			assert!(
				Self::scheduled_supply() <= T::MiningSupply::get(),
				"the reward schedule must fit in the mining supply"
			);
		}

		#[cfg(feature = "try-runtime")]
		fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
			ensure!(TotalMinted::<T>::get() <= T::MiningSupply::get(), "minted above the supply");
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// Reward of block `n`, before the supply cap.
		pub fn block_reward(n: BlockNumberFor<T>) -> BalanceOf<T> {
			if n.is_zero() {
				return Zero::zero();
			}
			let halvings: u128 =
				((n - 1u32.into()) / T::HalvingInterval::get()).unique_saturated_into();
			if halvings >= MAX_HALVINGS as u128 {
				return Zero::zero();
			}
			let initial: u128 = T::InitialReward::get().unique_saturated_into();
			BalanceOf::<T>::unique_saturated_from(initial >> halvings)
		}

		/// Sum of the rewards of every block, following the halvings.
		pub fn scheduled_supply() -> BalanceOf<T> {
			let initial: u128 = T::InitialReward::get().unique_saturated_into();
			let interval: u128 = T::HalvingInterval::get().unique_saturated_into();
			let total = (0..MAX_HALVINGS)
				.map(|halvings| (initial >> halvings).saturating_mul(interval))
				.fold(0u128, |total, era| total.saturating_add(era));
			BalanceOf::<T>::unique_saturated_from(total)
		}

		/// Miner of the current block.
		fn author() -> Option<T::AccountId> {
			let digest = frame_system::Pallet::<T>::digest();
			T::FindAuthor::find_author(digest.logs.iter().filter_map(|item| item.as_pre_runtime()))
		}
	}
}
