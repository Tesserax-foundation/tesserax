//! Governance: referenda in place of sudo, votes without the TESS still vesting, and a technical
//! committee that only whitelists.

use super::*;
use super::allocations::ext_of;
use frame_support::{
	assert_noop, assert_ok,
	traits::{Bounded, OnInitialize, OnRuntimeUpgrade, StorePreimage},
	weights::Weight,
};
use crate::governance::{self as gov, origins, tracks, TechnicalCollective};
use pallet_conviction_voting::{AccountVote, Conviction, Vote};
use sp_runtime::{
	traits::{Dispatchable, Hash as _},
	DispatchError, DispatchResult,
};

const KEY: &[u8] = b":governance-test";

/// A call only root may dispatch: it writes `value` at `KEY`.
fn root_call(value: &[u8]) -> RuntimeCall {
	RuntimeCall::System(frame_system::Call::set_storage { items: vec![(KEY.to_vec(), value.to_vec())] })
}

fn written() -> Option<Vec<u8>> {
	sp_io::storage::get(KEY).map(|v| v.to_vec())
}

fn dev() -> sp_io::TestExternalities {
	ext_of(sp_genesis_builder::DEV_RUNTIME_PRESET)
}

fn signed(who: &Keyring) -> RuntimeOrigin {
	RuntimeOrigin::signed(who.to_account_id())
}

/// Blocks up to `n`: the scheduler serves the referenda and dispatches what they passed.
fn run_to(n: BlockNumber) {
	while System::block_number() < n {
		let next = System::block_number() + 1;
		System::set_block_number(next);
		Scheduler::on_initialize(next);
	}
}

/// Submit `proposal` on the track of `origin`, place its decision deposit, and return its index.
fn submit(who: &Keyring, origin: OriginCaller, proposal: RuntimeCall) -> u32 {
	let index = pallet_referenda::ReferendumCount::<Runtime>::get();
	let bounded: Bounded<RuntimeCall, <Runtime as frame_system::Config>::Hashing> = Preimage::bound(proposal).unwrap();
	assert_ok!(Referenda::submit(
		signed(who),
		Box::new(origin),
		bounded,
		frame_support::traits::schedule::DispatchTime::After(0),
	));
	assert_ok!(Referenda::place_decision_deposit(signed(who), index));
	index
}

fn aye(balance: Balance) -> AccountVote<Balance> {
	AccountVote::Standard { vote: Vote { aye: true, conviction: Conviction::Locked1x }, balance }
}

fn approved(index: u32) -> bool {
	matches!(pallet_referenda::ReferendumInfoFor::<Runtime>::get(index), Some(pallet_referenda::ReferendumInfo::Approved(..)))
}

#[test]
fn a_root_referendum_dispatches_as_root() {
	dev().execute_with(|| {
		// Signed accounts reach root through a referendum only.
		assert_eq!(root_call(b"signed").dispatch(signed(&Keyring::Alice)).map_err(|e| e.error), Err(DispatchError::BadOrigin));

		let index = submit(&Keyring::Alice, frame_system::RawOrigin::Root.into(), root_call(b"root"));
		assert_ok!(ConvictionVoting::vote(signed(&Keyring::Bob), index, aye(500_000 * UNIT)));

		// Preparation, confirmation, then the enactment delay: nothing is dispatched before.
		run_to(System::block_number() + 2 * tracks::HOUR + tracks::DAY);
		assert_eq!(written(), None);
		run_to(System::block_number() + tracks::DAY + 10);
		assert!(approved(index));
		assert_eq!(written(), Some(b"root".to_vec()));
	});
}

#[test]
fn a_referendum_without_support_fails() {
	dev().execute_with(|| {
		let index = submit(&Keyring::Alice, frame_system::RawOrigin::Root.into(), root_call(b"root"));
		// A hundredth of a percent of the TESS that may vote is far from the support needed.
		assert_ok!(ConvictionVoting::vote(signed(&Keyring::Bob), index, aye(100 * UNIT)));
		run_to(System::block_number() + 2 * tracks::HOUR + 14 * tracks::DAY + tracks::DAY + 10);
		assert!(matches!(
			pallet_referenda::ReferendumInfoFor::<Runtime>::get(index),
			Some(pallet_referenda::ReferendumInfo::Rejected(..))
		));
		assert_eq!(written(), None);
	});
}

