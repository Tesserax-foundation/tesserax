use super::{
	air::ShieldedAir, layout::selector, prove, prover::unchecked_trace, verify, InputWitness,
	OutputWitness, ProveError, PublicInputs, Witness,
};
use crate::{
	hash::{Digest, Felt},
	keys::{nullifier_key, owner_tag},
	note::{nullifier, output_rho, Note, MAX_VALUE},
	tree::{MerklePath, Tree, TREE_DEPTH},
};
use p3_air::check_constraints;
use p3_field::PrimeCharacteristicRing;
use std::time::Instant;

fn nk(seed: u8) -> Digest {
	nullifier_key(&[seed; 32])
}

fn note(owner: u8, value: u64, salt: u64) -> Note {
	Note {
		value,
		tag: owner_tag(&nk(owner)),
		rho: Digest([salt, salt + 1, salt + 2, salt + 3]),
		r: [salt * 7, salt * 11, salt * 13],
	}
}

fn dummy_input(seed: u8) -> InputWitness {
	InputWitness {
		nk: nk(seed),
		note: note(seed, 0, 1000 + seed as u64),
		path: MerklePath { position: 0, siblings: vec![Digest::ZERO; TREE_DEPTH] },
	}
}

/// Outputs to `owners` with `values`, with the `rho` derived from the inputs.
fn outputs(inputs: &[InputWitness; 2], owners: [u8; 2], values: [u64; 2]) -> [OutputWitness; 2] {
	let nullifiers: [Digest; 2] =
		core::array::from_fn(|i| nullifier(&inputs[i].nk, &inputs[i].note.commitment()));
	core::array::from_fn(|j| OutputWitness {
		note: Note {
			value: values[j],
			tag: owner_tag(&nk(owners[j])),
			rho: output_rho(&nullifiers, j as u64),
			r: [j as u64 + 5, 6, 7],
		},
	})
}

/// A private transfer of two notes of a tree with some other notes.
fn transfer() -> Witness {
	let mut tree = Tree::default();
	for k in 0..5 {
		tree.append(note(9, 10 + k, 500 + k).commitment());
	}
	let a = note(1, 700, 1);
	let b = note(2, 300, 2);
	let pa = tree.append(a.commitment());
	tree.append(note(9, 1, 600).commitment());
	let pb = tree.append(b.commitment());
	let inputs = [
		InputWitness { nk: nk(1), note: a, path: tree.path(pa).unwrap() },
		InputWitness { nk: nk(2), note: b, path: tree.path(pb).unwrap() },
	];
	let outputs = outputs(&inputs, [3, 1], [850, 140]);
	Witness {
		inputs,
		outputs,
		anchor: tree.root(),
		value_in: 0,
		value_out: 0,
		fee: 10,
		binding: Digest([1, 2, 3, 4]),
	}
}

/// Moves a transparent amount into the pool.
fn shield() -> Witness {
	let inputs = [dummy_input(40), dummy_input(41)];
	let outputs = outputs(&inputs, [3, 4], [1_000, 0]);
	Witness {
		inputs,
		outputs,
		anchor: Tree::default().root(),
		value_in: 1_005,
		value_out: 0,
		fee: 5,
		binding: Digest([9, 9, 9, 9]),
	}
}

/// Spends one note to a transparent account.
fn unshield() -> Witness {
	let mut tree = Tree::default();
	let a = note(1, 5_000, 77);
	let position = tree.append(a.commitment());
	let inputs = [
		InputWitness { nk: nk(1), note: a, path: tree.path(position).unwrap() },
		dummy_input(42),
	];
	let outputs = outputs(&inputs, [1, 5], [1_000, 0]);
	Witness {
		inputs,
		outputs,
		anchor: tree.root(),
		value_in: 0,
		value_out: 3_990,
		fee: 10,
		binding: Digest([5, 6, 7, 8]),
	}
}

fn constraints_hold(witness: &Witness) {
	let trace = unchecked_trace(witness);
	check_constraints(&ShieldedAir::default(), &trace, &witness.public_inputs().to_field_elements());
}

#[test]
fn trace_satisfies_constraints() {
	constraints_hold(&transfer());
	constraints_hold(&shield());
	constraints_hold(&unshield());
}

#[test]
fn trace_digests_match_native_hashes() {
	let witness = transfer();
	let trace = unchecked_trace(&witness);
	let public = witness.public_inputs();
	let columns = super::layout::selector_columns();
	let row_output = |row: usize| -> Digest {
		let values = &trace.values[row * super::layout::NUM_COLS..(row + 1) * super::layout::NUM_COLS];
		let row: &super::layout::Row<Felt> = core::borrow::Borrow::borrow(values);
		let post = &row.perm.ending_full_rounds[super::layout::HALF_FULL_ROUNDS - 1].post;
		Digest::from_felts([post[0], post[1], post[2], post[3]])
	};
	let find = |s: usize| (0..super::layout::TRACE_HEIGHT).filter(move |r| columns[s][*r]);
	assert_eq!(find(selector::NF1).map(row_output).collect::<Vec<_>>(), vec![public.nullifiers[0]]);
	assert_eq!(find(selector::NF2).map(row_output).collect::<Vec<_>>(), vec![public.nullifiers[1]]);
	assert_eq!(find(selector::ROOT).map(row_output).collect::<Vec<_>>(), vec![public.anchor; 2]);
	assert_eq!(
		find(selector::OUT_VALUE1).map(row_output).collect::<Vec<_>>(),
		vec![public.commitments[0]]
	);
	assert_eq!(
		find(selector::OUT_VALUE2).map(row_output).collect::<Vec<_>>(),
		vec![public.commitments[1]]
	);
}

