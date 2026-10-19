//! Constraints of the shielded transaction circuit. See `docs/privacy.md` for the statement.

use super::layout::{
	public, selector, selector_columns, PermCols, Row, HALF_FULL_ROUNDS, NUM_COLS, P2_WIDTH,
	PARTIAL_ROUNDS, VALUE_BITS,
};
use crate::hash::domain;
use alloc::{borrow::Cow, vec::Vec};
use core::borrow::Borrow;
use p3_air::{Air, AirBuilder, BaseAir, WindowAccess};
use p3_field::PrimeCharacteristicRing;
use p3_goldilocks::{
	GenericPoseidon2LinearLayersGoldilocks as LinearLayers, Goldilocks,
	GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_FINAL, GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_INITIAL,
	GOLDILOCKS_POSEIDON2_RC_12_INTERNAL,
};
use p3_poseidon2::GenericPoseidon2LinearLayers;

/// Maximum constraint degree: the Poseidon2 S-box `x^7`.
pub const MAX_CONSTRAINT_DEGREE: usize = 7;

/// The shielded transaction AIR.
pub struct ShieldedAir {
	periodic: Vec<Vec<Goldilocks>>,
}

impl Default for ShieldedAir {
	fn default() -> Self {
		let periodic = selector_columns()
			.iter()
			.map(|column| column.iter().map(|set| Goldilocks::from_bool(*set)).collect())
			.collect();
		ShieldedAir { periodic }
	}
}

impl BaseAir<Goldilocks> for ShieldedAir {
	fn width(&self) -> usize {
		NUM_COLS
	}

	fn num_periodic_columns(&self) -> usize {
		selector::COUNT
	}

	fn periodic_columns(&self) -> Cow<'_, [Vec<Goldilocks>]> {
		Cow::Borrowed(&self.periodic)
	}

	fn max_constraint_degree(&self) -> Option<usize> {
		Some(MAX_CONSTRAINT_DEGREE)
	}

	fn num_public_values(&self) -> usize {
		public::COUNT
	}
}

/// Output digest of a permutation: the first four elements of the final state.
fn output<T: Copy>(perm: &PermCols<T>) -> [T; 4] {
	let last = &perm.ending_full_rounds[HALF_FULL_ROUNDS - 1].post;
	[last[0], last[1], last[2], last[3]]
}

