//! Micropayments against the full runtime: a channel funded by an owner, vouchers signed off
//! chain by its agent's key, claimed by the payee with ML-DSA transactions.

use super::*;
use pallet_agents::{Permissions, PolicyOf};
use pallet_channels::{voucher_message, VOUCHER_CONTEXT};
use sp_core::Pair as _;

fn voucher(signer: &Keyring, id: u64, amount: Balance) -> tesserax_crypto::Signature {
	let genesis = System::block_hash(0);
	signer.pair().sign_with_context(&voucher_message(&genesis, id, &amount), VOUCHER_CONTEXT)
}

fn channels_call(call: pallet_channels::Call<Runtime>) -> RuntimeCall {
	RuntimeCall::Channels(call)
}

#[test]
fn agent_pays_per_request_through_a_channel() {
	new_test_ext().execute_with(|| {
		let (owner, service, agent) = (Keyring::Alice, Keyring::Bob, Keyring::Charlie);
		let open = channels_call(pallet_channels::Call::open {
			payee: service.to_account_id(),
			signer: agent.pair().public(),
			deposit: 2 * UNIT,
		});
		assert_eq!(apply(signed(open, owner.to_account_id(), &owner.pair(), 0)), Ok(Ok(())));
		let id = 0;

		// A hundred requests of 0.001 TESS: vouchers off chain, no transaction.
		let price = MILLI_UNIT;
		let mut latest = None;
		for request in 1..=100u128 {
			latest = Some((request * price, voucher(&agent, id, request * price)));
		}
		let (amount, signature) = latest.unwrap();

		// The service claims once.
		let before = Balances::balance(&service.to_account_id());
		let claim = channels_call(pallet_channels::Call::claim { id, amount, signature });
		assert_eq!(apply(signed(claim, service.to_account_id(), &service.pair(), 0)), Ok(Ok(())));
		let fee_paid = before + amount - Balances::balance(&service.to_account_id());
		assert!(fee_paid < UNIT / 100, "the claim costs a fee, not the payments");

		// A voucher signed by another key is refused.
		let forged = voucher(&Keyring::Dave, id, amount + price);
		let claim = channels_call(pallet_channels::Call::claim { id, amount: amount + price, signature: forged });
		assert!(matches!(apply(signed(claim, service.to_account_id(), &service.pair(), 1)), Ok(Err(_))));

		// The service closes: the rest returns to the owner.
		let owner_before = Balances::balance(&owner.to_account_id());
		let close = channels_call(pallet_channels::Call::close { id });
		assert_eq!(apply(signed(close, service.to_account_id(), &service.pair(), 2)), Ok(Ok(())));
		assert!(Balances::balance(&owner.to_account_id()) > owner_before + UNIT);
	});
}

#[test]
fn agents_open_channels_within_their_budget() {
	new_test_ext().execute_with(|| {
		let (owner, service, agent) = (Keyring::Alice, Keyring::Bob, Keyring::Charlie);
		let policy = PolicyOf::<Runtime> {
			budget: 3 * UNIT,
			period: DAYS,
			max_per_call: 3 * UNIT,
			expires_at: None,
			permissions: Permissions { transfers: true, contracts: false },
			targets: vec![service.to_account_id()].try_into().unwrap(),
		};
		let authorize = RuntimeCall::Agents(pallet_agents::Call::authorize { agent: agent.to_account_id(), policy });
		assert_eq!(apply(signed(authorize, owner.to_account_id(), &owner.pair(), 0)), Ok(Ok(())));
		assert_eq!(
			apply(signed(transfer_call(&agent, UNIT), owner.to_account_id(), &owner.pair(), 1)),
			Ok(Ok(()))
		);

		let open = |deposit| {
			RuntimeCall::Agents(pallet_agents::Call::execute {
				owner: owner.to_account_id(),
				call: Box::new(channels_call(pallet_channels::Call::open {
					payee: service.to_account_id(),
					signer: agent.pair().public(),
					deposit,
				})),
			})
		};
		// The deposit and the storage deposit count in the budget.
		assert_eq!(apply(signed(open(2 * UNIT), agent.to_account_id(), &agent.pair(), 0)), Ok(Ok(())));
		let channel = pallet_channels::Channels::<Runtime>::get(0).unwrap();
		assert_eq!(channel.payer, owner.to_account_id());
		assert!(matches!(apply(signed(open(2 * UNIT), agent.to_account_id(), &agent.pair(), 1)), Ok(Err(_))));
	});
}
