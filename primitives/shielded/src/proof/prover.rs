//! Proof generation: builds the execution trace from the private witness.

use super::{
	air::ShieldedAir,
	config::prover_config,
	layout::{
		input_row, output_row, PermCols, Row, HALF_FULL_ROUNDS, NUM_COLS, P2_WIDTH,
		PARTIAL_ROUNDS, SBOX_DEGREE, SBOX_REGISTERS, TRACE_HEIGHT, VALUE_BITS,
	},
	PublicInputs,
};
use crate::{
	hash::{domain, hash_input, Digest, Felt},
	keys::owner_tag,
	note::{nullifier, output_rho, Note, MAX_VALUE},
	tree::{MerklePath, TREE_DEPTH},
};
use core::{borrow::BorrowMut, mem::MaybeUninit};
use p3_field::PrimeCharacteristicRing;
use p3_goldilocks::{
	GenericPoseidon2LinearLayersGoldilocks as LinearLayers, Goldilocks,
	GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_FINAL, GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_INITIAL,
	GOLDILOCKS_POSEIDON2_RC_12_INTERNAL,
};
use p3_matrix::dense::RowMajorMatrix;
use p3_poseidon2_air::{generate_trace_rows_for_perm, RoundConstants};

/// A spent note.
#[derive(Clone, Debug)]
pub struct InputWitness {
	/// Nullifier key of the owner.
	pub nk: Digest,
	/// The note. A dummy input is a note of value 0 owned by `nk`.
	pub note: Note,
	/// Authentication path of the note in the tree of `anchor`. Ignored for dummy inputs.
	pub path: MerklePath,
}

/// A created note. Its `rho` must be [`output_rho`] of the input nullifiers.
#[derive(Clone, Debug)]
pub struct OutputWitness {
	pub note: Note,
}

/// Private witness and public amounts of a transaction.
#[derive(Clone, Debug)]
pub struct Witness {
	pub inputs: [InputWitness; 2],
	pub outputs: [OutputWitness; 2],
	pub anchor: Digest,
	pub value_in: u64,
	pub value_out: u64,
	pub fee: u64,
	pub binding: Digest,
}

/// Why a proof could not be generated.
#[derive(Debug, PartialEq, Eq)]
pub enum ProveError {
	/// An input note is not owned by its nullifier key.
	WrongOwner,
	/// A value or an amount is out of range.
	ValueOutOfRange,
	/// An input path does not lead to the anchor.
	NotInTree,
	/// An output note does not use the expected `rho`.
	WrongRho,
	/// Inputs and outputs do not balance.
	Unbalanced,
	/// The proof system failed.
	Proving,
}

impl Witness {
	/// Nullifiers of the inputs.
	pub fn nullifiers(&self) -> [Digest; 2] {
		core::array::from_fn(|i| nullifier(&self.inputs[i].nk, &self.inputs[i].note.commitment()))
	}

	/// Public inputs of the transaction.
	pub fn public_inputs(&self) -> PublicInputs {
		PublicInputs {
			anchor: self.anchor,
			nullifiers: self.nullifiers(),
			commitments: core::array::from_fn(|j| self.outputs[j].note.commitment()),
			value_in: self.value_in,
			value_out: self.value_out,
			fee: self.fee,
			binding: self.binding,
		}
	}

	fn check(&self) -> Result<(), ProveError> {
		let nullifiers = self.nullifiers();
		for input in &self.inputs {
			if owner_tag(&input.nk) != input.note.tag {
				return Err(ProveError::WrongOwner);
			}
			if !input.note.is_well_formed() {
				return Err(ProveError::ValueOutOfRange);
			}
			if input.note.value != 0 &&
				(input.path.siblings.len() != TREE_DEPTH ||
					input.path.root(&input.note.commitment()) != self.anchor)
			{
				return Err(ProveError::NotInTree);
			}
		}
		for (j, output) in self.outputs.iter().enumerate() {
			if !output.note.is_well_formed() {
				return Err(ProveError::ValueOutOfRange);
			}
			if output.note.rho != output_rho(&nullifiers, j as u64) {
				return Err(ProveError::WrongRho);
			}
		}
		if [self.value_in, self.value_out, self.fee].iter().any(|v| *v >= MAX_VALUE) {
			return Err(ProveError::ValueOutOfRange);
		}
		let inputs = self.inputs.iter().map(|i| i.note.value as u128).sum::<u128>() +
			self.value_in as u128;
		let outputs = self.outputs.iter().map(|o| o.note.value as u128).sum::<u128>() +
			self.value_out as u128 +
			self.fee as u128;
		if inputs != outputs {
			return Err(ProveError::Unbalanced);
		}
		Ok(())
	}
}

fn round_constants() -> RoundConstants<Goldilocks, P2_WIDTH, HALF_FULL_ROUNDS, PARTIAL_ROUNDS> {
	RoundConstants::new(
		GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_INITIAL,
		GOLDILOCKS_POSEIDON2_RC_12_INTERNAL,
		GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_FINAL,
	)
}

struct TraceBuilder {
	values: Vec<Felt>,
	constants: RoundConstants<Goldilocks, P2_WIDTH, HALF_FULL_ROUNDS, PARTIAL_ROUNDS>,
}

impl TraceBuilder {
	fn new() -> Self {
		TraceBuilder { values: Felt::zero_vec(NUM_COLS * TRACE_HEIGHT), constants: round_constants() }
	}

	fn row(&mut self, index: usize) -> &mut Row<Felt> {
		self.values[index * NUM_COLS..(index + 1) * NUM_COLS].borrow_mut()
	}