#[test]
#[should_panic]
fn constraints_reject_wrong_balance() {
	let mut witness = transfer();
	witness.fee = 11;
	constraints_hold(&witness);
}

#[test]
#[should_panic]
fn constraints_reject_spending_outside_the_tree() {
	let mut witness = transfer();
	witness.anchor = Digest([1, 1, 1, 1]);
	constraints_hold(&witness);
}

#[test]
#[should_panic]
fn constraints_reject_a_foreign_note() {
	let mut witness = transfer();
	// Spending the note of owner 1 with the key of owner 4 changes the owner tag in the
	// commitment, which is no longer in the tree.
	witness.inputs[0].nk = nk(4);
	constraints_hold(&witness);
}

#[test]
#[should_panic]
fn constraints_reject_an_output_value_out_of_range() {
	let mut witness = shield();
	// The field modulus minus a small amount would balance a large value_in if values wrapped.
	witness.outputs[1].note.value = MAX_VALUE;
	witness.value_in += MAX_VALUE;
	constraints_hold(&witness);
}

#[test]
fn prover_checks_the_witness() {
	let mut witness = transfer();
	witness.fee = 11;
	assert_eq!(prove(&witness).unwrap_err(), ProveError::Unbalanced);

	let mut witness = transfer();
	witness.inputs[0].nk = nk(4);
	assert_eq!(prove(&witness).unwrap_err(), ProveError::WrongOwner);

	let mut witness = transfer();
	witness.anchor = Digest::ZERO;
	assert_eq!(prove(&witness).unwrap_err(), ProveError::NotInTree);

	let mut witness = transfer();
	witness.outputs[0].note.rho = Digest::ZERO;
	assert_eq!(prove(&witness).unwrap_err(), ProveError::WrongRho);

	let mut witness = shield();
	witness.value_in = MAX_VALUE;
	assert_eq!(prove(&witness).unwrap_err(), ProveError::ValueOutOfRange);
}

fn tampered(public: &PublicInputs) -> Vec<PublicInputs> {
	let mut cases = Vec::new();
	let mut p = public.clone();
	p.anchor.0[0] ^= 1;
	cases.push(p);
	let mut p = public.clone();
	p.nullifiers[0].0[3] ^= 1;
	cases.push(p);
	let mut p = public.clone();
	p.nullifiers.swap(0, 1);
	cases.push(p);
	let mut p = public.clone();
	p.commitments[1].0[2] ^= 1;
	cases.push(p);
	let mut p = public.clone();
	p.value_out += 1;
	cases.push(p);
	let mut p = public.clone();
	p.fee = p.fee.wrapping_sub(1);
	cases.push(p);
	let mut p = public.clone();
	p.binding.0[1] ^= 1;
	cases.push(p);
	cases
}

#[test]
fn proofs_verify_and_bind_public_inputs() {
	for (name, witness) in [("transfer", transfer()), ("shield", shield()), ("unshield", unshield())]
	{
		let start = Instant::now();
		let (proof, public) = prove(&witness).unwrap();
		let proving = start.elapsed();
		let start = Instant::now();
		assert!(verify(&proof, &public), "{name}");
		let verifying = start.elapsed();
		println!("{name}: proof {} bytes, prove {proving:?}, verify {verifying:?}", proof.len());

		for p in tampered(&public) {
			assert!(!verify(&proof, &p), "{name}: tampered {p:?}");
		}
		let mut corrupted = proof.clone();
		let middle = corrupted.len() / 2;
		corrupted[middle] ^= 1;
		assert!(!verify(&corrupted, &public), "{name}: corrupted proof");
		assert!(!verify(&proof[..proof.len() - 1], &public), "{name}: truncated proof");
	}
}

#[test]
fn proofs_are_randomized() {
	let witness = shield();
	let (a, public) = prove(&witness).unwrap();
	let (b, _) = prove(&witness).unwrap();
	assert_ne!(a, b);
	assert!(verify(&a, &public) && verify(&b, &public));
}

#[test]
fn verifier_rejects_out_of_range_amounts() {
	let witness = shield();
	let (proof, mut public) = prove(&witness).unwrap();
	public.value_out = MAX_VALUE;
	assert!(!verify(&proof, &public));
}

#[test]
fn public_inputs_layout() {
	let public = transfer().public_inputs();
	let felts = public.to_field_elements();
	assert_eq!(felts.len(), super::layout::public::COUNT);
	assert_eq!(felts[super::layout::public::FEE], Felt::from_u64(10));
	assert_eq!(&felts[super::layout::public::BINDING..], &public.binding.to_felts());
}

#[test]
fn security_level() {
	assert!(super::config::conjectured_security_bits() >= 128);
}