impl<AB: AirBuilder<F = Goldilocks>> Air<AB> for ShieldedAir {
	fn eval(&self, builder: &mut AB) {
		let main = builder.main();
		let local: &Row<AB::Var> = main.current_slice().borrow();
		let next: &Row<AB::Var> = main.next_slice().borrow();

		let s: Vec<AB::Expr> = builder.periodic_values().iter().map(|v| (*v).into()).collect();
		let pis: Vec<AB::Expr> = builder.public_values().iter().map(|v| (*v).into()).collect();
		let pi = |index: usize| -> [AB::Expr; 4] { core::array::from_fn(|k| pis[index + k].clone()) };

		// Every row is a valid Poseidon2 permutation.
		eval_poseidon2::<AB>(builder, &local.perm);

		let input: [AB::Expr; P2_WIDTH] = local.perm.inputs.map(Into::into);
		let out = output(&local.perm).map(Into::<AB::Expr>::into);
		let next_input: [AB::Expr; P2_WIDTH] = next.perm.inputs.map(Into::into);
		let nk = local.nk.map(Into::<AB::Expr>::into);
		let acc = local.acc.map(Into::<AB::Expr>::into);
		let sibling = local.sibling.map(Into::<AB::Expr>::into);
		let bit: AB::Expr = local.bit.into();
		let value: AB::Expr = local.value.into();

		// Domain and zero capacity of every row used by the transaction.
		let domains = [
			(selector::TAG, domain::TAG),
			(selector::IN_OWNER, domain::CM_OWNER),
			(selector::IN_VALUE, domain::CM_VALUE),
			(selector::NF1, domain::NF),
			(selector::NF2, domain::NF),
			(selector::MERKLE, domain::NODE),
			(selector::RHO1, domain::RHO),
			(selector::RHO2, domain::RHO + 1),
			(selector::OUT_OWNER, domain::CM_OWNER),
			(selector::OUT_VALUE1, domain::CM_VALUE),
			(selector::OUT_VALUE2, domain::CM_VALUE),
		];
		for (sel, dom) in domains {
			let mut when = builder.when(s[sel].clone());
			when.assert_eq(input[8].clone(), AB::Expr::from_u64(dom));
			for k in 9..P2_WIDTH {
				when.assert_zero(input[k].clone());
			}
		}

		// Owner tag: `H(nk, 0, TAG)`, then fed into the first commitment row.
		{
			let mut when = builder.when(s[selector::TAG].clone());
			for k in 0..4 {
				when.assert_eq(input[k].clone(), nk[k].clone());
				when.assert_zero(input[4 + k].clone());
			}
		}
		{
			let mut when = builder.when_transition();
			let mut when = when.when(s[selector::TAG].clone());
			for k in 0..4 {
				when.assert_eq(next_input[k].clone(), out[k].clone());
			}
		}

		// Input commitment: owner digest fed into the value row; the value register.
		{
			let mut when = builder.when_transition();
			let mut when = when.when(s[selector::IN_OWNER].clone());
			for k in 0..4 {
				when.assert_eq(next_input[k].clone(), out[k].clone());
			}
		}
		builder.when(s[selector::IN_VALUE].clone()).assert_eq(value.clone(), input[4].clone());

		// Nullifier: `H(nk, cm, NF)`; the commitment also starts the Merkle path.
		{
			let mut when = builder.when_transition();
			let mut when = when.when(s[selector::IN_VALUE].clone());
			for k in 0..4 {
				when.assert_eq(next_input[4 + k].clone(), out[k].clone());
				when.assert_eq(next.acc[k].into(), out[k].clone());
			}
		}
		let nf_row = s[selector::NF1].clone() + s[selector::NF2].clone();
		{
			let mut when = builder.when(nf_row.clone());
			for k in 0..4 {
				when.assert_eq(input[k].clone(), nk[k].clone());
			}
		}
		{
			let nf1 = pi(public::NULLIFIER1);
			let nf2 = pi(public::NULLIFIER2);
			for k in 0..4 {
				builder.when(s[selector::NF1].clone()).assert_eq(out[k].clone(), nf1[k].clone());
				builder.when(s[selector::NF2].clone()).assert_eq(out[k].clone(), nf2[k].clone());
			}
			let mut when = builder.when_transition();
			let mut when = when.when(nf_row);
			for k in 0..4 {
				when.assert_eq(next.acc[k].into(), acc[k].clone());
			}
		}

		// Merkle path: children ordered by the position bit, accumulator updated with each node.
		{
			let mut when = builder.when(s[selector::MERKLE].clone());
			when.assert_bool(bit.clone());
			for k in 0..4 {
				let left = acc[k].clone() + bit.clone() * (sibling[k].clone() - acc[k].clone());
				let right = sibling[k].clone() + bit.clone() * (acc[k].clone() - sibling[k].clone());
				when.assert_eq(input[k].clone(), left);
				when.assert_eq(input[4 + k].clone(), right);
			}
		}
		{
			let mut when = builder.when_transition();
			let mut when = when.when(s[selector::MERKLE].clone() - s[selector::ROOT].clone());
			for k in 0..4 {
				when.assert_eq(next.acc[k].into(), out[k].clone());
			}
		}
		// Notes of value 0 (dummy inputs) are exempt from membership.
		{
			let anchor = pi(public::ANCHOR);
			let mut when = builder.when(s[selector::ROOT].clone() * value.clone());
			for k in 0..4 {
				when.assert_eq(out[k].clone(), anchor[k].clone());
			}
		}

		// The nullifier key and value stay the same within an input section.
		{
			let mut when = builder.when_transition();
			let mut when = when.when(s[selector::CONTINUE].clone());
			for k in 0..4 {
				when.assert_eq(next.nk[k].into(), nk[k].clone());
			}
			when.assert_eq(next.value.into(), value.clone());
		}

		// Output rho: `H(nf1, nf2, RHO + j)`, fed into the first commitment row.
		{
			let nf1 = pi(public::NULLIFIER1);
			let nf2 = pi(public::NULLIFIER2);
			let mut when = builder.when(s[selector::RHO1].clone() + s[selector::RHO2].clone());
			for k in 0..4 {
				when.assert_eq(input[k].clone(), nf1[k].clone());
				when.assert_eq(input[4 + k].clone(), nf2[k].clone());
			}
		}
		{
			let mut when = builder.when_transition();
			let mut when = when.when(s[selector::RHO1].clone() + s[selector::RHO2].clone());
			for k in 0..4 {
				when.assert_eq(next_input[4 + k].clone(), out[k].clone());
			}
		}
		// Output commitment.
		{
			let mut when = builder.when_transition();
			let mut when = when.when(s[selector::OUT_OWNER].clone());
			for k in 0..4 {
				when.assert_eq(next_input[k].clone(), out[k].clone());
			}
		}
		{
			let cm1 = pi(public::COMMITMENT1);
			let cm2 = pi(public::COMMITMENT2);
			for k in 0..4 {
				builder.when(s[selector::OUT_VALUE1].clone()).assert_eq(out[k].clone(), cm1[k].clone());
				builder.when(s[selector::OUT_VALUE2].clone()).assert_eq(out[k].clone(), cm2[k].clone());
			}
		}

		// Every note value is below `2^VALUE_BITS`.
		let out_value_row = s[selector::OUT_VALUE1].clone() + s[selector::OUT_VALUE2].clone();
		{
			let value_row = s[selector::IN_VALUE].clone() + out_value_row.clone();
			let mut when = builder.when(value_row);
			let mut sum = AB::Expr::ZERO;
			for (k, b) in local.value_bits.iter().enumerate() {
				let b: AB::Expr = (*b).into();
				when.assert_bool(b.clone());
				sum += b * AB::Expr::from_u64(1 << k);
			}
			debug_assert_eq!(local.value_bits.len(), VALUE_BITS);
			when.assert_eq(sum, input[4].clone());
		}

		// Balance: inputs plus the public input amount equal outputs, public output and fee.
		let balance: AB::Expr = local.balance.into();
		builder.when_first_row().assert_zero(balance.clone());
		builder.when_transition().assert_eq(
			next.balance.into(),
			balance.clone() + s[selector::IN_VALUE].clone() * input[4].clone() -
				out_value_row * input[4].clone(),
		);
		builder.when_last_row().assert_zero(
			balance + pis[public::VALUE_IN].clone() -
				pis[public::VALUE_OUT].clone() -
				pis[public::FEE].clone(),
		);
	}
}

