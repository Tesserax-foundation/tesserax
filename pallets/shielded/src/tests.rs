use crate::{mock::*, Error, Event, Nullifiers, PoolBalance, Roots, ShieldedTransaction};
use frame_support::{
	assert_noop, assert_ok,
	pallet_prelude::{InvalidTransaction, TransactionSource, TransactionValidityError},
	traits::Authorize,
};
use frame_system::RawOrigin;
use tessera_shielded::{
	encryption::{decrypt, encrypt},
	keys::{Address, SpendingKey},
	note::{nullifier, output_rho, random_randomness, Note},
	proof::{prove, InputWitness, OutputWitness, Witness},
	tree::{MerklePath, Tree, TREE_DEPTH},
	Digest, EncryptedNote,
};

fn key(seed: u8) -> SpendingKey {
	SpendingKey::from_seed([seed; 32])
}

/// A note owned by `key` at `position` in the tree.
#[derive(Clone)]
struct Owned {
	key: SpendingKey,
	note: Note,
	position: u64,
}

/// Wallet-side copy of the note tree, built from the events.
#[derive(Default)]
struct Chain {
	tree: Tree,
}

impl Chain {
	fn input(&self, owned: &Owned) -> InputWitness {
		InputWitness {
			nk: owned.key.nullifier_key(),
			note: owned.note,
			path: self.tree.path(owned.position).unwrap(),
		}
	}

	/// Append the notes of the last `Transacted` event and decrypt those owned by `keys`.
	fn scan(&mut self, keys: &[SpendingKey]) -> Vec<Owned> {
		let Some(Event::Transacted { commitments, position, notes, .. }) = last_event() else {
			panic!("no transaction applied");
		};
		let mut owned = Vec::new();
		for (j, (commitment, encrypted)) in commitments.iter().zip(notes.iter()).enumerate() {
			assert_eq!(self.tree.append(*commitment), position + j as u64);
			for key in keys {
				if let Some(note) = decrypt(encrypted, commitment, key) {
					owned.push(Owned { key: key.clone(), note, position: position + j as u64 });
				}
			}
		}
		assert_eq!(Roots::<Test>::get().last(), Some(&self.tree.root()));
		owned
	}
}

fn last_event() -> Option<Event<Test>> {
	System::events().into_iter().rev().find_map(|record| match record.event {
		RuntimeEvent::Shielded(event) => Some(event),
		_ => None,
	})
}

fn dummy_input() -> InputWitness {
	let key = SpendingKey::generate();
	InputWitness {
		nk: key.nullifier_key(),
		note: Note { value: 0, tag: key.tag(), rho: Digest::ZERO, r: random_randomness() },
		path: MerklePath { position: 0, siblings: vec![Digest::ZERO; TREE_DEPTH] },
	}
}

struct Build {
	inputs: [InputWitness; 2],
	outputs: [(Address, u64); 2],
	value_in: u64,
	value_out: u64,
	fee: u64,
	recipient: Option<u64>,
	source: Option<u64>,
}

/// Build and prove a transaction, as a wallet does.
fn build(chain: &Chain, b: Build) -> ShieldedTransaction<u64> {
	let nullifiers: [Digest; 2] =
		core::array::from_fn(|i| nullifier(&b.inputs[i].nk, &b.inputs[i].note.commitment()));
	let notes: [Note; 2] = core::array::from_fn(|j| Note {
		value: b.outputs[j].1,
		tag: b.outputs[j].0.tag,
		rho: output_rho(&nullifiers, j as u64),
		r: random_randomness(),
	});
	let encrypted: [EncryptedNote; 2] = core::array::from_fn(|j| {
		encrypt(&notes[j], &b.outputs[j].0).unwrap().try_into().unwrap()
	});
	let mut transaction = ShieldedTransaction {
		anchor: chain.tree.root(),
		nullifiers,
		commitments: notes.map(|n| n.commitment()),
		value_in: b.value_in,
		value_out: b.value_out,
		fee: b.fee,
		recipient: b.recipient,
		notes: encrypted,
		proof: Default::default(),
	};
	let witness = Witness {
		inputs: b.inputs,
		outputs: notes.map(|note| OutputWitness { note }),
		anchor: transaction.anchor,
		value_in: b.value_in,
		value_out: b.value_out,
		fee: b.fee,
		binding: transaction.binding(b.source.as_ref()),
	};
	let (proof, public) = prove(&witness).unwrap();
	assert_eq!(public, transaction.public_inputs(b.source.as_ref()));
	transaction.proof = proof.try_into().unwrap();
	transaction
}

