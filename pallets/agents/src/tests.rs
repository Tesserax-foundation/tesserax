use crate::{mock::*, Delegations, Error, Event, Permissions, PolicyOf};
use frame_support::{assert_noop, assert_ok, traits::fungible::InspectHold};

fn policy(budget: u64, max_per_call: u64, targets: Vec<u64>) -> PolicyOf<Test> {
	PolicyOf::<Test> {
		budget,
		period: 10,
		max_per_call,
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: true },
		targets: targets.try_into().unwrap(),
	}
}

fn transfer(to: u64, value: u64) -> Box<RuntimeCall> {
	Box::new(RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest: to, value }))
}

fn execute(call: Box<RuntimeCall>) -> frame_support::dispatch::DispatchResultWithPostInfo {
	Agents::execute(RuntimeOrigin::signed(AGENT), OWNER, call)
}

fn held() -> u64 {
	Balances::balance_on_hold(&crate::HoldReason::Delegation.into(), &OWNER)
}

#[test]
fn delegations_hold_a_deposit() {
	new_test_ext().execute_with(|| {
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, policy(100, 50, vec![SHOP])));
		assert_eq!(held(), DEPOSIT_BASE + DEPOSIT_PER_TARGET);
		System::assert_last_event(Event::AgentAuthorized { owner: OWNER, agent: AGENT }.into());

		assert_ok!(Agents::authorize(
			RuntimeOrigin::signed(OWNER),
			AGENT,
			policy(100, 50, vec![SHOP, OTHER, 7])
		));
		assert_eq!(held(), DEPOSIT_BASE + 3 * DEPOSIT_PER_TARGET);
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, policy(100, 50, vec![])));
		assert_eq!(held(), DEPOSIT_BASE);

		assert_ok!(Agents::revoke(RuntimeOrigin::signed(OWNER), AGENT));
		assert_eq!(held(), 0);
		assert_eq!(Balances::free_balance(OWNER), 10_000);
	});
}

#[test]
fn agent_spends_within_its_limits() {
	new_test_ext().execute_with(|| {
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, policy(100, 60, vec![])));
		assert_ok!(execute(transfer(SHOP, 40)));
		assert_eq!(Balances::free_balance(SHOP), 140);
		System::assert_last_event(Event::AgentExecuted { owner: OWNER, agent: AGENT, spent: 40 }.into());
		assert_eq!(Agents::remaining_budget(&OWNER, &AGENT), 60);

		// More than the limit per call.
		assert_noop!(execute(transfer(SHOP, 61)), Error::<Test>::CallLimitExceeded);
		// More than what is left of the budget.
		assert_ok!(execute(transfer(SHOP, 60)));
		assert_noop!(execute(transfer(SHOP, 1)), Error::<Test>::BudgetExceeded);
		assert_eq!(Balances::free_balance(SHOP), 200);

		// The next period starts with a full budget.
		System::set_block_number(11);
		assert_eq!(Agents::remaining_budget(&OWNER, &AGENT), 100);
		assert_ok!(execute(transfer(SHOP, 60)));
		assert_eq!(Delegations::<Test>::get(OWNER, AGENT).unwrap().period_start, 11);
	});
}

#[test]
fn every_loss_of_the_owner_is_counted() {
	new_test_ext().execute_with(|| {
		// A "contract call" that drains the account is caught by the budget, whatever its form.
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, policy(1_000, 1_000, vec![])));
		let drain = Box::new(RuntimeCall::Balances(pallet_balances::Call::transfer_all {
			dest: SHOP,
			keep_alive: true,
		}));
		assert_noop!(execute(drain), Error::<Test>::CallLimitExceeded);
		assert_eq!(Balances::free_balance(SHOP), 100);
	});
}

