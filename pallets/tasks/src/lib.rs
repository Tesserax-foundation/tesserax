//! # Tasks pallet
//!
//! A task market with escrow: clients order work from agents, and the chain holds the reward
//! until the work is delivered.
//!
//! 1. A client creates a task: a specification (a URI or a short text), an optional capability
//!    tag, a reward held from its account, an optional invited worker, an optional bond the
//!    worker must hold, a delivery period and a review period.
//! 2. A worker accepts the task and holds the bond. It has the delivery period to deliver a
//!    result (a URI or a short text).
//! 3. The client approves the result, and the worker is paid. If the client does nothing during
//!    the review period, the result is approved automatically: anyone finalizes the task and the
//!    worker is paid.
//! 4. If the worker does not deliver in time, anyone finalizes the task: the reward returns to
//!    the client with the worker's bond.
//!
//! Disputes need an arbiter, named by the client when creating the task with its fee. During
//! the review period the client can dispute the result; the arbiter then decides the share of
//! the reward paid to the worker and receives its fee. If the arbiter does not decide within
//! `ArbitrationPeriod`, the result is approved automatically. Without arbiter, results are
//! approved by the client or automatically: a client cannot withhold a delivered reward.
//!
//! Every outcome is recorded in the reputation of the worker in the agent registry.

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

/// Identifier of a task.
pub type TaskId = u64;

/// Maximum length of a specification or a result, in bytes.
pub const MAX_TEXT: u32 = 256;
/// Maximum length of a capability tag, in bytes.
pub const MAX_CAPABILITY: u32 = 32;

/// A specification or a result: a URI or a short text.
pub type Text = BoundedVec<u8, ConstU32<MAX_TEXT>>;
/// A capability tag, such as `translation`.
pub type Capability = BoundedVec<u8, ConstU32<MAX_CAPABILITY>>;

/// The arbiter of a task and its fee.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen)]
pub struct Arbiter<AccountId, Balance> {
	pub account: AccountId,
	/// Paid by the client if the arbiter decides a dispute.
	pub fee: Balance,
}

/// Where a task stands.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub enum Status<AccountId, BlockNumber> {
	/// Waiting for a worker.
	Open,
	/// Accepted by `worker`, to be delivered before `deadline`.
	Assigned { worker: AccountId, deadline: BlockNumber },
	/// Delivered; approved automatically from `review_until` unless disputed.
	Delivered { worker: AccountId, result: Text, review_until: BlockNumber },
	/// Disputed; the arbiter decides before `until`, or the result is approved.
	Disputed { worker: AccountId, result: Text, until: BlockNumber },
}

/// A task.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct TaskInfo<AccountId, Balance, BlockNumber> {
	pub client: AccountId,
	pub spec: Text,
	pub capability: Option<Capability>,
	/// Held from the client, paid to the worker.
	pub reward: Balance,
	/// The only worker allowed to accept, if any.
	pub invited: Option<AccountId>,
	pub arbiter: Option<Arbiter<AccountId, Balance>>,
	/// Held from the worker while the task is assigned; lost if it does not deliver.
	pub bond: Balance,
	/// Blocks the worker has to deliver after accepting.
	pub delivery_period: BlockNumber,
	/// Blocks the client has to approve or dispute after the delivery.
	pub review_period: BlockNumber,
	pub status: Status<AccountId, BlockNumber>,
	/// Storage deposit held from the client.
	pub storage_deposit: Balance,
}

/// How a task ended, for the reputation of its worker.
pub trait ReputationRecorder<AccountId> {
	fn record_task(worker: &AccountId, completed: bool);
}

