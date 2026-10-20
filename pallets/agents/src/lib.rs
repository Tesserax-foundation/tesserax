//! # Agents pallet
//!
//! Lets an owner account delegate to an AI agent's key the right to act for it, within limits
//! enforced by the chain. The agent signs with its own ML-DSA key and calls [`Pallet::execute`]
//! with a call, which is dispatched as the owner if the delegation allows it:
//!
//! - the kind of call is allowed: transfers, contract calls (the runtime decides which calls are
//!   which, every other call is refused, including those of this pallet);
//! - the recipient or contract is in the list of targets, if the list is not empty;
//! - the delegation has not expired;
//! - what the owner's balance loses in the call stays within the limit per call and within the
//!   budget of the current period.
//!
//! The amount spent is measured on the owner's balance before and after the call, so that
//! value moved in any way (transfers, contract value, storage deposits, holds) is counted. A call
//! that exceeds the limits is reverted.
//!
//! The owner revokes a delegation at once with [`Pallet::revoke`]; the agent can renounce it.
//! A deposit, held from the owner, pays for the storage of each delegation. The agent pays the
//! fees of its transactions from its own account.

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
use frame_support::{traits::Get, BoundedVec, CloneNoBound, DebugNoBound, EqNoBound, PartialEqNoBound};
use scale_info::TypeInfo;

/// What a call does, as far as delegation is concerned.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CallKind<AccountId> {
	/// A transfer of the native token to `to`.
	Transfer { to: AccountId },
	/// A call of the contract with account `to`.
	Contract { to: AccountId },
	/// A call an agent may never make.
	Forbidden,
}

/// Calls used by the benchmarks.
#[cfg(feature = "runtime-benchmarks")]
pub trait BenchmarkHelper<Call, AccountId> {
	/// A transfer of a small amount to `to`, which the classifier accepts.
	fn transfer(to: AccountId) -> Call;
}

/// Classifies the calls of the runtime for delegation.
pub trait ClassifyCall<Call, AccountId> {
	fn classify(call: &Call) -> CallKind<AccountId>;
}

/// Kinds of calls an agent may make.
#[derive(
	Clone, Copy, PartialEq, Eq, Debug, Default, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct Permissions {
	/// Transfers of the native token.
	pub transfers: bool,
	/// Calls of contracts.
	pub contracts: bool,
}

/// Limits of a delegation, set by the owner.
#[derive(
	CloneNoBound,
	PartialEqNoBound,
	EqNoBound,
	DebugNoBound,
	Encode,
	Decode,
	DecodeWithMemTracking,
	TypeInfo,
	MaxEncodedLen,
)]
#[scale_info(skip_type_params(MaxTargets))]
pub struct Policy<AccountId, Balance, BlockNumber, MaxTargets: Get<u32>>
where
	AccountId: Clone + PartialEq + Eq + core::fmt::Debug,
	Balance: Clone + PartialEq + Eq + core::fmt::Debug,
	BlockNumber: Clone + PartialEq + Eq + core::fmt::Debug,
{
	/// Maximum spent per period.
	pub budget: Balance,
	/// Length of a period, in blocks.
	pub period: BlockNumber,
	/// Maximum spent by a single call.
	pub max_per_call: Balance,
	/// First block at which the delegation is no longer valid, if any.
	pub expires_at: Option<BlockNumber>,
	/// Kinds of calls allowed.
	pub permissions: Permissions,
	/// Recipients and contracts allowed; any if empty.
	pub targets: BoundedVec<AccountId, MaxTargets>,
}