	/// Fill the permutation columns of `index` and return the output digest.
	fn hash(&mut self, index: usize, input: [Felt; P2_WIDTH]) -> [Felt; 4] {
		self.permutation(index, input);
		let post = &self.row(index).perm.ending_full_rounds[HALF_FULL_ROUNDS - 1].post;
		[post[0], post[1], post[2], post[3]]
	}

	/// Fill the permutation columns of `index`.
	fn permutation(&mut self, index: usize, input: [Felt; P2_WIDTH]) {
		let constants = self.constants.clone();
		let perm: &mut PermCols<Felt> = &mut self.row(index).perm;
		// SAFETY: `MaybeUninit<Felt>` has the layout of `Felt`, and every column is written.
		let perm = unsafe {
			&mut *(perm as *mut PermCols<Felt>
				as *mut p3_poseidon2_air::Poseidon2Cols<
					MaybeUninit<Felt>,
					P2_WIDTH,
					SBOX_DEGREE,
					SBOX_REGISTERS,
					HALF_FULL_ROUNDS,
					PARTIAL_ROUNDS,
				>)
		};
		generate_trace_rows_for_perm::<
			Felt,
			LinearLayers,
			P2_WIDTH,
			SBOX_DEGREE,
			SBOX_REGISTERS,
			HALF_FULL_ROUNDS,
			PARTIAL_ROUNDS,
		>(perm, input, &constants);
	}

	fn value_bits(&mut self, index: usize, value: u64) {
		let row = self.row(index);
		for k in 0..VALUE_BITS {
			row.value_bits[k] = Felt::from_bool((value >> k) & 1 == 1);
		}
	}
}

fn felts(values: [u64; 4]) -> [Felt; 4] {
	values.map(Felt::from_u64)
}

/// Build the execution trace of a transaction.
fn trace(witness: &Witness) -> RowMajorMatrix<Felt> {
	let mut t = TraceBuilder::new();
	let nullifiers = witness.nullifiers();
	let zero = [Felt::ZERO; 4];

	for (i, input) in witness.inputs.iter().enumerate() {
		let base = input_row(i);
		let nk = input.nk.to_felts();
		let note = &input.note;
		for row in base..base + super::layout::INPUT_ROWS {
			let r = t.row(row);
			r.nk = nk;
			r.value = Felt::from_u64(note.value);
		}

		let tag = t.hash(base, hash_input(&nk, &zero, domain::TAG));
		let owner = t.hash(base + 1, hash_input(&tag, &note.rho.to_felts(), domain::CM_OWNER));
		let value = felts([note.value, note.r[0], note.r[1], note.r[2]]);
		let cm = t.hash(base + 2, hash_input(&owner, &value, domain::CM_VALUE));
		t.value_bits(base + 2, note.value);
		t.permutation(base + 3, hash_input(&nk, &cm, domain::NF));
		t.row(base + 3).acc = cm;

		let mut acc = cm;
		for height in 0..TREE_DEPTH {
			let row = base + 4 + height;
			let sibling = input.path.siblings.get(height).map(|d| d.to_felts()).unwrap_or(zero);
			let bit = (input.path.position >> height) & 1 == 1;
			let (left, right) = if bit { (sibling, acc) } else { (acc, sibling) };
			{
				let r = t.row(row);
				r.acc = acc;
				r.sibling = sibling;
				r.bit = Felt::from_bool(bit);
			}
			acc = t.hash(row, hash_input(&left, &right, domain::NODE));
		}
	}

	for (j, output) in witness.outputs.iter().enumerate() {
		let base = output_row(j);
		let note = &output.note;
		let rho = t.hash(
			base,
			hash_input(&nullifiers[0].to_felts(), &nullifiers[1].to_felts(), domain::RHO + j as u64),
		);
		let owner = t.hash(base + 1, hash_input(&note.tag.to_felts(), &rho, domain::CM_OWNER));
		let value = felts([note.value, note.r[0], note.r[1], note.r[2]]);
		t.permutation(base + 2, hash_input(&owner, &value, domain::CM_VALUE));
		t.value_bits(base + 2, note.value);
	}

	for row in super::layout::USED_ROWS..TRACE_HEIGHT {
		t.permutation(row, [Felt::ZERO; P2_WIDTH]);
	}

	// Running balance: inputs minus outputs of the rows before.
	let mut balance = Felt::ZERO;
	let selectors = super::layout::selector_columns();
	for row in 0..TRACE_HEIGHT {
		let value = t.row(row).perm.inputs[4];
		t.row(row).balance = balance;
		if selectors[super::layout::selector::IN_VALUE][row] {
			balance += value;
		}
		if selectors[super::layout::selector::OUT_VALUE1][row] ||
			selectors[super::layout::selector::OUT_VALUE2][row]
		{
			balance -= value;
		}
	}

	RowMajorMatrix::new(t.values, NUM_COLS)
}

/// Prove a transaction. Returns the encoded proof and the public inputs.
pub fn prove(witness: &Witness) -> Result<(Vec<u8>, PublicInputs), ProveError> {
	witness.check()?;
	let public = witness.public_inputs();
	let proof = p3_uni_stark::prove(
		&prover_config(),
		&ShieldedAir::default(),
		trace(witness),
		&public.to_field_elements(),
	)
	.map_err(|_| ProveError::Proving)?;
	let encoded = postcard::to_allocvec(&proof).map_err(|_| ProveError::Proving)?;
	Ok((encoded, public))
}

#[cfg(test)]
pub(super) fn unchecked_trace(witness: &Witness) -> RowMajorMatrix<Felt> {
	trace(witness)
}