fn call(transaction: &ShieldedTransaction<u64>) -> RuntimeCall {
	RuntimeCall::Shielded(crate::Call::transact { transaction: Box::new(transaction.clone()) })
}

fn authorize(transaction: &ShieldedTransaction<u64>) -> Result<(), TransactionValidityError> {
	call(transaction).authorize(TransactionSource::External).unwrap().map(|_| ())
}

/// Submit a transaction without signer, as the block author would include it.
fn submit(transaction: &ShieldedTransaction<u64>) {
	authorize(transaction).unwrap();
	assert_ok!(Shielded::transact(RawOrigin::Authorized.into(), Box::new(transaction.clone())));
}

fn next_block() {
	System::set_block_number(System::block_number() + 1);
}

fn shield(chain: &mut Chain, who: u64, outputs: [(Address, u64); 2], keys: &[SpendingKey]) -> Vec<Owned> {
	let value_in = outputs[0].1 + outputs[1].1;
	let transaction = build(
		chain,
		Build {
			inputs: [dummy_input(), dummy_input()],
			outputs,
			value_in,
			value_out: 0,
			fee: 0,
			recipient: None,
			source: Some(who),
		},
	);
	assert_ok!(Shielded::shield(RuntimeOrigin::signed(who), Box::new(transaction)));
	next_block();
	chain.scan(keys)
}

#[test]
fn shield_transfer_unshield() {
	new_test_ext().execute_with(|| {
		let mut chain = Chain::default();
		let (alice, bob) = (key(1), key(2));

		// Account 1 shields 50 000 to Alice.
		let owned = shield(
			&mut chain,
			1,
			[(alice.address(), 40_000), (alice.address(), 10_000)],
			&[alice.clone(), bob.clone()],
		);
		assert_eq!(owned.len(), 2);
		assert!(owned.iter().all(|o| o.key.tag() == alice.tag()));
		assert_eq!(Balances::free_balance(1), 950_000);
		assert_eq!(PoolBalance::<Test>::get(), 50_000);

		// Alice pays 30 000 to Bob privately, keeps 18 000 and pays a fee of 2 000.
		let transaction = build(
			&chain,
			Build {
				inputs: [chain.input(&owned[0]), chain.input(&owned[1])],
				outputs: [(bob.address(), 30_000), (alice.address(), 18_000)],
				value_in: 0,
				value_out: 0,
				fee: 2_000,
				recipient: None,
				source: None,
			},
		);
		submit(&transaction);
		next_block();
		let owned = chain.scan(&[alice.clone(), bob.clone()]);
		assert_eq!(owned.iter().map(|o| (o.key.tag(), o.note.value)).collect::<Vec<_>>(), vec![
			(bob.tag(), 30_000),
			(alice.tag(), 18_000)
		]);
		assert_eq!(Balances::free_balance(AUTHOR), 3_000);
		assert_eq!(PoolBalance::<Test>::get(), 48_000);
		assert!(transaction.nullifiers.iter().all(|nf| Nullifiers::<Test>::contains_key(nf)));

		// Bob unshields 29 000 to a new account 7, paying a fee of 1 000.
		let unshield = build(
			&chain,
			Build {
				inputs: [chain.input(&owned[0]), dummy_input()],
				outputs: [(bob.address(), 0), (key(9).address(), 0)],
				value_in: 0,
				value_out: 29_000,
				fee: 1_000,
				recipient: Some(7),
				source: None,
			},
		);
		submit(&unshield);
		assert_eq!(Balances::free_balance(7), 29_000);
		assert_eq!(Balances::free_balance(AUTHOR), 4_000);
		assert_eq!(PoolBalance::<Test>::get(), 18_000);

		// Spending the same notes again is rejected.
		assert_eq!(authorize(&unshield), Err(InvalidTransaction::Stale.into()));
		assert_noop!(
			Shielded::transact(RawOrigin::Authorized.into(), Box::new(unshield)),
			Error::<Test>::NullifierSpent
		);
	});
}

/// A transfer of a 40 000 note of Alice, shielded by account 1.
fn setup() -> (Chain, Owned) {
	let mut chain = Chain::default();
	let alice = key(1);
	let owned = shield(&mut chain, 1, [(alice.address(), 40_000), (alice.address(), 0)], &[alice]);
	(chain, owned[0].clone())
}