/// A delegation and its use.
#[derive(
	CloneNoBound,
	PartialEqNoBound,
	EqNoBound,
	DebugNoBound,
	Encode,
	Decode,
	TypeInfo,
	MaxEncodedLen,
)]
#[scale_info(skip_type_params(MaxTargets))]
pub struct Delegation<AccountId, Balance, BlockNumber, MaxTargets: Get<u32>>
where
	AccountId: Clone + PartialEq + Eq + core::fmt::Debug,
	Balance: Clone + PartialEq + Eq + core::fmt::Debug,
	BlockNumber: Clone + PartialEq + Eq + core::fmt::Debug,
{
	pub policy: Policy<AccountId, Balance, BlockNumber, MaxTargets>,
	/// Spent in the current period.
	pub spent: Balance,
	/// First block of the current period.
	pub period_start: BlockNumber,
	/// Deposit held from the owner.
	pub deposit: Balance,
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use alloc::boxed::Box;
	use frame_support::{
		dispatch::{GetDispatchInfo, PostDispatchInfo},
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate, MutateHold},
			tokens::Precision,
		},
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::{Dispatchable, Saturating, Zero};

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	pub type PolicyOf<T> = Policy<
		<T as frame_system::Config>::AccountId,
		BalanceOf<T>,
		BlockNumberFor<T>,
		<T as Config>::MaxTargets,
	>;

	pub type DelegationOf<T> = Delegation<
		<T as frame_system::Config>::AccountId,
		BalanceOf<T>,
		BlockNumberFor<T>,
		<T as Config>::MaxTargets,
	>;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The calls an agent can make.
		type RuntimeCall: Parameter
			+ Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
			+ GetDispatchInfo;

		/// The overarching hold reason.
		type RuntimeHoldReason: From<HoldReason>;

		/// The native currency.
		type Currency: Mutate<Self::AccountId>
			+ MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;

		/// Which calls are transfers, which are contract calls.
		type CallClassifier: ClassifyCall<<Self as Config>::RuntimeCall, Self::AccountId>;

		/// Maximum number of targets of a delegation.
		#[pallet::constant]
		type MaxTargets: Get<u32>;

		/// Deposit for a delegation.
		#[pallet::constant]
		type DepositBase: Get<BalanceOf<Self>>;

		/// Deposit for each target of a delegation.
		#[pallet::constant]
		type DepositPerTarget: Get<BalanceOf<Self>>;

		/// Weights of the calls.
		type WeightInfo: WeightInfo;

		/// Calls used by the benchmarks.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: BenchmarkHelper<<Self as Config>::RuntimeCall, Self::AccountId>;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Reasons for funds held by this pallet.
	#[pallet::composite_enum]
	pub enum HoldReason {
		/// Deposit for the storage of a delegation.
		Delegation,
	}

	/// Delegations, by owner and agent.
	#[pallet::storage]
	pub type Delegations<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Blake2_128Concat,
		T::AccountId,
		DelegationOf<T>,
		OptionQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// `owner` delegated to `agent`, or changed the limits of the delegation.
		AgentAuthorized { owner: T::AccountId, agent: T::AccountId },
		/// The delegation of `owner` to `agent` was revoked by the owner or renounced by the agent.
		AgentRevoked { owner: T::AccountId, agent: T::AccountId },
		/// `agent` made a call for `owner`, spending `spent`.
		AgentExecuted { owner: T::AccountId, agent: T::AccountId, spent: BalanceOf<T> },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// The agent has no delegation from the owner.
		NotDelegated,
		/// The delegation has expired.
		Expired,
		/// Agents cannot make this call.
		CallNotAllowed,
		/// The recipient or contract is not in the targets of the delegation.
		TargetNotAllowed,
		/// The call spends more than the limit per call.
		CallLimitExceeded,
		/// The call spends more than what is left of the budget of the period.
		BudgetExceeded,
		/// An account cannot be its own agent.
		SelfDelegation,
		/// The period must be at least one block.
		InvalidPeriod,
		/// The expiry is in the past.
		InvalidExpiry,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Delegate to `agent` within `policy`, or replace the policy of an existing delegation.
		/// The use of the current period is kept.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::authorize(policy.targets.len() as u32))]
		pub fn authorize(
			origin: OriginFor<T>,
			agent: T::AccountId,
			policy: PolicyOf<T>,
		) -> DispatchResult {
			let owner = ensure_signed(origin)?;
			ensure!(owner != agent, Error::<T>::SelfDelegation);
			ensure!(!policy.period.is_zero(), Error::<T>::InvalidPeriod);
			let now = frame_system::Pallet::<T>::block_number();
			if let Some(expires_at) = policy.expires_at {
				ensure!(expires_at > now, Error::<T>::InvalidExpiry);
			}

			let deposit = T::DepositBase::get().saturating_add(
				T::DepositPerTarget::get().saturating_mul((policy.targets.len() as u32).into()),
			);
			let previous = Delegations::<T>::get(&owner, &agent);
			let (spent, period_start, held) = match &previous {
				Some(d) => (d.spent, d.period_start, d.deposit),
				None => (Zero::zero(), now, Zero::zero()),
			};
			let reason = HoldReason::Delegation.into();
			if deposit > held {
				T::Currency::hold(&reason, &owner, deposit - held)?;
			} else if held > deposit {
				T::Currency::release(&reason, &owner, held - deposit, Precision::BestEffort)?;
			}

			Delegations::<T>::insert(
				&owner,
				&agent,
				Delegation { policy, spent, period_start, deposit },
			);
			Self::deposit_event(Event::AgentAuthorized { owner, agent });
			Ok(())
		}

		/// Revoke the delegation to `agent`, effective immediately.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::revoke())]
		pub fn revoke(origin: OriginFor<T>, agent: T::AccountId) -> DispatchResult {
			let owner = ensure_signed(origin)?;
			Self::remove(owner, agent)
		}

		/// Renounce the delegation of `owner`, as its agent.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::revoke())]
		pub fn renounce(origin: OriginFor<T>, owner: T::AccountId) -> DispatchResult {
			let agent = ensure_signed(origin)?;
			Self::remove(owner, agent)
		}

		/// Make `call` as `owner`, within the delegation of `owner` to the signer.
		#[pallet::call_index(3)]
		#[pallet::weight({
			let info = call.get_dispatch_info();
			(T::WeightInfo::execute().saturating_add(info.call_weight), info.class)
		})]
		pub fn execute(
			origin: OriginFor<T>,
			owner: T::AccountId,
			call: Box<<T as Config>::RuntimeCall>,
		) -> DispatchResultWithPostInfo {
			let agent = ensure_signed(origin)?;
			let mut delegation =
				Delegations::<T>::get(&owner, &agent).ok_or(Error::<T>::NotDelegated)?;
			let policy = &delegation.policy;
			let now = frame_system::Pallet::<T>::block_number();
			if let Some(expires_at) = policy.expires_at {
				ensure!(now < expires_at, Error::<T>::Expired);
			}

			let (allowed, target) = match T::CallClassifier::classify(&call) {
				CallKind::Transfer { to } => (policy.permissions.transfers, to),
				CallKind::Contract { to } => (policy.permissions.contracts, to),
				CallKind::Forbidden => (false, owner.clone()),
			};
			ensure!(allowed, Error::<T>::CallNotAllowed);
			ensure!(
				policy.targets.is_empty() || policy.targets.contains(&target),
				Error::<T>::TargetNotAllowed
			);

			if now >= delegation.period_start.saturating_add(policy.period) {
				delegation.period_start = now;
				delegation.spent = Zero::zero();
			}

			let before = T::Currency::balance(&owner);
			let post = call
				.dispatch(frame_system::RawOrigin::Signed(owner.clone()).into())
				.map_err(|e| e.error)?;
			let spent = before.saturating_sub(T::Currency::balance(&owner));

			ensure!(spent <= delegation.policy.max_per_call, Error::<T>::CallLimitExceeded);
			let total = delegation.spent.saturating_add(spent);
			ensure!(total <= delegation.policy.budget, Error::<T>::BudgetExceeded);
			delegation.spent = total;
			Delegations::<T>::insert(&owner, &agent, delegation);

			Self::deposit_event(Event::AgentExecuted { owner, agent, spent });
			Ok(post.actual_weight.map(|w| w.saturating_add(T::WeightInfo::execute())).into())
		}
	}

	impl<T: Config> Pallet<T> {
		fn remove(owner: T::AccountId, agent: T::AccountId) -> DispatchResult {
			let delegation =
				Delegations::<T>::take(&owner, &agent).ok_or(Error::<T>::NotDelegated)?;
			T::Currency::release(
				&HoldReason::Delegation.into(),
				&owner,
				delegation.deposit,
				Precision::BestEffort,
			)?;
			Self::deposit_event(Event::AgentRevoked { owner, agent });
			Ok(())
		}

		/// What `agent` can still spend for `owner` in the current period.
		pub fn remaining_budget(owner: &T::AccountId, agent: &T::AccountId) -> BalanceOf<T> {
			let Some(d) = Delegations::<T>::get(owner, agent) else { return Zero::zero() };
			let now = frame_system::Pallet::<T>::block_number();
			if d.policy.expires_at.is_some_and(|e| now >= e) {
				return Zero::zero();
			}
			if now >= d.period_start.saturating_add(d.policy.period) {
				return d.policy.budget;
			}
			d.policy.budget.saturating_sub(d.spent)
		}
	}
}
