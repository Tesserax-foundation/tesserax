//! On-chain governance: there is no sudo key.
//!
//! - Anyone proposes a call with a referendum, on the track that may dispatch it
//!   ([`tracks`]): root for anything, a runtime upgrade included; urgent for a call the technical
//!   committee whitelisted; cancellation to stop another referendum.
//! - Holders of TESS vote with conviction: locking their vote longer multiplies it, up to six
//!   times. TESS still vesting do not vote, nor count in the support a referendum needs: the
//!   allocations of the team and the foundation decide nothing while they vest.
//! - The technical committee is the three signatories of the foundation; two of them whitelist a
//!   call, which then needs a referendum of the urgent track, shorter than root. It can do
//!   nothing else, and its members are fixed: no call changes them.
//! - A call passed is scheduled, and dispatched with the origin of its track.

pub mod origins;
pub mod tracks;

use crate::{
	AccountId, Balance, Balances, BlockNumber, OriginCaller, Preimage, Referenda, Runtime, RuntimeCall, RuntimeEvent,
	RuntimeHoldReason, RuntimeOrigin, Scheduler, Vesting, MILLI_UNIT, UNIT,
};
use frame_support::{
	parameter_types,
	traits::{
		fungible::{self, HoldConsideration},
		tokens::{DepositConsequence, Fortitude, Preservation, Provenance, WithdrawConsequence},
		BalanceStatus, ConstU32, Currency, EitherOf, EqualPrivilegeOnly, ExistenceRequirement, Get,
		LinearStoragePrice, LockIdentifier, LockableCurrency, ReservableCurrency, SignedImbalance,
		VestingSchedule, WithdrawReasons,
	},
	weights::Weight,
};
use frame_system::{EnsureNever, EnsureRoot, EnsureSigned};
use sp_runtime::{DispatchError, DispatchResult, Perbill};

/// The technical committee: the signatories of the foundation.
pub type TechnicalCollective = pallet_collective::Instance1;

/// Two of the three members of the technical committee.
pub type TwoThirdsOfTheCommittee = pallet_collective::EnsureProportionAtLeast<AccountId, TechnicalCollective, 2, 3>;

/// Balances as governance sees them: the TESS still vesting are not there. Votes and
/// delegations are capped by this total balance; the locks they set are the locks of the
/// balances. Every other function is the one of the balances.
pub struct VotingBalances;

/// TESS of `who` still vesting.
fn vesting(who: &AccountId) -> Balance {
	<Vesting as VestingSchedule<AccountId>>::vesting_balance(who).unwrap_or_default()
}

impl Currency<AccountId> for VotingBalances {
	type Balance = Balance;
	type PositiveImbalance = <Balances as Currency<AccountId>>::PositiveImbalance;
	type NegativeImbalance = <Balances as Currency<AccountId>>::NegativeImbalance;

	fn total_balance(who: &AccountId) -> Balance {
		<Balances as Currency<AccountId>>::total_balance(who).saturating_sub(vesting(who))
	}
	fn can_slash(who: &AccountId, value: Balance) -> bool {
		<Balances as Currency<AccountId>>::can_slash(who, value)
	}
	fn total_issuance() -> Balance {
		<Balances as Currency<AccountId>>::total_issuance()
	}
	fn active_issuance() -> Balance {
		<Balances as Currency<AccountId>>::active_issuance()
	}
	fn deactivate(amount: Balance) {
		<Balances as Currency<AccountId>>::deactivate(amount)
	}
	fn reactivate(amount: Balance) {
		<Balances as Currency<AccountId>>::reactivate(amount)
	}
	fn minimum_balance() -> Balance {
		<Balances as Currency<AccountId>>::minimum_balance()
	}
	fn burn(amount: Balance) -> Self::PositiveImbalance {
		<Balances as Currency<AccountId>>::burn(amount)
	}
	fn issue(amount: Balance) -> Self::NegativeImbalance {
		<Balances as Currency<AccountId>>::issue(amount)
	}
	fn free_balance(who: &AccountId) -> Balance {
		<Balances as Currency<AccountId>>::free_balance(who)
	}
	fn ensure_can_withdraw(who: &AccountId, amount: Balance, reasons: WithdrawReasons, new_balance: Balance) -> DispatchResult {
		<Balances as Currency<AccountId>>::ensure_can_withdraw(who, amount, reasons, new_balance)
	}
	fn transfer(source: &AccountId, dest: &AccountId, value: Balance, existence: ExistenceRequirement) -> DispatchResult {
		<Balances as Currency<AccountId>>::transfer(source, dest, value, existence)
	}
	fn slash(who: &AccountId, value: Balance) -> (Self::NegativeImbalance, Balance) {
		<Balances as Currency<AccountId>>::slash(who, value)
	}
	fn deposit_into_existing(who: &AccountId, value: Balance) -> Result<Self::PositiveImbalance, DispatchError> {
		<Balances as Currency<AccountId>>::deposit_into_existing(who, value)
	}
	fn deposit_creating(who: &AccountId, value: Balance) -> Self::PositiveImbalance {
		<Balances as Currency<AccountId>>::deposit_creating(who, value)
	}
	fn withdraw(
		who: &AccountId,
		value: Balance,
		reasons: WithdrawReasons,
		liveness: ExistenceRequirement,
	) -> Result<Self::NegativeImbalance, DispatchError> {
		<Balances as Currency<AccountId>>::withdraw(who, value, reasons, liveness)
	}
	fn make_free_balance_be(who: &AccountId, balance: Balance) -> SignedImbalance<Balance, Self::PositiveImbalance> {
		<Balances as Currency<AccountId>>::make_free_balance_be(who, balance)
	}
}

