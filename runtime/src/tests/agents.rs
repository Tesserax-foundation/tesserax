//! AI agents against the full runtime: an agent key spends for its owner within limits, with
//! ML-DSA transactions, and agents are registered and paid.

use super::*;
use pallet_agents::{CallKind, ClassifyCall, Permissions, PolicyOf};
use pallet_revive::AddressMapper;

fn policy(budget: Balance, max_per_call: Balance, targets: Vec<AccountId>) -> PolicyOf<Runtime> {
	PolicyOf::<Runtime> {
		budget,
		period: DAYS,
		max_per_call,
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: true },
		targets: targets.try_into().unwrap(),
	}
}

fn execute_call(owner: &Keyring, call: RuntimeCall) -> RuntimeCall {
	RuntimeCall::Agents(pallet_agents::Call::execute { owner: owner.to_account_id(), call: Box::new(call) })
}

#[test]
fn agent_key_spends_within_its_budget() {
	new_test_ext().execute_with(|| {
		let (alice, bob, agent) = (Keyring::Alice, Keyring::Bob, Keyring::Charlie);
		// The owner funds the agent's fees and delegates 10 TESS a day, 6 TESS per call, to Bob.
		let authorize = RuntimeCall::Agents(pallet_agents::Call::authorize {
			agent: agent.to_account_id(),
			policy: policy(10 * UNIT, 6 * UNIT, vec![bob.to_account_id()]),
		});
		assert_eq!(apply(signed(authorize, alice.to_account_id(), &alice.pair(), 0)), Ok(Ok(())));
		assert_eq!(
			apply(signed(transfer_call(&agent, UNIT), alice.to_account_id(), &alice.pair(), 1)),
			Ok(Ok(()))
		);

		// The agent signs with its own ML-DSA key and pays for the owner.
		let pay_bob = |amount| execute_call(&alice, transfer_call(&bob, amount));
		assert_eq!(apply(signed(pay_bob(5 * UNIT), agent.to_account_id(), &agent.pair(), 0)), Ok(Ok(())));
		assert_eq!(Balances::balance(&bob.to_account_id()), 1_005 * UNIT);
		assert_eq!(pallet_agents::Pallet::<Runtime>::remaining_budget(&alice.to_account_id(), &agent.to_account_id()), 5 * UNIT);

		// Beyond the budget, or to someone else: refused, and the owner keeps its funds.
		let over = apply(signed(pay_bob(6 * UNIT), agent.to_account_id(), &agent.pair(), 1));
		assert!(matches!(over, Ok(Err(_))));
		let elsewhere = execute_call(&alice, transfer_call(&Keyring::Dave, UNIT));
		assert!(matches!(apply(signed(elsewhere, agent.to_account_id(), &agent.pair(), 2)), Ok(Err(_))));
		assert_eq!(Balances::balance(&bob.to_account_id()), 1_005 * UNIT);

		// The agent cannot raise its own limits.
		let escalate = execute_call(
			&alice,
			RuntimeCall::Agents(pallet_agents::Call::authorize {
				agent: agent.to_account_id(),
				policy: policy(Balance::MAX, Balance::MAX, vec![]),
			}),
		);
		assert!(matches!(apply(signed(escalate, agent.to_account_id(), &agent.pair(), 3)), Ok(Err(_))));

		// Revocation is immediate.
		let revoke = RuntimeCall::Agents(pallet_agents::Call::revoke { agent: agent.to_account_id() });
		assert_eq!(apply(signed(revoke, alice.to_account_id(), &alice.pair(), 2)), Ok(Ok(())));
		assert!(matches!(
			apply(signed(pay_bob(UNIT), agent.to_account_id(), &agent.pair(), 4)),
			Ok(Err(_))
		));
	});
}

#[test]
fn agents_may_only_pay_and_call_contracts() {
	new_test_ext().execute_with(|| {
		let bob = Keyring::Bob.to_account_id();
		let transfer = RuntimeCall::Balances(BalancesCall::transfer_keep_alive { dest: bob.clone().into(), value: 1 });
		assert_eq!(configs::AgentCalls::classify(&transfer), CallKind::Transfer { to: bob.clone() });

		let pay = RuntimeCall::AgentRegistry(pallet_agent_registry::Call::pay { agent: bob.clone(), amount: 1 });
		assert_eq!(configs::AgentCalls::classify(&pay), CallKind::Transfer { to: bob.clone() });

		let contract = sp_core::H160::repeat_byte(7);
		let call = RuntimeCall::Revive(pallet_revive::Call::call {
			dest: contract,
			value: 0,
			weight_limit: Default::default(),
			storage_deposit_limit: 0,
			data: vec![],
		});
		assert_eq!(
			configs::AgentCalls::classify(&call),
			CallKind::Contract { to: <Runtime as pallet_revive::Config>::AddressMapper::to_account_id(&contract) }
		);

		for forbidden in [
			RuntimeCall::Sudo(pallet_sudo::Call::sudo { call: Box::new(transfer.clone()) }),
			RuntimeCall::Balances(BalancesCall::transfer_all { dest: bob.clone().into(), keep_alive: false }),
			RuntimeCall::Agents(pallet_agents::Call::revoke { agent: bob.clone() }),
			RuntimeCall::Vesting(pallet_vesting::Call::vest {}),
			RuntimeCall::AgentRegistry(pallet_agent_registry::Call::deregister { agent: bob }),
		] {
			assert_eq!(configs::AgentCalls::classify(&forbidden), CallKind::Forbidden);
		}
	});
}

#[test]
fn agents_register_and_are_paid() {
	new_test_ext().execute_with(|| {
		let (agent, customer) = (Keyring::Alice, Keyring::Bob);
		let profile = pallet_agent_registry::Profile {
			name: b"translator".to_vec().try_into().unwrap(),
			description: b"French to English".to_vec().try_into().unwrap(),
			endpoint: b"https://translator.example/mcp".to_vec().try_into().unwrap(),
			capabilities: vec![b"translation".to_vec().try_into().unwrap()].try_into().unwrap(),
			metadata: None,
		};
		let register = RuntimeCall::AgentRegistry(pallet_agent_registry::Call::register { profile, owner: None });
		assert_eq!(apply(signed(register, agent.to_account_id(), &agent.pair(), 0)), Ok(Ok(())));

		let pay = RuntimeCall::AgentRegistry(pallet_agent_registry::Call::pay {
			agent: agent.to_account_id(),
			amount: 2 * UNIT,
		});
		assert_eq!(apply(signed(pay, customer.to_account_id(), &customer.pair(), 0)), Ok(Ok(())));
		let rate = RuntimeCall::AgentRegistry(pallet_agent_registry::Call::rate {
			agent: agent.to_account_id(),
			positive: true,
		});
		assert_eq!(apply(signed(rate, customer.to_account_id(), &customer.pair(), 1)), Ok(Ok(())));

		let r = pallet_agent_registry::Reputations::<Runtime>::get(agent.to_account_id());
		assert_eq!((r.payments, r.volume, r.payers, r.positive), (1, 2 * UNIT, 1, 1));
	});
}
