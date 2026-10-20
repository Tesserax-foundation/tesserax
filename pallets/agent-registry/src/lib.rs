//! # Agent registry pallet
//!
//! On-chain identities of AI agents, so that agents and people can find them and decide whether
//! to trust them.
//!
//! An agent registers with its own key: a name, a description, the endpoint of its service, the
//! capabilities it offers and the hash of an off-chain description (an "agent card"). It may
//! name an owner, who must confirm before the link is shown as verified. A deposit, held from the
//! agent, pays for the storage of its profile.
//!
//! Reputation is built from what the chain can check:
//!
//! - payments: [`Pallet::pay`] transfers to an active agent and records the payment, the volume
//!   and the number of distinct payers. Payments from the agent itself or its confirmed owner
//!   are refused;
//! - ratings: each payment entitles the payer to one rating, positive or negative, with
//!   [`Pallet::rate`]. Ratings cannot be given without paying;
//! - tasks: other pallets (a task market) record completed and failed tasks through
//!   [`ReputationRecorder`].
//!
//! Reputation outlives the registration: an agent cannot erase a bad record by registering
//! again with the same key.

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

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{traits::ConstU32, BoundedVec};
use scale_info::TypeInfo;

/// Maximum length of a name, in bytes.
pub const MAX_NAME: u32 = 64;
/// Maximum length of a description, in bytes.
pub const MAX_DESCRIPTION: u32 = 512;
/// Maximum length of an endpoint, in bytes.
pub const MAX_ENDPOINT: u32 = 256;
/// Maximum number of capabilities.
pub const MAX_CAPABILITIES: u32 = 16;
/// Maximum length of a capability, in bytes.
pub const MAX_CAPABILITY: u32 = 32;

pub type Name = BoundedVec<u8, ConstU32<MAX_NAME>>;
pub type Description = BoundedVec<u8, ConstU32<MAX_DESCRIPTION>>;
pub type Endpoint = BoundedVec<u8, ConstU32<MAX_ENDPOINT>>;
pub type Capability = BoundedVec<u8, ConstU32<MAX_CAPABILITY>>;
pub type Capabilities = BoundedVec<Capability, ConstU32<MAX_CAPABILITIES>>;

/// Public description of an agent.
#[derive(
	Clone, PartialEq, Eq, Debug, Default, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct Profile {
	/// Display name.
	pub name: Name,
	/// What the agent does.
	pub description: Description,
	/// Where its service is reached, for example an HTTPS or MCP URL.
	pub endpoint: Endpoint,
	/// Capabilities offered, as lowercase tags such as `translation` or `code-review`.
	pub capabilities: Capabilities,
	/// Hash of an off-chain description of the agent, if any.
	pub metadata: Option<[u8; 32]>,
}

impl Profile {
	/// Whether the profile is well formed: a name, printable ASCII endpoint, lowercase tags.
	pub fn is_valid(&self) -> bool {
		let printable = |b: &u8| b.is_ascii_graphic();
		let tag = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.:".contains(b);
		!self.name.is_empty() &&
			self.endpoint.iter().all(printable) &&
			self.capabilities.iter().all(|c| !c.is_empty() && c.iter().all(tag))
	}

	/// Whether the agent offers `capability`.
	pub fn offers(&self, capability: &[u8]) -> bool {
		self.capabilities.iter().any(|c| c.as_slice() == capability)
	}
}

/// A registered agent.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct AgentInfo<AccountId, Balance, BlockNumber> {
	/// Owner named by the agent.
	pub owner: Option<AccountId>,
	/// Whether the owner confirmed it owns the agent.
	pub owner_confirmed: bool,
	pub profile: Profile,
	/// Whether the agent accepts payments and work.
	pub active: bool,
	pub registered_at: BlockNumber,
	/// Deposit held from the agent.
	pub deposit: Balance,
}

impl<AccountId: PartialEq, Balance, BlockNumber> AgentInfo<AccountId, Balance, BlockNumber> {
	/// The owner, if it confirmed.
	pub fn confirmed_owner(&self) -> Option<&AccountId> {
		self.owner.as_ref().filter(|_| self.owner_confirmed)
	}
}