#[test]
fn targets_and_permissions_are_enforced() {
	new_test_ext().execute_with(|| {
		let mut p = policy(100, 100, vec![SHOP]);
		p.permissions.contracts = false;
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, p));

		assert_noop!(execute(transfer(OTHER, 10)), Error::<Test>::TargetNotAllowed);
		let contract = Box::new(RuntimeCall::System(frame_system::Call::remark { remark: vec![SHOP as u8] }));
		assert_noop!(execute(contract), Error::<Test>::CallNotAllowed);

		// An agent can never change delegations, even through `execute`.
		let escalate = Box::new(RuntimeCall::Agents(crate::Call::authorize {
			agent: AGENT,
			policy: policy(u64::MAX, u64::MAX, vec![]),
		}));
		assert_noop!(execute(escalate), Error::<Test>::CallNotAllowed);
		let revoke_other =
			Box::new(RuntimeCall::Balances(pallet_balances::Call::force_set_balance { who: OWNER, new_free: 0 }));
		assert_noop!(execute(revoke_other), Error::<Test>::CallNotAllowed);

		assert_ok!(execute(transfer(SHOP, 10)));
	});
}

#[test]
fn contract_calls_to_listed_contracts() {
	new_test_ext().execute_with(|| {
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, policy(100, 100, vec![SHOP])));
		let call = |to: u64| Box::new(RuntimeCall::System(frame_system::Call::remark { remark: vec![to as u8] }));
		assert_ok!(execute(call(SHOP)));
		assert_noop!(execute(call(OTHER)), Error::<Test>::TargetNotAllowed);
	});
}

#[test]
fn delegations_expire_and_can_be_revoked() {
	new_test_ext().execute_with(|| {
		let mut p = policy(100, 100, vec![]);
		p.expires_at = Some(5);
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, p.clone()));
		assert_ok!(execute(transfer(SHOP, 10)));
		System::set_block_number(5);
		assert_noop!(execute(transfer(SHOP, 10)), Error::<Test>::Expired);
		assert_eq!(Agents::remaining_budget(&OWNER, &AGENT), 0);

		// Revocation is immediate.
		p.expires_at = None;
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, p));
		assert_ok!(Agents::revoke(RuntimeOrigin::signed(OWNER), AGENT));
		assert_noop!(execute(transfer(SHOP, 10)), Error::<Test>::NotDelegated);
		System::assert_last_event(Event::AgentRevoked { owner: OWNER, agent: AGENT }.into());
	});
}

#[test]
fn agents_can_renounce() {
	new_test_ext().execute_with(|| {
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, policy(100, 100, vec![])));
		assert_noop!(Agents::renounce(RuntimeOrigin::signed(OTHER), OWNER), Error::<Test>::NotDelegated);
		assert_ok!(Agents::renounce(RuntimeOrigin::signed(AGENT), OWNER));
		assert!(Delegations::<Test>::get(OWNER, AGENT).is_none());
		assert_eq!(held(), 0);
	});
}

#[test]
fn invalid_delegations_are_rejected() {
	new_test_ext().execute_with(|| {
		assert_noop!(
			Agents::authorize(RuntimeOrigin::signed(OWNER), OWNER, policy(1, 1, vec![])),
			Error::<Test>::SelfDelegation
		);
		let mut p = policy(1, 1, vec![]);
		p.period = 0;
		assert_noop!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, p), Error::<Test>::InvalidPeriod);
		let mut p = policy(1, 1, vec![]);
		p.expires_at = Some(1);
		assert_noop!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, p), Error::<Test>::InvalidExpiry);
		assert_noop!(execute(transfer(SHOP, 1)), Error::<Test>::NotDelegated);
	});
}

#[test]
fn failed_calls_are_not_recorded() {
	new_test_ext().execute_with(|| {
		assert_ok!(Agents::authorize(RuntimeOrigin::signed(OWNER), AGENT, policy(100_000, 100_000, vec![])));
		// More than the owner has.
		assert!(execute(transfer(SHOP, 50_000)).is_err());
		assert_eq!(Delegations::<Test>::get(OWNER, AGENT).unwrap().spent, 0);
	});
}