fn transfer(chain: &Chain, owned: &Owned, value_out: u64, fee: u64) -> ShieldedTransaction<u64> {
	build(
		chain,
		Build {
			inputs: [chain.input(owned), dummy_input()],
			outputs: [(key(2).address(), owned.note.value - value_out - fee), (key(3).address(), 0)],
			value_in: 0,
			value_out,
			fee,
			recipient: (value_out > 0).then_some(8),
			source: None,
		},
	)
}

#[test]
fn proofs_bind_every_field() {
	new_test_ext().execute_with(|| {
		let (chain, owned) = setup();
		let transaction = transfer(&chain, &owned, 5_000, 2_000);
		let bad_proof = Err(InvalidTransaction::BadProof.into());

		// A relayer cannot redirect the funds or replace a note.
		let mut t = transaction.clone();
		t.recipient = Some(9);
		assert_eq!(authorize(&t), bad_proof);
		let mut t = transaction.clone();
		t.notes[1][100] ^= 1;
		assert_eq!(authorize(&t), bad_proof);
		let mut t = transaction.clone();
		t.commitments.swap(0, 1);
		assert_eq!(authorize(&t), bad_proof);
		let mut t = transaction.clone();
		t.value_out += 1_000;
		t.fee -= 1_000;
		assert_eq!(authorize(&t), bad_proof);
		let mut t = transaction.clone();
		let mut proof = t.proof.into_inner();
		proof[1000] ^= 1;
		t.proof = proof.try_into().unwrap();
		assert_eq!(authorize(&t), bad_proof);

		assert_ok!(authorize(&transaction));
	});
}

#[test]
fn transactions_without_signer_are_checked() {
	new_test_ext().execute_with(|| {
		let (chain, owned) = setup();
		let transaction = transfer(&chain, &owned, 0, 2_000);
		let custom = |error: Error<Test>| {
			Err(InvalidTransaction::Custom(codec::Encode::encode(&error)[0]).into())
		};

		let mut t = transaction.clone();
		t.nullifiers[1] = t.nullifiers[0];
		assert_eq!(authorize(&t), custom(Error::DuplicateNullifier));
		let mut t = transaction.clone();
		t.anchor = Digest([1, 2, 3, 4]);
		assert_eq!(authorize(&t), custom(Error::UnknownAnchor));
		let mut t = transaction.clone();
		t.value_in = 1;
		assert_eq!(authorize(&t), custom(Error::UnexpectedValueIn));
		let mut t = transaction.clone();
		t.recipient = Some(8);
		assert_eq!(authorize(&t), custom(Error::InvalidRecipient));
		let mut t = transaction.clone();
		t.fee = tessera_shielded::MAX_VALUE;
		assert_eq!(authorize(&t), custom(Error::AmountOutOfRange));

		let cheap = transfer(&chain, &owned, 0, MINIMUM_FEE - 1);
		assert_eq!(authorize(&cheap), Err(InvalidTransaction::Payment.into()));

		// Transactions must be authorized.
		assert_noop!(
			Shielded::transact(RuntimeOrigin::signed(1), Box::new(transaction.clone())),
			sp_runtime::DispatchError::BadOrigin
		);
		assert_noop!(
			Shielded::transact(RuntimeOrigin::none(), Box::new(transaction)),
			sp_runtime::DispatchError::BadOrigin
		);
	});
}

#[test]
fn recipients_must_be_able_to_receive() {
	new_test_ext().execute_with(|| {
		let (chain, owned) = setup();
		// Below the existential deposit of a new account.
		let transaction = transfer(&chain, &owned, 5, 2_000);
		assert_eq!(
			authorize(&transaction),
			Err(InvalidTransaction::Custom(codec::Encode::encode(&Error::<Test>::CannotReceive)[0])
				.into())
		);
	});
}

#[test]
fn the_pool_cannot_go_negative() {
	new_test_ext().execute_with(|| {
		let (chain, owned) = setup();
		let transaction = transfer(&chain, &owned, 30_000, 2_000);
		// Simulate a pool holding less than the notes claim, as a forged proof would need.
		PoolBalance::<Test>::put(31_000);
		assert_noop!(
			Shielded::transact(RawOrigin::Authorized.into(), Box::new(transaction.clone())),
			Error::<Test>::PoolUnderflow
		);
		PoolBalance::<Test>::put(32_000);
		submit(&transaction);
		assert_eq!(PoolBalance::<Test>::get(), 0);
	});
}