impl ReservableCurrency<AccountId> for VotingBalances {
	fn can_reserve(who: &AccountId, value: Balance) -> bool {
		<Balances as ReservableCurrency<AccountId>>::can_reserve(who, value)
	}
	fn slash_reserved(who: &AccountId, value: Balance) -> (Self::NegativeImbalance, Balance) {
		<Balances as ReservableCurrency<AccountId>>::slash_reserved(who, value)
	}
	fn reserved_balance(who: &AccountId) -> Balance {
		<Balances as ReservableCurrency<AccountId>>::reserved_balance(who)
	}
	fn reserve(who: &AccountId, value: Balance) -> DispatchResult {
		<Balances as ReservableCurrency<AccountId>>::reserve(who, value)
	}
	fn unreserve(who: &AccountId, value: Balance) -> Balance {
		<Balances as ReservableCurrency<AccountId>>::unreserve(who, value)
	}
	fn repatriate_reserved(
		slashed: &AccountId,
		beneficiary: &AccountId,
		value: Balance,
		status: BalanceStatus,
	) -> Result<Balance, DispatchError> {
		<Balances as ReservableCurrency<AccountId>>::repatriate_reserved(slashed, beneficiary, value, status)
	}
}

impl LockableCurrency<AccountId> for VotingBalances {
	type Moment = BlockNumber;
	type MaxLocks = <Balances as LockableCurrency<AccountId>>::MaxLocks;

	fn set_lock(id: LockIdentifier, who: &AccountId, amount: Balance, reasons: WithdrawReasons) {
		<Balances as LockableCurrency<AccountId>>::set_lock(id, who, amount, reasons)
	}
	fn extend_lock(id: LockIdentifier, who: &AccountId, amount: Balance, reasons: WithdrawReasons) {
		<Balances as LockableCurrency<AccountId>>::extend_lock(id, who, amount, reasons)
	}
	fn remove_lock(id: LockIdentifier, who: &AccountId) {
		<Balances as LockableCurrency<AccountId>>::remove_lock(id, who)
	}
}

impl fungible::Inspect<AccountId> for VotingBalances {
	type Balance = Balance;

	fn total_issuance() -> Balance {
		<Balances as fungible::Inspect<AccountId>>::total_issuance()
	}
	fn active_issuance() -> Balance {
		<Balances as fungible::Inspect<AccountId>>::active_issuance()
	}
	fn minimum_balance() -> Balance {
		<Balances as fungible::Inspect<AccountId>>::minimum_balance()
	}
	fn total_balance(who: &AccountId) -> Balance {
		<Balances as fungible::Inspect<AccountId>>::total_balance(who).saturating_sub(vesting(who))
	}
	fn balance(who: &AccountId) -> Balance {
		<Balances as fungible::Inspect<AccountId>>::balance(who)
	}
	fn reducible_balance(who: &AccountId, preservation: Preservation, force: Fortitude) -> Balance {
		<Balances as fungible::Inspect<AccountId>>::reducible_balance(who, preservation, force)
	}
	fn can_deposit(who: &AccountId, amount: Balance, provenance: Provenance) -> DepositConsequence {
		<Balances as fungible::Inspect<AccountId>>::can_deposit(who, amount, provenance)
	}
	fn can_withdraw(who: &AccountId, amount: Balance) -> WithdrawConsequence<Balance> {
		<Balances as fungible::Inspect<AccountId>>::can_withdraw(who, amount)
	}
}

/// The TESS that may vote: the total issuance without what the genesis allocations still vest.
/// Support is measured against it, so that it does not count TESS that cannot vote.
pub struct VotableIssuance;

impl Get<Balance> for VotableIssuance {
	fn get() -> Balance {
		let vesting: Balance = pallet_allocations::Allocations::<Runtime>::iter_keys().map(|who| vesting(&who)).sum();
		<Balances as Currency<AccountId>>::total_issuance().saturating_sub(vesting)
	}
}

parameter_types! {
	/// A vote with a conviction of 1 stays locked a week after the referendum; each conviction
	/// doubles it, up to 32 weeks at 6x.
	pub const VoteLockingPeriod: BlockNumber = 7 * tracks::DAY;
}

