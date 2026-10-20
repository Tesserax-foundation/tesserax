use crate::{mock::*, Arbiter, Error, Event, HoldReason, Status, Tasks as TaskStore};
use frame_support::{assert_noop, assert_ok, traits::fungible::InspectHold};
use sp_runtime::Percent;

const REWARD: u128 = 1_000;
const BOND: u128 = 50;
const DELIVERY: u64 = 10;
const REVIEW: u64 = 30;
const FEE: u128 = 40;

fn text(s: &str) -> crate::Text {
	s.as_bytes().to_vec().try_into().unwrap()
}

fn create(invited: Option<u64>, arbiter: bool) -> u64 {
	let id = crate::NextTaskId::<Test>::get();
	assert_ok!(Tasks::create(
		RuntimeOrigin::signed(CLIENT),
		text("https://example.com/spec.json"),
		Some(b"translation".to_vec().try_into().unwrap()),
		REWARD,
		invited,
		arbiter.then_some(Arbiter { account: ARBITER, fee: FEE }),
		BOND,
		DELIVERY,
		REVIEW,
	));
	id
}

fn held(who: u64) -> u128 {
	Balances::balance_on_hold(&HoldReason::Task.into(), &who)
}

fn deliver(id: u64) {
	assert_ok!(Tasks::accept(RuntimeOrigin::signed(WORKER), id));
	assert_ok!(Tasks::deliver(RuntimeOrigin::signed(WORKER), id, text("ipfs://result")));
}

fn recorded() -> Vec<(u64, bool)> {
	RECORDED.with(|r| r.borrow().clone())
}

#[test]
fn creating_holds_the_reward() {
	new_test_ext().execute_with(|| {
		let id = create(None, true);
		assert_eq!(held(CLIENT), REWARD + FEE + STORAGE_DEPOSIT);
		System::assert_last_event(
			Event::Created { id, client: CLIENT, reward: REWARD, capability: Some(b"translation".to_vec().try_into().unwrap()) }
				.into(),
		);
		let create = |reward, delivery| {
			Tasks::create(RuntimeOrigin::signed(CLIENT), text("spec"), None, reward, None, None, 0, delivery, REVIEW)
		};
		assert_noop!(create(MIN_REWARD - 1, DELIVERY), Error::<Test>::RewardTooLow);
		assert_noop!(create(REWARD, 4), Error::<Test>::InvalidPeriod);
		assert_noop!(create(REWARD, 1_001), Error::<Test>::InvalidPeriod);
	});
}

#[test]
fn approved_work_is_paid() {
	new_test_ext().execute_with(|| {
		let id = create(None, true);
		assert_ok!(Tasks::accept(RuntimeOrigin::signed(WORKER), id));
		assert_eq!(held(WORKER), BOND);
		assert_eq!(TaskStore::<Test>::get(id).unwrap().status, Status::Assigned { worker: WORKER, deadline: 1 + DELIVERY });
		assert_ok!(Tasks::deliver(RuntimeOrigin::signed(WORKER), id, text("ipfs://result")));
		assert_noop!(Tasks::approve(RuntimeOrigin::signed(WORKER), id), Error::<Test>::NotClient);

		assert_ok!(Tasks::approve(RuntimeOrigin::signed(CLIENT), id));
		System::assert_last_event(Event::Completed { id, worker: WORKER, paid: REWARD, refunded: 0 }.into());
		assert_eq!(Balances::free_balance(WORKER), 1_000 + REWARD);
		assert_eq!(Balances::free_balance(CLIENT), 10_000 - REWARD);
		assert_eq!((held(CLIENT), held(WORKER)), (0, 0));
		assert!(TaskStore::<Test>::get(id).is_none());
		assert_eq!(recorded(), vec![(WORKER, true)]);
	});
}

#[test]
fn results_are_approved_automatically() {
	new_test_ext().execute_with(|| {
		let id = create(None, false);
		deliver(id);
		assert_noop!(Tasks::finalize(RuntimeOrigin::signed(OTHER), id), Error::<Test>::NotFinalizable);
		// Without arbiter, a client cannot withhold the reward.
		assert_noop!(Tasks::dispute(RuntimeOrigin::signed(CLIENT), id), Error::<Test>::NoArbiter);

		System::set_block_number(1 + REVIEW);
		assert_ok!(Tasks::finalize(RuntimeOrigin::signed(OTHER), id));
		assert_eq!(Balances::free_balance(WORKER), 1_000 + REWARD);
		assert_eq!(held(CLIENT), 0);
		assert_eq!(recorded(), vec![(WORKER, true)]);
	});
}

#[test]
fn undelivered_work_returns_the_reward_and_the_bond() {
	new_test_ext().execute_with(|| {
		let id = create(None, false);
		assert_ok!(Tasks::accept(RuntimeOrigin::signed(WORKER), id));
		System::set_block_number(1 + DELIVERY);
		assert_noop!(Tasks::finalize(RuntimeOrigin::signed(OTHER), id), Error::<Test>::NotFinalizable);
		System::set_block_number(2 + DELIVERY);
		assert_noop!(
			Tasks::deliver(RuntimeOrigin::signed(WORKER), id, text("late")),
			Error::<Test>::DeadlinePassed
		);
		assert_ok!(Tasks::finalize(RuntimeOrigin::signed(OTHER), id));
		System::assert_last_event(Event::Expired { id, worker: WORKER }.into());
		assert_eq!(Balances::free_balance(CLIENT), 10_000 + BOND);
		assert_eq!(Balances::free_balance(WORKER), 1_000 - BOND);
		assert_eq!((held(CLIENT), held(WORKER)), (0, 0));
		assert_eq!(recorded(), vec![(WORKER, false)]);
	});
}

