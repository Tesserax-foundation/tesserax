//! Trace layout of the shielded transaction circuit.
//!
//! Every row evaluates one Poseidon2 permutation, i.e. one call to `H(a, b, domain)`. Rows are
//! grouped in sections:
//!
//! - input `i` (36 rows from `36 * i`): owner tag, commitment (two rows), nullifier, then the
//!   32 levels of the Merkle path, the last one being compared to the anchor;
//! - output `j` (3 rows from `72 + 3 * j`): `rho`, then the commitment (two rows);
//! - padding up to [`TRACE_HEIGHT`].
//!
//! Next to the permutation columns, each row carries registers (nullifier key, Merkle
//! accumulator, value, running balance) and witness columns (sibling, path bit, value bits).

use core::borrow::{Borrow, BorrowMut};
use core::mem::size_of;
use p3_poseidon2_air::Poseidon2Cols;

/// Width of the Poseidon2 permutation.
pub const P2_WIDTH: usize = 12;
/// S-box degree of Poseidon2 over Goldilocks.
pub const SBOX_DEGREE: u64 = 7;
/// No intermediate S-box registers: constraints have degree 7.
pub const SBOX_REGISTERS: usize = 0;
/// Full rounds at the beginning and at the end.
pub const HALF_FULL_ROUNDS: usize = 4;
/// Partial rounds.
pub const PARTIAL_ROUNDS: usize = 22;

/// Poseidon2 columns of a row.
pub type PermCols<T> =
	Poseidon2Cols<T, P2_WIDTH, SBOX_DEGREE, SBOX_REGISTERS, HALF_FULL_ROUNDS, PARTIAL_ROUNDS>;

/// Bits of a note value.
pub const VALUE_BITS: usize = crate::note::VALUE_BITS;

/// Columns of a row.
#[repr(C)]
pub struct Row<T> {
	/// Poseidon2 permutation of this row.
	pub perm: PermCols<T>,
	/// Nullifier key of the current input.
	pub nk: [T; 4],
	/// Merkle accumulator: the commitment, then each node up to the root.
	pub acc: [T; 4],
	/// Sibling of the accumulator on Merkle rows.
	pub sibling: [T; 4],
	/// Position bit on Merkle rows: 1 if the accumulator is the right child.
	pub bit: T,
	/// Value of the current input note.
	pub value: T,
	/// Bits of the value checked on commitment rows.
	pub value_bits: [T; VALUE_BITS],
	/// Running balance: inputs minus outputs so far.
	pub balance: T,
}

/// Number of columns of the trace.
pub const NUM_COLS: usize = size_of::<Row<u8>>();

impl<T> Borrow<Row<T>> for [T] {
	fn borrow(&self) -> &Row<T> {
		debug_assert_eq!(self.len(), NUM_COLS);
		let (prefix, rows, suffix) = unsafe { self.align_to::<Row<T>>() };
		debug_assert!(prefix.is_empty() && suffix.is_empty() && rows.len() == 1);
		&rows[0]
	}
}

impl<T> BorrowMut<Row<T>> for [T] {
	fn borrow_mut(&mut self) -> &mut Row<T> {
		debug_assert_eq!(self.len(), NUM_COLS);
		let (prefix, rows, suffix) = unsafe { self.align_to_mut::<Row<T>>() };
		debug_assert!(prefix.is_empty() && suffix.is_empty() && rows.len() == 1);
		&mut rows[0]
	}
}

/// Number of rows of the trace.
pub const TRACE_HEIGHT: usize = 128;

/// Rows of an input section.
pub const INPUT_ROWS: usize = 4 + crate::tree::TREE_DEPTH;

/// First row of input `i`.
pub const fn input_row(i: usize) -> usize {
	INPUT_ROWS * i
}

/// First row of output `j`.
pub const fn output_row(j: usize) -> usize {
	2 * INPUT_ROWS + 3 * j
}

/// Number of rows used by the transaction; the others are padding.
pub const USED_ROWS: usize = output_row(2);