impl pallet_conviction_voting::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = pallet_conviction_voting::weights::SubstrateWeight<Runtime>;
	type Currency = VotingBalances;
	type Polls = Referenda;
	type MaxTurnout = VotableIssuance;
	type MaxVotes = ConstU32<512>;
	type VoteLockingPeriod = VoteLockingPeriod;
	type BlockNumberProvider = crate::System;
	type VotingHooks = ();
}

parameter_types! {
	pub const SubmissionDeposit: Balance = 10 * UNIT;
	pub const UndecidingTimeout: BlockNumber = 14 * tracks::DAY;
}

impl pallet_referenda::Config for Runtime {
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = pallet_referenda::weights::SubstrateWeight<Runtime>;
	type Scheduler = Scheduler;
	type Currency = Balances;
	type SubmitOrigin = EnsureSigned<AccountId>;
	type CancelOrigin = EitherOf<EnsureRoot<AccountId>, origins::ReferendumCanceller>;
	type KillOrigin = EitherOf<EnsureRoot<AccountId>, origins::ReferendumCanceller>;
	type Slash = crate::configs::ToAuthor;
	type Votes = pallet_conviction_voting::VotesOf<Runtime>;
	type Tally = pallet_conviction_voting::TallyOf<Runtime>;
	type SubmissionDeposit = SubmissionDeposit;
	type MaxQueued = ConstU32<100>;
	type UndecidingTimeout = UndecidingTimeout;
	type AlarmInterval = ConstU32<1>;
	type Tracks = tracks::TracksInfo;
	type Preimages = Preimage;
	type BlockNumberProvider = crate::System;
}

impl origins::Config for Runtime {}

/// Calls the technical committee whitelisted are dispatched as root by a referendum of the urgent
/// track.
impl pallet_whitelist::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type WhitelistOrigin = TwoThirdsOfTheCommittee;
	type DispatchWhitelistedOrigin = EitherOf<EnsureRoot<AccountId>, origins::WhitelistedCaller>;
	type Preimages = Preimage;
	type WeightInfo = pallet_whitelist::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	pub const CommitteeMotionDuration: BlockNumber = 2 * tracks::DAY;
	pub CommitteeMaxProposalWeight: Weight = Perbill::from_percent(50) * crate::configs::RuntimeBlockWeights::get().max_block;
}

/// The technical committee, whose members are set at genesis and never change.
impl pallet_collective::Config<TechnicalCollective> for Runtime {
	type RuntimeOrigin = RuntimeOrigin;
	type Proposal = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type MotionDuration = CommitteeMotionDuration;
	type MaxProposals = ConstU32<16>;
	type MaxMembers = ConstU32<3>;
	type DefaultVote = pallet_collective::MoreThanMajorityThenPrimeDefaultVote;
	type WeightInfo = pallet_collective::weights::SubstrateWeight<Runtime>;
	type SetMembersOrigin = EnsureNever<()>;
	type MaxProposalWeight = CommitteeMaxProposalWeight;
	type DisapproveOrigin = EnsureNever<()>;
	type KillOrigin = EnsureNever<()>;
	type Consideration = ();
}

parameter_types! {
	pub const PreimageBaseDeposit: Balance = 2 * UNIT;
	/// A runtime of 900 KiB holds about 92 TESS while its referendum lasts.
	pub const PreimageByteDeposit: Balance = MILLI_UNIT / 10;
	pub const PreimageHoldReason: RuntimeHoldReason = RuntimeHoldReason::Preimage(pallet_preimage::HoldReason::Preimage);
}

/// Calls of referenda, stored until they are dispatched.
impl pallet_preimage::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = pallet_preimage::weights::SubstrateWeight<Runtime>;
	type Currency = Balances;
	type ManagerOrigin = EnsureRoot<AccountId>;
	type Consideration = HoldConsideration<
		AccountId,
		Balances,
		PreimageHoldReason,
		LinearStoragePrice<PreimageBaseDeposit, PreimageByteDeposit, Balance>,
	>;
}

parameter_types! {
	pub MaximumSchedulerWeight: Weight = Perbill::from_percent(80) * crate::configs::RuntimeBlockWeights::get().max_block;
}

/// Calls passed by referenda, dispatched at their block.
impl pallet_scheduler::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeOrigin = RuntimeOrigin;
	type PalletsOrigin = OriginCaller;
	type RuntimeCall = RuntimeCall;
	type MaximumWeight = MaximumSchedulerWeight;
	type ScheduleOrigin = EnsureRoot<AccountId>;
	type OriginPrivilegeCmp = EqualPrivilegeOnly;
	type MaxScheduledPerBlock = ConstU32<50>;
	type WeightInfo = pallet_scheduler::weights::SubstrateWeight<Runtime>;
	type Preimages = Preimage;
	type BlockNumberProvider = crate::System;
}