#[test]
fn vesting_tess_do_not_vote() {
	dev().execute_with(|| {
		let index = submit(&Keyring::Alice, frame_system::RawOrigin::Root.into(), root_call(b"root"));
		// Dave holds half of the development team allocation, all of it vesting.
		let dave = Keyring::Dave.to_account_id();
		assert!(Balances::total_balance(&dave) >= TEAM_ALLOCATION / 2);
		assert_noop!(
			ConvictionVoting::vote(signed(&Keyring::Dave), index, aye(UNIT)),
			pallet_conviction_voting::Error::<Runtime>::InsufficientFunds
		);
		assert_noop!(
			ConvictionVoting::delegate(
				signed(&Keyring::Dave),
				tracks::ROOT,
				Keyring::Ferdie.to_account_id().into(),
				Conviction::Locked6x,
				UNIT
			),
			pallet_conviction_voting::Error::<Runtime>::InsufficientFunds
		);

		// The foundation votes with its liquid part only.
		let foundation = super::allocations::foundation();
		assert_noop!(
			ConvictionVoting::vote(RuntimeOrigin::signed(foundation.clone()), index, aye(FOUNDATION_LIQUID + UNIT)),
			pallet_conviction_voting::Error::<Runtime>::InsufficientFunds
		);
		assert_ok!(ConvictionVoting::vote(RuntimeOrigin::signed(foundation), index, aye(FOUNDATION_LIQUID)));
	});
}

#[test]
fn support_counts_the_tess_that_may_vote() {
	dev().execute_with(|| {
		// What vests at genesis: the whole team allocation, and the foundation but its liquid part,
		// less the little the foundation released since.
		let vesting = TEAM_ALLOCATION + FOUNDATION_ALLOCATION - FOUNDATION_LIQUID;
		let issuance = <Balances as frame_support::traits::fungible::Inspect<_>>::total_issuance();
		let votable = <gov::VotableIssuance as frame_support::traits::Get<Balance>>::get();
		assert!((issuance - vesting..issuance - vesting + UNIT).contains(&votable), "{votable}");
	});
}

fn whitelist_proposal(call: &RuntimeCall) -> RuntimeCall {
	RuntimeCall::Whitelist(pallet_whitelist::Call::whitelist_call {
		call_hash: <Runtime as frame_system::Config>::Hashing::hash_of(call),
	})
}

fn whitelisted(call: &RuntimeCall) -> bool {
	pallet_whitelist::WhitelistedCall::<Runtime>::contains_key(<Runtime as frame_system::Config>::Hashing::hash_of(call))
}

/// Whitelist `call` with the votes of `members` of the technical committee, two needed.
fn whitelist(members: &[Keyring], call: &RuntimeCall) -> DispatchResult {
	let proposal = whitelist_proposal(call);
	let length = proposal.encoded_size() as u32;
	let index = pallet_collective::ProposalCount::<Runtime, TechnicalCollective>::get();
	let proposal_hash = <Runtime as frame_system::Config>::Hashing::hash_of(&proposal);
	TechnicalCommittee::propose(signed(&members[0]), 2, Box::new(proposal.clone()), length).map_err(|e| e.error)?;
	// The member proposing votes too.
	for member in members {
		TechnicalCommittee::vote(signed(member), proposal_hash, index, true).map_err(|e| e.error)?;
	}
	TechnicalCommittee::close(signed(&members[0]), proposal_hash, index, Weight::MAX, length).map(|_| ()).map_err(|e| e.error)
}