impl<AccountId> ReputationRecorder<AccountId> for () {
	fn record_task(_: &AccountId, _: bool) {}
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate, MutateHold},
			tokens::{Fortitude, Precision, Restriction},
		},
		PalletId,
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::{
		traits::{AccountIdConversion, Saturating, Zero},
		Percent,
	};

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	pub type TaskOf<T> =
		TaskInfo<<T as frame_system::Config>::AccountId, BalanceOf<T>, BlockNumberFor<T>>;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The overarching hold reason.
		type RuntimeHoldReason: From<HoldReason>;

		/// The native currency.
		type Currency: Mutate<Self::AccountId>
			+ MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;

		/// Records the outcome of tasks in the reputation of workers.
		type Reputation: ReputationRecorder<Self::AccountId>;

		/// Identifier of the task market, for delegation targets.
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		/// Minimum reward of a task.
		#[pallet::constant]
		type MinReward: Get<BalanceOf<Self>>;

		/// Deposit for the storage of a task.
		#[pallet::constant]
		type StorageDeposit: Get<BalanceOf<Self>>;

		/// Shortest delivery and review periods.
		#[pallet::constant]
		type MinPeriod: Get<BlockNumberFor<Self>>;

		/// Longest delivery and review periods.
		#[pallet::constant]
		type MaxPeriod: Get<BlockNumberFor<Self>>;

		/// Blocks an arbiter has to decide a dispute.
		#[pallet::constant]
		type ArbitrationPeriod: Get<BlockNumberFor<Self>>;

		/// Weights of the calls.
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Reasons for funds held by this pallet.
	#[pallet::composite_enum]
	pub enum HoldReason {
		/// Reward, arbiter fee and storage deposit of a client, or bond of a worker.
		Task,
	}

	/// Identifier of the next task.
	#[pallet::storage]
	pub type NextTaskId<T: Config> = StorageValue<_, TaskId, ValueQuery>;

	/// Tasks in progress. Finished tasks are removed; their events remain.
	#[pallet::storage]
	pub type Tasks<T: Config> = StorageMap<_, Blake2_128Concat, TaskId, TaskOf<T>, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A task was created.
		Created { id: TaskId, client: T::AccountId, reward: BalanceOf<T>, capability: Option<Capability> },
		/// The client cancelled a task no worker had accepted.
		Cancelled { id: TaskId },
		/// A worker accepted a task.
		Accepted { id: TaskId, worker: T::AccountId, deadline: BlockNumberFor<T> },
		/// The worker delivered `result`.
		Delivered { id: TaskId, worker: T::AccountId, result: Text, review_until: BlockNumberFor<T> },
		/// The client disputed the result.
		Disputed { id: TaskId, until: BlockNumberFor<T> },
		/// The task is finished: `paid` went to the worker, `refunded` to the client.
		Completed { id: TaskId, worker: T::AccountId, paid: BalanceOf<T>, refunded: BalanceOf<T> },
		/// The worker did not deliver in time: the reward and its bond went to the client.
		Expired { id: TaskId, worker: T::AccountId },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// No task has this identifier.
		UnknownTask,
		/// The reward is below the minimum.
		RewardTooLow,
		/// A period is out of bounds.
		InvalidPeriod,
		/// The arbiter fee is too small to be paid.
		InvalidFee,
		/// The client cannot work on, invite itself to or arbitrate its own task, nor can the
		/// worker arbitrate it.
		ConflictOfInterest,
		/// Only the client can do this.
		NotClient,
		/// Only the worker can do this.
		NotWorker,
		/// Only the arbiter can do this.
		NotArbiter,
		/// Another worker is invited.
		NotInvited,
		/// The task is not in the right state for this.
		WrongStatus,
		/// The delivery deadline has passed.
		DeadlinePassed,
		/// The review period is over.
		ReviewOver,
		/// The task has no arbiter: results are approved by the client or automatically.
		NoArbiter,
		/// The task cannot be finalized yet.
		NotFinalizable,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			assert!(T::MinPeriod::get() <= T::MaxPeriod::get(), "MinPeriod must not exceed MaxPeriod");
			assert!(!T::ArbitrationPeriod::get().is_zero(), "the arbitration period must not be zero");
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Create a task, holding `reward`, the arbiter fee and a storage deposit.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::create())]
		pub fn create(
			origin: OriginFor<T>,
			spec: Text,
			capability: Option<Capability>,
			reward: BalanceOf<T>,
			invited: Option<T::AccountId>,
			arbiter: Option<Arbiter<T::AccountId, BalanceOf<T>>>,
			bond: BalanceOf<T>,
			delivery_period: BlockNumberFor<T>,
			review_period: BlockNumberFor<T>,
		) -> DispatchResult {
			let client = ensure_signed(origin)?;
			ensure!(reward >= T::MinReward::get(), Error::<T>::RewardTooLow);
			let periods = T::MinPeriod::get()..=T::MaxPeriod::get();
			ensure!(
				periods.contains(&delivery_period) && periods.contains(&review_period),
				Error::<T>::InvalidPeriod
			);
			ensure!(invited.as_ref() != Some(&client), Error::<T>::ConflictOfInterest);
			let mut fee = Zero::zero();
			if let Some(arbiter) = &arbiter {
				ensure!(arbiter.account != client, Error::<T>::ConflictOfInterest);
				ensure!(invited.as_ref() != Some(&arbiter.account), Error::<T>::ConflictOfInterest);
				ensure!(
					arbiter.fee.is_zero() || arbiter.fee >= T::Currency::minimum_balance(),
					Error::<T>::InvalidFee
				);
				fee = arbiter.fee;
			}

			let storage_deposit = T::StorageDeposit::get();
			T::Currency::hold(
				&HoldReason::Task.into(),
				&client,
				reward.saturating_add(fee).saturating_add(storage_deposit),
			)?;
			let id = NextTaskId::<T>::mutate(|next| {
				let id = *next;
				*next = next.saturating_add(1);
				id
			});
			Tasks::<T>::insert(
				id,
				TaskInfo {
					client: client.clone(),
					spec,
					capability: capability.clone(),
					reward,
					invited,
					arbiter,
					bond,
					delivery_period,
					review_period,
					status: Status::Open,
					storage_deposit,
				},
			);
			Self::deposit_event(Event::Created { id, client, reward, capability });
			Ok(())
		}

		/// Cancel a task no worker has accepted, as its client.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::cancel())]
		pub fn cancel(origin: OriginFor<T>, id: TaskId) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let task = Tasks::<T>::get(id).ok_or(Error::<T>::UnknownTask)?;
			ensure!(task.client == who, Error::<T>::NotClient);
			ensure!(task.status == Status::Open, Error::<T>::WrongStatus);
			Self::release_client(&task, task.reward)?;
			Tasks::<T>::remove(id);
			Self::deposit_event(Event::Cancelled { id });
			Ok(())
		}

		/// Accept a task as its worker, holding the bond.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::accept())]
		pub fn accept(origin: OriginFor<T>, id: TaskId) -> DispatchResult {
			let worker = ensure_signed(origin)?;
			let deadline = Tasks::<T>::try_mutate(id, |task| -> Result<_, DispatchError> {
				let task = task.as_mut().ok_or(Error::<T>::UnknownTask)?;
				ensure!(task.status == Status::Open, Error::<T>::WrongStatus);
				ensure!(task.client != worker, Error::<T>::ConflictOfInterest);
				ensure!(task.arbiter.as_ref().is_none_or(|a| a.account != worker), Error::<T>::ConflictOfInterest);
				ensure!(task.invited.as_ref().is_none_or(|i| *i == worker), Error::<T>::NotInvited);
				T::Currency::hold(&HoldReason::Task.into(), &worker, task.bond)?;
				let deadline = frame_system::Pallet::<T>::block_number().saturating_add(task.delivery_period);
				task.status = Status::Assigned { worker: worker.clone(), deadline };
				Ok(deadline)
			})?;
			Self::deposit_event(Event::Accepted { id, worker, deadline });
			Ok(())
		}

		/// Deliver `result` as the worker, before the deadline.
		#[pallet::call_index(3)]
		#[pallet::weight(T::WeightInfo::deliver())]
		pub fn deliver(origin: OriginFor<T>, id: TaskId, result: Text) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let now = frame_system::Pallet::<T>::block_number();
			let review_until = Tasks::<T>::try_mutate(id, |task| -> Result<_, DispatchError> {
				let task = task.as_mut().ok_or(Error::<T>::UnknownTask)?;
				let Status::Assigned { worker, deadline } = &task.status else {
					return Err(Error::<T>::WrongStatus.into());
				};
				ensure!(*worker == who, Error::<T>::NotWorker);
				ensure!(now <= *deadline, Error::<T>::DeadlinePassed);
				let review_until = now.saturating_add(task.review_period);
				task.status = Status::Delivered { worker: who.clone(), result: result.clone(), review_until };
				Ok(review_until)
			})?;
			Self::deposit_event(Event::Delivered { id, worker: who, result, review_until });
			Ok(())
		}

		/// Approve the result as the client: the worker is paid.
		#[pallet::call_index(4)]
		#[pallet::weight(T::WeightInfo::complete())]
		pub fn approve(origin: OriginFor<T>, id: TaskId) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let task = Tasks::<T>::get(id).ok_or(Error::<T>::UnknownTask)?;
			ensure!(task.client == who, Error::<T>::NotClient);
			let Status::Delivered { worker, .. } = task.status.clone() else {
				return Err(Error::<T>::WrongStatus.into());
			};
			Self::complete(id, task, worker, Percent::one(), false)
		}

		/// Dispute the result as the client, during the review period. Needs an arbiter.
		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::dispute())]
		pub fn dispute(origin: OriginFor<T>, id: TaskId) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let now = frame_system::Pallet::<T>::block_number();
			let until = Tasks::<T>::try_mutate(id, |task| -> Result<_, DispatchError> {
				let task = task.as_mut().ok_or(Error::<T>::UnknownTask)?;
				ensure!(task.client == who, Error::<T>::NotClient);
				ensure!(task.arbiter.is_some(), Error::<T>::NoArbiter);
				let Status::Delivered { worker, result, review_until } = &task.status else {
					return Err(Error::<T>::WrongStatus.into());
				};
				ensure!(now < *review_until, Error::<T>::ReviewOver);
				let until = now.saturating_add(T::ArbitrationPeriod::get());
				task.status = Status::Disputed { worker: worker.clone(), result: result.clone(), until };
				Ok(until)
			})?;
			Self::deposit_event(Event::Disputed { id, until });
			Ok(())
		}

		/// Decide a dispute as the arbiter: `worker_share` of the reward goes to the worker, the
		/// rest returns to the client, and the arbiter receives its fee.
		#[pallet::call_index(6)]
		#[pallet::weight(T::WeightInfo::complete())]
		pub fn resolve(origin: OriginFor<T>, id: TaskId, worker_share: Percent) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let task = Tasks::<T>::get(id).ok_or(Error::<T>::UnknownTask)?;
			ensure!(task.arbiter.as_ref().is_some_and(|a| a.account == who), Error::<T>::NotArbiter);
			let Status::Disputed { worker, .. } = task.status.clone() else {
				return Err(Error::<T>::WrongStatus.into());
			};
			Self::complete(id, task, worker, worker_share, true)
		}

		/// Finalize a task whose time is up; anyone can call it.
		///
		/// - Delivered and not disputed during the review period: the result is approved and the
		///   worker paid.
		/// - Disputed and not decided during the arbitration period: likewise.
		/// - Not delivered before the deadline: the reward and the worker's bond go to the
		///   client.
		#[pallet::call_index(7)]
		#[pallet::weight(T::WeightInfo::complete())]
		pub fn finalize(origin: OriginFor<T>, id: TaskId) -> DispatchResult {
			ensure_signed(origin)?;
			let task = Tasks::<T>::get(id).ok_or(Error::<T>::UnknownTask)?;
			let now = frame_system::Pallet::<T>::block_number();
			match task.status.clone() {
				Status::Delivered { worker, review_until, .. } if now >= review_until =>
					Self::complete(id, task, worker, Percent::one(), false),
				Status::Disputed { worker, until, .. } if now >= until =>
					Self::complete(id, task, worker, Percent::one(), false),
				Status::Assigned { worker, deadline } if now > deadline => Self::expire(id, task, worker),
				_ => Err(Error::<T>::NotFinalizable.into()),
			}
		}
	}

	impl<T: Config> Pallet<T> {
		/// Account standing for the task market in delegation targets. It holds no funds.
		pub fn account_id() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}

		fn hold_reason() -> T::RuntimeHoldReason {
			HoldReason::Task.into()
		}

		/// Return to the client `amount` of the reward, the storage deposit and, unless paid,
		/// the arbiter fee.
		fn release_client(task: &TaskOf<T>, amount: BalanceOf<T>) -> DispatchResult {
			let fee = task.arbiter.as_ref().map(|a| a.fee).unwrap_or_else(Zero::zero);
			T::Currency::release(
				&Self::hold_reason(),
				&task.client,
				amount.saturating_add(fee).saturating_add(task.storage_deposit),
				Precision::BestEffort,
			)?;
			Ok(())
		}

		fn pay(from: &T::AccountId, to: &T::AccountId, amount: BalanceOf<T>) -> DispatchResult {
			if !amount.is_zero() {
				T::Currency::transfer_on_hold(
					&Self::hold_reason(),
					from,
					to,
					amount,
					Precision::Exact,
					Restriction::Free,
					Fortitude::Polite,
				)?;
			}
			Ok(())
		}

		/// Pay `worker_share` of the reward to the worker, the rest to the client, and the
		/// arbiter fee to the arbiter if `arbitrated`.
		fn complete(
			id: TaskId,
			task: TaskOf<T>,
			worker: T::AccountId,
			worker_share: Percent,
			arbitrated: bool,
		) -> DispatchResult {
			let paid = worker_share.mul_floor(task.reward);
			let refunded = task.reward.saturating_sub(paid);
			Self::pay(&task.client, &worker, paid)?;
			let mut client_refund = refunded.saturating_add(task.storage_deposit);
			match &task.arbiter {
				Some(arbiter) if arbitrated => Self::pay(&task.client, &arbiter.account, arbiter.fee)?,
				Some(arbiter) => client_refund.saturating_accrue(arbiter.fee),
				None => {},
			}
			T::Currency::release(&Self::hold_reason(), &task.client, client_refund, Precision::BestEffort)?;

			// The worker keeps its bond unless it gets nothing.
			if paid.is_zero() {
				Self::pay(&worker, &task.client, task.bond)?;
			} else {
				T::Currency::release(&Self::hold_reason(), &worker, task.bond, Precision::BestEffort)?;
			}
			T::Reputation::record_task(&worker, worker_share >= Percent::from_percent(50));
			Tasks::<T>::remove(id);
			Self::deposit_event(Event::Completed { id, worker, paid, refunded });
			Ok(())
		}

		/// The worker did not deliver: the reward returns to the client with the worker's bond.
		fn expire(id: TaskId, task: TaskOf<T>, worker: T::AccountId) -> DispatchResult {
			Self::release_client(&task, task.reward)?;
			Self::pay(&worker, &task.client, task.bond)?;
			T::Reputation::record_task(&worker, false);
			Tasks::<T>::remove(id);
			Self::deposit_event(Event::Expired { id, worker });
			Ok(())
		}
	}
}