#[test]
fn shielding_is_bound_to_the_signer() {
	new_test_ext().execute_with(|| {
		let chain = Chain::default();
		let transaction = build(
			&chain,
			Build {
				inputs: [dummy_input(), dummy_input()],
				outputs: [(key(1).address(), 500), (key(1).address(), 0)],
				value_in: 500,
				value_out: 0,
				fee: 0,
				recipient: None,
				source: Some(1),
			},
		);
		assert_noop!(
			Shielded::shield(RuntimeOrigin::signed(2), Box::new(transaction.clone())),
			Error::<Test>::InvalidProof
		);
		// Without signer, it cannot take value from an account.
		assert_eq!(
			authorize(&transaction),
			Err(InvalidTransaction::Custom(codec::Encode::encode(&Error::<Test>::UnexpectedValueIn)[0])
				.into())
		);
		assert_ok!(Shielded::shield(RuntimeOrigin::signed(1), Box::new(transaction)));
		assert_eq!(Balances::free_balance(1), 999_500);
		assert_eq!(PoolBalance::<Test>::get(), 500);
	});
}

#[test]
fn anchors_expire() {
	new_test_ext().execute_with(|| {
		let (mut chain, owned) = setup();
		let transaction = transfer(&chain, &owned, 0, 2_000);
		// Each block with shielded activity records a root; the oldest ones are forgotten.
		for _ in 0..ROOT_HISTORY {
			shield(&mut chain, 2, [(key(5).address(), 100), (key(5).address(), 0)], &[]);
		}
		assert_eq!(Roots::<Test>::get().len(), ROOT_HISTORY as usize);
		assert_eq!(
			authorize(&transaction),
			Err(InvalidTransaction::Custom(codec::Encode::encode(&Error::<Test>::UnknownAnchor)[0])
				.into())
		);
		// The same note spent against a recent anchor is accepted.
		submit(&transfer(&chain, &owned, 0, 2_000));
	});
}

#[test]
fn roots_are_recorded_once_per_block() {
	new_test_ext().execute_with(|| {
		let mut chain = Chain::default();
		let alice = key(1);
		let before = Roots::<Test>::get().len();
		let owned = shield(&mut chain, 1, [(alice.address(), 4_000), (alice.address(), 4_000)], &[alice]);
		assert_eq!(Roots::<Test>::get().len(), before + 1);
		// Two transactions in the same block, both anchored at the previous block.
		let a = transfer(&chain, &owned[0], 0, 2_000);
		let b = transfer(&chain, &owned[1], 0, 2_000);
		submit(&a);
		submit(&b);
		assert_eq!(Roots::<Test>::get().len(), before + 2);
		chain.tree.append(a.commitments[0]);
		chain.tree.append(a.commitments[1]);
		chain.tree.append(b.commitments[0]);
		chain.tree.append(b.commitments[1]);
		assert_eq!(Roots::<Test>::get().last(), Some(&chain.tree.root()));
	});
}

/// Transaction used by the benchmarks: a private transfer of two notes paying a fee of 1 TESS.
/// Regenerate with `cargo test -p pallet-shielded benchmark_fixture -- --ignored`.
#[test]
#[ignore]
fn benchmark_fixture() {
	let mut chain = Chain::default();
	let owned: Vec<Owned> = [500_000_000_000_000u64, 700_000_000_000_000]
		.iter()
		.enumerate()
		.map(|(i, value)| {
			let key = key(10 + i as u8);
			let note = Note { value: *value, tag: key.tag(), rho: Digest([i as u64; 4]), r: [1, 2, 3] };
			let position = chain.tree.append(note.commitment());
			Owned { key, note, position }
		})
		.collect();
	let transaction = build(
		&chain,
		Build {
			inputs: [chain.input(&owned[0]), chain.input(&owned[1])],
			outputs: [(key(12).address(), 1_000_000_000_000_000), (key(13).address(), 199_000_000_000_000)],
			value_in: 0,
			value_out: 0,
			fee: 1_000_000_000_000,
			recipient: None,
			source: None,
		},
	);
	let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/fixtures/transfer.scale");
	std::fs::write(path, codec::Encode::encode(&transaction)).unwrap();
}
