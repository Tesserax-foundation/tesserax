//! The task market against the full runtime: an agent orders work for its owner, another agent
//! delivers it, the reward is paid from escrow and the worker's reputation records it.

use super::*;
use pallet_agents::{Permissions, PolicyOf};
use pallet_tasks::{Arbiter, Text};

const DELIVERY: BlockNumber = 100;
const REVIEW: BlockNumber = 50;

fn text(s: &str) -> Text {
	s.as_bytes().to_vec().try_into().unwrap()
}

fn tasks_call(call: pallet_tasks::Call<Runtime>) -> RuntimeCall {
	RuntimeCall::Tasks(call)
}

fn at(block: BlockNumber) {
	System::set_block_number(block);
}

#[test]
fn an_agent_orders_work_from_another_agent() {
	new_test_ext().execute_with(|| {
		let (owner, client_agent, worker) = (Keyring::Alice, Keyring::Charlie, Keyring::Bob);

		// The owner lets its agent spend 10 TESS a day on tasks only.
		let policy = PolicyOf::<Runtime> {
			budget: 10 * UNIT,
			period: DAYS,
			max_per_call: 10 * UNIT,
			expires_at: None,
			permissions: Permissions { transfers: true, contracts: false },
			targets: vec![pallet_tasks::Pallet::<Runtime>::account_id()].try_into().unwrap(),
		};
		let authorize =
			RuntimeCall::Agents(pallet_agents::Call::authorize { agent: client_agent.to_account_id(), policy });
		assert_eq!(apply(signed(authorize, owner.to_account_id(), &owner.pair(), 0)), Ok(Ok(())));
		assert_eq!(
			apply(signed(transfer_call(&client_agent, UNIT), owner.to_account_id(), &owner.pair(), 1)),
			Ok(Ok(()))
		);

		// The agent orders a translation for 5 TESS: the reward is held from the owner.
		let create = tasks_call(pallet_tasks::Call::create {
			spec: text("https://example.com/contract-fr.txt"),
			capability: Some(b"translation".to_vec().try_into().unwrap()),
			reward: 5 * UNIT,
			invited: None,
			arbiter: None,
			bond: UNIT / 10,
			delivery_period: DELIVERY,
			review_period: REVIEW,
		});
		let order = RuntimeCall::Agents(pallet_agents::Call::execute {
			owner: owner.to_account_id(),
			call: Box::new(create),
		});
		assert_eq!(apply(signed(order, client_agent.to_account_id(), &client_agent.pair(), 0)), Ok(Ok(())));
		let task = pallet_tasks::Tasks::<Runtime>::get(0).unwrap();
		assert_eq!(task.client, owner.to_account_id());

		// Another agent accepts and delivers.
		let worker_before = Balances::balance(&worker.to_account_id());
		let accept = tasks_call(pallet_tasks::Call::accept { id: 0 });
		assert_eq!(apply(signed(accept, worker.to_account_id(), &worker.pair(), 0)), Ok(Ok(())));
		let deliver = tasks_call(pallet_tasks::Call::deliver { id: 0, result: text("ipfs://contract-en") });
		assert_eq!(apply(signed(deliver, worker.to_account_id(), &worker.pair(), 1)), Ok(Ok(())));

		// Nobody objects during the review: the result is approved automatically.
		at(System::block_number() + REVIEW);
		let finalize = tasks_call(pallet_tasks::Call::finalize { id: 0 });
		assert_eq!(apply(signed(finalize, worker.to_account_id(), &worker.pair(), 2)), Ok(Ok(())));
		assert!(Balances::balance(&worker.to_account_id()) > worker_before + 4 * UNIT);
		assert!(pallet_tasks::Tasks::<Runtime>::get(0).is_none());
		assert_eq!(pallet_agent_registry::Reputations::<Runtime>::get(worker.to_account_id()).tasks_completed, 1);
	});
}

#[test]
fn the_agent_budget_bounds_rewards() {
	new_test_ext().execute_with(|| {
		let (owner, agent) = (Keyring::Alice, Keyring::Charlie);
		let policy = PolicyOf::<Runtime> {
			budget: 3 * UNIT,
			period: DAYS,
			max_per_call: 3 * UNIT,
			expires_at: None,
			permissions: Permissions { transfers: true, contracts: false },
			targets: Default::default(),
		};
		let authorize = RuntimeCall::Agents(pallet_agents::Call::authorize { agent: agent.to_account_id(), policy });
		assert_eq!(apply(signed(authorize, owner.to_account_id(), &owner.pair(), 0)), Ok(Ok(())));
		assert_eq!(apply(signed(transfer_call(&agent, UNIT), owner.to_account_id(), &owner.pair(), 1)), Ok(Ok(())));

		let order = |reward| {
			RuntimeCall::Agents(pallet_agents::Call::execute {
				owner: owner.to_account_id(),
				call: Box::new(tasks_call(pallet_tasks::Call::create {
					spec: text("spec"),
					capability: None,
					reward,
					invited: None,
					arbiter: Some(Arbiter { account: Keyring::Dave.to_account_id(), fee: UNIT / 10 }),
					bond: 0,
					delivery_period: DELIVERY,
					review_period: REVIEW,
				})),
			})
		};
		// The reward, the arbiter fee and the storage deposit count in the budget.
		assert!(matches!(apply(signed(order(3 * UNIT), agent.to_account_id(), &agent.pair(), 0)), Ok(Err(_))));
		assert_eq!(apply(signed(order(2 * UNIT), agent.to_account_id(), &agent.pair(), 1)), Ok(Ok(())));
	});
}
