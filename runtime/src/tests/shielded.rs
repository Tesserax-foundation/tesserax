//! Private transfers against the full runtime: shielding with a signed transaction, then a
//! private transfer submitted without signer.

use super::*;
use crate::configs::ShieldedMinimumFee;
use pallet_shielded::{PoolBalance, ShieldedTransaction};
use tesserax_shielded::{
	encryption::encrypt,
	keys::{Address, SpendingKey},
	note::{nullifier, output_rho, random_randomness, Note},
	proof::{prove, InputWitness, OutputWitness, Witness},
	tree::{MerklePath, Tree, TREE_DEPTH},
	Digest,
};

fn dummy_input() -> InputWitness {
	let key = SpendingKey::generate();
	InputWitness {
		nk: key.nullifier_key(),
		note: Note { value: 0, tag: key.tag(), rho: Digest::ZERO, r: random_randomness() },
		path: MerklePath { position: 0, siblings: vec![Digest::ZERO; TREE_DEPTH] },
	}
}

/// Build and prove a transaction. Returns it with its output notes.
fn build(
	tree: &Tree,
	inputs: [InputWitness; 2],
	outputs: [(Address, u64); 2],
	(value_in, value_out, fee): (u64, u64, u64),
	recipient: Option<AccountId>,
	source: Option<AccountId>,
) -> (ShieldedTransaction<AccountId>, [Note; 2]) {
	let nullifiers: [Digest; 2] =
		core::array::from_fn(|i| nullifier(&inputs[i].nk, &inputs[i].note.commitment()));
	let notes: [Note; 2] = core::array::from_fn(|j| Note {
		value: outputs[j].1,
		tag: outputs[j].0.tag,
		rho: output_rho(&nullifiers, j as u64),
		r: random_randomness(),
	});
	let mut transaction = ShieldedTransaction {
		anchor: tree.root(),
		nullifiers,
		commitments: notes.map(|n| n.commitment()),
		value_in,
		value_out,
		fee,
		recipient,
		notes: core::array::from_fn(|j| {
			encrypt(&notes[j], &outputs[j].0).unwrap().try_into().unwrap()
		}),
		proof: Default::default(),
	};
	let witness = Witness {
		inputs,
		outputs: notes.map(|note| OutputWitness { note }),
		anchor: transaction.anchor,
		value_in,
		value_out,
		fee,
		binding: transaction.binding(source.as_ref()),
	};
	transaction.proof = prove(&witness).unwrap().0.try_into().unwrap();
	(transaction, notes)
}

fn transact(transaction: ShieldedTransaction<AccountId>) -> UncheckedExtrinsic {
	let call = RuntimeCall::Shielded(pallet_shielded::Call::transact {
		transaction: Box::new(transaction),
	});
	UncheckedExtrinsic::new_transaction(call, tx_extension(0))
}

fn next_block() {
	System::set_block_number(System::block_number() + 1);
}

#[test]
fn private_transfer_without_signer() {
	new_test_ext().execute_with(|| {
		let alice = Keyring::Alice;
		let (key, other) = (SpendingKey::from_seed([1; 32]), SpendingKey::from_seed([2; 32]));
		let mut tree = Tree::default();

		// Alice shields 10 TESS with a signed transaction.
		let amount = (10 * UNIT) as u64;
		let (shield, notes) = build(
			&tree,
			[dummy_input(), dummy_input()],
			[(key.address(), amount), (key.address(), 0)],
			(amount, 0, 0),
			None,
			Some(alice.to_account_id()),
		);
		let call = RuntimeCall::Shielded(pallet_shielded::Call::shield { transaction: Box::new(shield) });
		let before = Balances::balance(&alice.to_account_id());
		assert_eq!(apply(signed(call, alice.to_account_id(), &alice.pair(), 0)), Ok(Ok(())));
		assert!(Balances::balance(&alice.to_account_id()) < before - 10 * UNIT + UNIT / 100);
		assert_eq!(PoolBalance::<Runtime>::get(), 10 * UNIT);
		let position = tree.append(notes[0].commitment());
		tree.append(notes[1].commitment());
		next_block();

		// The note pays Bob 4 TESS, without any signer.
		let fee = ShieldedMinimumFee::get();
		let input = InputWitness { nk: key.nullifier_key(), note: notes[0], path: tree.path(position).unwrap() };
		let (transfer, _) = build(
			&tree,
			[input, dummy_input()],
			[(other.address(), amount - 4 * UNIT as u64 - fee), (other.address(), 0)],
			(0, 4 * UNIT as u64, fee),
			Some(Keyring::Bob.to_account_id()),
			None,
		);

		let mut redirected = transfer.clone();
		redirected.recipient = Some(Keyring::Charlie.to_account_id());
		let validity =
			Executive::validate_transaction(TransactionSource::External, transact(redirected), System::parent_hash());
		assert_eq!(validity, Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof)));

		let validity = Executive::validate_transaction(
			TransactionSource::External,
			transact(transfer.clone()),
			System::parent_hash(),
		)
		.unwrap();
		assert_eq!(validity.priority, fee);
		assert_eq!(validity.provides.len(), 2);

		// Unsigned extrinsics without authorization are not accepted.
		let call = RuntimeCall::Shielded(pallet_shielded::Call::transact { transaction: Box::new(transfer.clone()) });
		assert!(Executive::validate_transaction(
			TransactionSource::External,
			UncheckedExtrinsic::new_bare(call),
			System::parent_hash(),
		)
		.is_err());

		assert_eq!(apply(transact(transfer.clone())), Ok(Ok(())));
		assert_eq!(Balances::balance(&Keyring::Bob.to_account_id()), 1_004 * UNIT);
		assert_eq!(PoolBalance::<Runtime>::get(), 6 * UNIT - fee as Balance);

		// The same notes cannot be spent twice.
		assert_eq!(
			apply(transact(transfer)),
			Err(TransactionValidityError::Invalid(InvalidTransaction::Stale))
		);
	});
}

#[test]
fn minimum_fee_covers_weight_and_length() {
	let fee = ShieldedMinimumFee::get() as Balance;
	// About 0.016 TESS with the current weights: verifying a proof takes about 13 ms.
	assert!(fee > UNIT / 100 && fee < UNIT / 10);
}