/// Periodic selector columns, one value per row.
pub mod selector {
	/// Owner tag of an input: `H(nk, 0, TAG)`.
	pub const TAG: usize = 0;
	/// First commitment row of an input: `H(tag, rho, CM_OWNER)`.
	pub const IN_OWNER: usize = 1;
	/// Second commitment row of an input: `H(owner, [v, r], CM_VALUE)`.
	pub const IN_VALUE: usize = 2;
	/// Nullifier of input 1.
	pub const NF1: usize = 3;
	/// Nullifier of input 2.
	pub const NF2: usize = 4;
	/// Merkle path rows.
	pub const MERKLE: usize = 5;
	/// Last Merkle row: its output is the root.
	pub const ROOT: usize = 6;
	/// `rho` of output 1.
	pub const RHO1: usize = 7;
	/// `rho` of output 2.
	pub const RHO2: usize = 8;
	/// First commitment row of an output.
	pub const OUT_OWNER: usize = 9;
	/// Commitment of output 1.
	pub const OUT_VALUE1: usize = 10;
	/// Commitment of output 2.
	pub const OUT_VALUE2: usize = 11;
	/// The next row belongs to the same input: the nullifier key and value carry over.
	pub const CONTINUE: usize = 12;

	/// Number of selector columns.
	pub const COUNT: usize = 13;
}

/// Selector columns: `columns[s][row]` is 1 if selector `s` is set on `row`.
pub fn selector_columns() -> [[bool; TRACE_HEIGHT]; selector::COUNT] {
	let mut columns = [[false; TRACE_HEIGHT]; selector::COUNT];
	for i in 0..2 {
		let base = input_row(i);
		columns[selector::TAG][base] = true;
		columns[selector::IN_OWNER][base + 1] = true;
		columns[selector::IN_VALUE][base + 2] = true;
		columns[if i == 0 { selector::NF1 } else { selector::NF2 }][base + 3] = true;
		for row in base + 4..base + INPUT_ROWS {
			columns[selector::MERKLE][row] = true;
		}
		columns[selector::ROOT][base + INPUT_ROWS - 1] = true;
		for row in base..base + INPUT_ROWS - 1 {
			columns[selector::CONTINUE][row] = true;
		}
	}
	for j in 0..2 {
		let base = output_row(j);
		columns[if j == 0 { selector::RHO1 } else { selector::RHO2 }][base] = true;
		columns[selector::OUT_OWNER][base + 1] = true;
		columns[if j == 0 { selector::OUT_VALUE1 } else { selector::OUT_VALUE2 }][base + 2] = true;
	}
	columns
}

/// Indices of the public values.
pub mod public {
	pub const ANCHOR: usize = 0;
	pub const NULLIFIER1: usize = 4;
	pub const NULLIFIER2: usize = 8;
	pub const COMMITMENT1: usize = 12;
	pub const COMMITMENT2: usize = 16;
	pub const VALUE_IN: usize = 20;
	pub const VALUE_OUT: usize = 21;
	pub const FEE: usize = 22;
	/// Hash of the data the proof is bound to: transparent recipient and encrypted notes.
	pub const BINDING: usize = 23;

	/// Number of public values.
	pub const COUNT: usize = 27;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn layout_fits() {
		assert_eq!(USED_ROWS, 78);
		assert!(USED_ROWS < TRACE_HEIGHT);
		assert!(TRACE_HEIGHT.is_power_of_two());
	}

	#[test]
	fn selectors_do_not_overlap() {
		let columns = selector_columns();
		for row in 0..TRACE_HEIGHT {
			let kinds = (0..selector::COUNT)
				.filter(|s| *s != selector::ROOT && *s != selector::CONTINUE && *s != selector::MERKLE)
				.filter(|s| columns[*s][row])
				.count();
			assert!(kinds <= 1, "row {row}");
			if columns[selector::ROOT][row] {
				assert!(columns[selector::MERKLE][row]);
			}
		}
		assert!((USED_ROWS..TRACE_HEIGHT).all(|row| (0..selector::COUNT).all(|s| !columns[s][row])));
	}
}