/// Poseidon2 permutation constraints.
///
/// Adapted from `p3-poseidon2-air` (Plonky3, MIT/Apache-2.0), with the round constants of the
/// Goldilocks permutation used by [`crate::hash::permute`].
fn eval_poseidon2<AB: AirBuilder<F = Goldilocks>>(builder: &mut AB, perm: &PermCols<AB::Var>) {
	let mut state: [AB::Expr; P2_WIDTH] = perm.inputs.map(Into::into);
	LinearLayers::external_linear_layer(&mut state);

	let full_round = |builder: &mut AB,
	                  state: &mut [AB::Expr; P2_WIDTH],
	                  post: &[AB::Var; P2_WIDTH],
	                  constants: &[Goldilocks; P2_WIDTH]| {
		for (s, c) in state.iter_mut().zip(constants) {
			*s += AB::Expr::from(*c);
			*s = s.exp_const_u64::<7>();
		}
		LinearLayers::external_linear_layer(state);
		for (s, p) in state.iter_mut().zip(post) {
			builder.assert_eq(s.clone(), *p);
			*s = (*p).into();
		}
	};

	for round in 0..HALF_FULL_ROUNDS {
		full_round(
			builder,
			&mut state,
			&perm.beginning_full_rounds[round].post,
			&GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_INITIAL[round],
		);
	}

	for round in 0..PARTIAL_ROUNDS {
		state[0] += AB::Expr::from(GOLDILOCKS_POSEIDON2_RC_12_INTERNAL[round]);
		state[0] = state[0].exp_const_u64::<7>();
		builder.assert_eq(state[0].clone(), perm.partial_rounds[round].post_sbox);
		state[0] = perm.partial_rounds[round].post_sbox.into();
		LinearLayers::internal_linear_layer(&mut state);
	}

	for round in 0..HALF_FULL_ROUNDS {
		full_round(
			builder,
			&mut state,
			&perm.ending_full_rounds[round].post,
			&GOLDILOCKS_POSEIDON2_RC_12_EXTERNAL_FINAL[round],
		);
	}
}