/// Reputation of an agent.
#[derive(Clone, PartialEq, Eq, Debug, Default, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct Reputation<Balance> {
	/// Number of payments received.
	pub payments: u64,
	/// Total received.
	pub volume: Balance,
	/// Number of distinct payers.
	pub payers: u32,
	/// Positive ratings.
	pub positive: u32,
	/// Negative ratings.
	pub negative: u32,
	/// Tasks completed.
	pub tasks_completed: u32,
	/// Tasks failed.
	pub tasks_failed: u32,
}

/// Payments of an account to an agent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct PayerRecord {
	pub payments: u32,
	/// Payments not rated yet.
	pub unrated: u32,
}

/// Records the outcome of tasks in the reputation of agents.
pub trait ReputationRecorder<AccountId> {
	fn record_task(agent: &AccountId, completed: bool);
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate, MutateHold},
			tokens::{Precision, Preservation},
		},
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::{Saturating, Zero};

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	pub type AgentInfoOf<T> =
		AgentInfo<<T as frame_system::Config>::AccountId, BalanceOf<T>, BlockNumberFor<T>>;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The overarching hold reason.
		type RuntimeHoldReason: From<HoldReason>;

		/// The native currency.
		type Currency: Mutate<Self::AccountId> + MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;

		/// Deposit for a registration.
		#[pallet::constant]
		type DepositBase: Get<BalanceOf<Self>>;

		/// Deposit for each byte of a profile.
		#[pallet::constant]
		type DepositPerByte: Get<BalanceOf<Self>>;

		/// Minimum payment recorded in the reputation.
		#[pallet::constant]
		type MinPayment: Get<BalanceOf<Self>>;

		/// Weights of the calls.
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Reasons for funds held by this pallet.
	#[pallet::composite_enum]
	pub enum HoldReason {
		/// Deposit for the storage of a registration.
		Registration,
	}

	/// Registered agents.
	#[pallet::storage]
	pub type Agents<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, AgentInfoOf<T>, OptionQuery>;

	/// Reputation of agents, kept after deregistration.
	#[pallet::storage]
	pub type Reputations<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, Reputation<BalanceOf<T>>, ValueQuery>;

	/// Payments by agent and payer.
	#[pallet::storage]
	pub type Payers<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Blake2_128Concat,
		T::AccountId,
		PayerRecord,
		OptionQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// An agent registered.
		Registered { agent: T::AccountId, owner: Option<T::AccountId> },
		/// An agent changed its profile.
		ProfileUpdated { agent: T::AccountId },
		/// An agent named a new owner, to be confirmed.
		OwnerProposed { agent: T::AccountId, owner: Option<T::AccountId> },
		/// The owner of an agent confirmed it.
		OwnerConfirmed { agent: T::AccountId, owner: T::AccountId },
		/// An agent was activated or deactivated.
		ActiveSet { agent: T::AccountId, active: bool },
		/// An agent deregistered.
		Deregistered { agent: T::AccountId },
		/// `payer` paid `amount` to `agent`.
		Paid { agent: T::AccountId, payer: T::AccountId, amount: BalanceOf<T> },
		/// `payer` rated `agent`.
		Rated { agent: T::AccountId, payer: T::AccountId, positive: bool },
		/// The outcome of a task of `agent` was recorded.
		TaskRecorded { agent: T::AccountId, completed: bool },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// The account is already registered.
		AlreadyRegistered,
		/// The account is not a registered agent.
		NotRegistered,
		/// The profile is malformed.
		InvalidProfile,
		/// An agent cannot own itself.
		SelfOwned,
		/// The caller is not the agent or its confirmed owner.
		NotAllowed,
		/// The caller is not the owner named by the agent.
		NotProposedOwner,
		/// The agent does not accept payments.
		Inactive,
		/// An agent cannot be paid by itself or its owner.
		SelfPayment,
		/// The payment is below the minimum.
		PaymentTooSmall,
		/// The caller has no payment left to rate.
		NothingToRate,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Register the signer as an agent, optionally naming its owner.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::register())]
		pub fn register(
			origin: OriginFor<T>,
			profile: Profile,
			owner: Option<T::AccountId>,
		) -> DispatchResult {
			let agent = ensure_signed(origin)?;
			ensure!(!Agents::<T>::contains_key(&agent), Error::<T>::AlreadyRegistered);
			ensure!(profile.is_valid(), Error::<T>::InvalidProfile);
			ensure!(owner.as_ref() != Some(&agent), Error::<T>::SelfOwned);

			let deposit = Self::deposit_for(&profile);
			T::Currency::hold(&HoldReason::Registration.into(), &agent, deposit)?;
			Agents::<T>::insert(
				&agent,
				AgentInfo {
					owner: owner.clone(),
					owner_confirmed: false,
					profile,
					active: true,
					registered_at: frame_system::Pallet::<T>::block_number(),
					deposit,
				},
			);
			Self::deposit_event(Event::Registered { agent, owner });
			Ok(())
		}

		/// Replace the profile of the signer.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::update_profile())]
		pub fn update_profile(origin: OriginFor<T>, profile: Profile) -> DispatchResult {
			let agent = ensure_signed(origin)?;
			let mut info = Agents::<T>::get(&agent).ok_or(Error::<T>::NotRegistered)?;
			ensure!(profile.is_valid(), Error::<T>::InvalidProfile);

			let deposit = Self::deposit_for(&profile);
			let reason = HoldReason::Registration.into();
			if deposit > info.deposit {
				T::Currency::hold(&reason, &agent, deposit - info.deposit)?;
			} else if info.deposit > deposit {
				T::Currency::release(&reason, &agent, info.deposit - deposit, Precision::BestEffort)?;
			}
			info.profile = profile;
			info.deposit = deposit;
			Agents::<T>::insert(&agent, info);
			Self::deposit_event(Event::ProfileUpdated { agent });
			Ok(())
		}

		/// Name a new owner of the signer, or none. The owner must confirm.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::set_owner())]
		pub fn set_owner(origin: OriginFor<T>, owner: Option<T::AccountId>) -> DispatchResult {
			let agent = ensure_signed(origin)?;
			ensure!(owner.as_ref() != Some(&agent), Error::<T>::SelfOwned);
			Agents::<T>::try_mutate(&agent, |info| -> DispatchResult {
				let info = info.as_mut().ok_or(Error::<T>::NotRegistered)?;
				info.owner = owner.clone();
				info.owner_confirmed = false;
				Ok(())
			})?;
			Self::deposit_event(Event::OwnerProposed { agent, owner });
			Ok(())
		}

		/// Confirm owning `agent`, as the owner it named.
		#[pallet::call_index(3)]
		#[pallet::weight(T::WeightInfo::confirm_owner())]
		pub fn confirm_owner(origin: OriginFor<T>, agent: T::AccountId) -> DispatchResult {
			let owner = ensure_signed(origin)?;
			Agents::<T>::try_mutate(&agent, |info| -> DispatchResult {
				let info = info.as_mut().ok_or(Error::<T>::NotRegistered)?;
				ensure!(info.owner.as_ref() == Some(&owner), Error::<T>::NotProposedOwner);
				info.owner_confirmed = true;
				Ok(())
			})?;
			Self::deposit_event(Event::OwnerConfirmed { agent, owner });
			Ok(())
		}

		/// Activate or deactivate `agent`, as the agent or its confirmed owner.
		#[pallet::call_index(4)]
		#[pallet::weight(T::WeightInfo::set_active())]
		pub fn set_active(origin: OriginFor<T>, agent: T::AccountId, active: bool) -> DispatchResult {
			let who = ensure_signed(origin)?;
			Agents::<T>::try_mutate(&agent, |info| -> DispatchResult {
				let info = info.as_mut().ok_or(Error::<T>::NotRegistered)?;
				ensure!(who == agent || info.confirmed_owner() == Some(&who), Error::<T>::NotAllowed);
				info.active = active;
				Ok(())
			})?;
			Self::deposit_event(Event::ActiveSet { agent, active });
			Ok(())
		}

		/// Deregister `agent`, as the agent or its confirmed owner. The deposit returns to the
		/// agent; the reputation is kept.
		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::deregister())]
		pub fn deregister(origin: OriginFor<T>, agent: T::AccountId) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let info = Agents::<T>::get(&agent).ok_or(Error::<T>::NotRegistered)?;
			ensure!(who == agent || info.confirmed_owner() == Some(&who), Error::<T>::NotAllowed);
			T::Currency::release(
				&HoldReason::Registration.into(),
				&agent,
				info.deposit,
				Precision::BestEffort,
			)?;
			Agents::<T>::remove(&agent);
			Self::deposit_event(Event::Deregistered { agent });
			Ok(())
		}

		/// Pay `amount` to `agent`. The payment counts in its reputation and entitles the payer
		/// to one rating.
		#[pallet::call_index(6)]
		#[pallet::weight(T::WeightInfo::pay())]
		pub fn pay(origin: OriginFor<T>, agent: T::AccountId, amount: BalanceOf<T>) -> DispatchResult {
			let payer = ensure_signed(origin)?;
			let info = Agents::<T>::get(&agent).ok_or(Error::<T>::NotRegistered)?;
			ensure!(info.active, Error::<T>::Inactive);
			ensure!(
				payer != agent && info.confirmed_owner() != Some(&payer),
				Error::<T>::SelfPayment
			);
			ensure!(amount >= T::MinPayment::get(), Error::<T>::PaymentTooSmall);

			T::Currency::transfer(&payer, &agent, amount, Preservation::Preserve)?;
			let new_payer = !Payers::<T>::contains_key(&agent, &payer);
			Payers::<T>::mutate(&agent, &payer, |record| {
				let record = record.get_or_insert_with(Default::default);
				record.payments.saturating_inc();
				record.unrated.saturating_inc();
			});
			Reputations::<T>::mutate(&agent, |r| {
				r.payments.saturating_inc();
				r.volume.saturating_accrue(amount);
				if new_payer {
					r.payers.saturating_inc();
				}
			});
			Self::deposit_event(Event::Paid { agent, payer, amount });
			Ok(())
		}

		/// Rate `agent` once for a payment made to it.
		#[pallet::call_index(7)]
		#[pallet::weight(T::WeightInfo::rate())]
		pub fn rate(origin: OriginFor<T>, agent: T::AccountId, positive: bool) -> DispatchResult {
			let payer = ensure_signed(origin)?;
			Payers::<T>::try_mutate(&agent, &payer, |record| -> DispatchResult {
				let record = record.as_mut().ok_or(Error::<T>::NothingToRate)?;
				ensure!(!record.unrated.is_zero(), Error::<T>::NothingToRate);
				record.unrated -= 1;
				Ok(())
			})?;
			Reputations::<T>::mutate(&agent, |r| {
				if positive {
					r.positive.saturating_inc()
				} else {
					r.negative.saturating_inc()
				}
			});
			Self::deposit_event(Event::Rated { agent, payer, positive });
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// Deposit for registering `profile`.
		pub fn deposit_for(profile: &Profile) -> BalanceOf<T> {
			T::DepositBase::get()
				.saturating_add(T::DepositPerByte::get().saturating_mul((profile.encoded_size() as u32).into()))
		}
	}

	impl<T: Config> ReputationRecorder<T::AccountId> for Pallet<T> {
		fn record_task(agent: &T::AccountId, completed: bool) {
			Reputations::<T>::mutate(agent, |r| {
				if completed {
					r.tasks_completed.saturating_inc()
				} else {
					r.tasks_failed.saturating_inc()
				}
			});
			Self::deposit_event(Event::TaskRecorded { agent: agent.clone(), completed });
		}
	}
}