#[test]
fn the_committee_whitelists_for_the_urgent_track() {
	dev().execute_with(|| {
		let call = root_call(b"urgent");
		// One member is not enough: a motion of one executes with a third of the committee.
		let proposal = whitelist_proposal(&call);
		assert_ok!(TechnicalCommittee::propose(signed(&Keyring::Alice), 1, Box::new(proposal.clone()), proposal.encoded_size() as u32));
		assert!(!whitelisted(&call));
		// No one outside the committee may whitelist.
		assert_noop!(
			Whitelist::whitelist_call(signed(&Keyring::Dave), <Runtime as frame_system::Config>::Hashing::hash_of(&call)),
			DispatchError::BadOrigin
		);
		assert_ok!(whitelist(&[Keyring::Alice, Keyring::Bob], &call));
		assert!(whitelisted(&call));

		// A whitelisted call still needs a referendum, of the urgent track, shorter than root.
		let dispatch = RuntimeCall::Whitelist(pallet_whitelist::Call::dispatch_whitelisted_call_with_preimage {
			call: Box::new(call),
		});
		let index = submit(&Keyring::Alice, origins::Origin::WhitelistedCaller.into(), dispatch);
		assert_ok!(ConvictionVoting::vote(signed(&Keyring::Bob), index, aye(500_000 * UNIT)));
		run_to(System::block_number() + 3 * tracks::TEN_MINUTES + 2 * tracks::TEN_MINUTES + 10);
		assert!(approved(index));
		assert_eq!(written(), Some(b"urgent".to_vec()));
	});
}

#[test]
fn the_committee_dispatches_nothing_itself_and_never_changes() {
	dev().execute_with(|| {
		// Two members cannot dispatch a call as root, only whitelist it.
		let call = root_call(b"committee");
		let length = call.encoded_size() as u32;
		let hash = <Runtime as frame_system::Config>::Hashing::hash_of(&call);
		let index = pallet_collective::ProposalCount::<Runtime, TechnicalCollective>::get();
		assert_ok!(TechnicalCommittee::propose(signed(&Keyring::Alice), 2, Box::new(call), length));
		assert_ok!(TechnicalCommittee::vote(signed(&Keyring::Alice), hash, index, true));
		assert_ok!(TechnicalCommittee::vote(signed(&Keyring::Bob), hash, index, true));
		assert_ok!(TechnicalCommittee::close(signed(&Keyring::Alice), hash, index, Weight::MAX, length));
		assert_eq!(written(), None);

		// Not even root changes the members.
		let members = pallet_collective::Members::<Runtime, TechnicalCollective>::get();
		assert_eq!(members.len(), 3);
		assert_noop!(
			TechnicalCommittee::set_members(RuntimeOrigin::root(), vec![Keyring::Dave.to_account_id()], None, 3),
			DispatchError::BadOrigin
		);
	});
}

#[test]
fn the_testnet_moves_from_sudo_to_governance() {
	ext_of(genesis_config_presets::TESTNET_RUNTIME_PRESET).execute_with(|| {
		// The state of the testnet before: its genesis hash, a sudo key, no committee.
		frame_system::BlockHash::<Runtime>::insert(
			0,
			sp_core::H256(genesis_config_presets::bytes32(
				"c2f9cb03897c5ad30fff2f93debaa8b4a6e08f37a12270e107c71eb1ad39e5d1",
			)),
		);
		let sudo_key = frame_support::storage::storage_prefix(b"Sudo", b"Key");
		sp_io::storage::set(&sudo_key, &[1u8; 32]);
		pallet_collective::Members::<Runtime, TechnicalCollective>::kill();

		migrations::Migrations::on_runtime_upgrade();

		assert_eq!(sp_io::storage::get(&sudo_key), None);
		let mut committee = genesis_config_presets::testnet_committee().to_vec();
		committee.sort();
		assert_eq!(pallet_collective::Members::<Runtime, TechnicalCollective>::get(), committee);
		// A second upgrade changes nothing.
		migrations::Migrations::on_runtime_upgrade();
		assert_eq!(pallet_collective::Members::<Runtime, TechnicalCollective>::get(), committee);
	});
}