#[test]
fn arbiters_decide_disputes() {
	new_test_ext().execute_with(|| {
		let id = create(None, true);
		deliver(id);
		assert_ok!(Tasks::dispute(RuntimeOrigin::signed(CLIENT), id));
		System::assert_last_event(Event::Disputed { id, until: 1 + ARBITRATION_PERIOD }.into());
		assert_noop!(Tasks::approve(RuntimeOrigin::signed(CLIENT), id), Error::<Test>::WrongStatus);
		assert_noop!(
			Tasks::resolve(RuntimeOrigin::signed(CLIENT), id, Percent::from_percent(0)),
			Error::<Test>::NotArbiter
		);

		// The work was partly done: 30% to the worker.
		assert_ok!(Tasks::resolve(RuntimeOrigin::signed(ARBITER), id, Percent::from_percent(30)));
		System::assert_last_event(Event::Completed { id, worker: WORKER, paid: 300, refunded: 700 }.into());
		assert_eq!(Balances::free_balance(WORKER), 1_000 + 300);
		assert_eq!(Balances::free_balance(ARBITER), 10 + FEE);
		assert_eq!(Balances::free_balance(CLIENT), 10_000 - 300 - FEE);
		assert_eq!((held(CLIENT), held(WORKER)), (0, 0));
		assert_eq!(recorded(), vec![(WORKER, false)]);
	});
}

#[test]
fn workers_paid_nothing_lose_their_bond() {
	new_test_ext().execute_with(|| {
		let id = create(None, true);
		deliver(id);
		assert_ok!(Tasks::dispute(RuntimeOrigin::signed(CLIENT), id));
		assert_ok!(Tasks::resolve(RuntimeOrigin::signed(ARBITER), id, Percent::zero()));
		assert_eq!(Balances::free_balance(WORKER), 1_000 - BOND);
		assert_eq!(Balances::free_balance(CLIENT), 10_000 - FEE + BOND);
	});
}

#[test]
fn absent_arbiters_do_not_block_payment() {
	new_test_ext().execute_with(|| {
		let id = create(None, true);
		deliver(id);
		assert_ok!(Tasks::dispute(RuntimeOrigin::signed(CLIENT), id));
		System::set_block_number(1 + ARBITRATION_PERIOD);
		assert_ok!(Tasks::finalize(RuntimeOrigin::signed(WORKER), id));
		// The worker is paid and the unpaid arbiter fee returns to the client.
		assert_eq!(Balances::free_balance(WORKER), 1_000 + REWARD);
		assert_eq!(Balances::free_balance(CLIENT), 10_000 - REWARD);
		assert_eq!(Balances::free_balance(ARBITER), 10);
	});
}

#[test]
fn disputes_happen_during_the_review_period() {
	new_test_ext().execute_with(|| {
		let id = create(None, true);
		deliver(id);
		System::set_block_number(1 + REVIEW);
		assert_noop!(Tasks::dispute(RuntimeOrigin::signed(CLIENT), id), Error::<Test>::ReviewOver);
	});
}

#[test]
fn invited_workers_and_conflicts_of_interest() {
	new_test_ext().execute_with(|| {
		let id = create(Some(WORKER), true);
		assert_noop!(Tasks::accept(RuntimeOrigin::signed(OTHER), id), Error::<Test>::NotInvited);
		assert_noop!(Tasks::accept(RuntimeOrigin::signed(CLIENT), id), Error::<Test>::ConflictOfInterest);
		assert_noop!(Tasks::accept(RuntimeOrigin::signed(ARBITER), id), Error::<Test>::ConflictOfInterest);
		assert_ok!(Tasks::accept(RuntimeOrigin::signed(WORKER), id));
		assert_noop!(Tasks::accept(RuntimeOrigin::signed(WORKER), id), Error::<Test>::WrongStatus);
		assert_noop!(
			Tasks::deliver(RuntimeOrigin::signed(OTHER), id, text("x")),
			Error::<Test>::NotWorker
		);

		let invalid = |invited, arbiter| {
			Tasks::create(RuntimeOrigin::signed(CLIENT), text("spec"), None, REWARD, invited, arbiter, 0, DELIVERY, REVIEW)
		};
		assert_noop!(invalid(Some(CLIENT), None), Error::<Test>::ConflictOfInterest);
		assert_noop!(invalid(None, Some(Arbiter { account: CLIENT, fee: 0 })), Error::<Test>::ConflictOfInterest);
		assert_noop!(
			invalid(Some(ARBITER), Some(Arbiter { account: ARBITER, fee: 0 })),
			Error::<Test>::ConflictOfInterest
		);
	});
}

#[test]
fn open_tasks_can_be_cancelled() {
	new_test_ext().execute_with(|| {
		let id = create(None, true);
		assert_noop!(Tasks::cancel(RuntimeOrigin::signed(OTHER), id), Error::<Test>::NotClient);
		assert_ok!(Tasks::cancel(RuntimeOrigin::signed(CLIENT), id));
		assert_eq!(held(CLIENT), 0);
		assert_eq!(Balances::free_balance(CLIENT), 10_000);

		let id = create(None, true);
		assert_ok!(Tasks::accept(RuntimeOrigin::signed(WORKER), id));
		assert_noop!(Tasks::cancel(RuntimeOrigin::signed(CLIENT), id), Error::<Test>::WrongStatus);
	});
}
