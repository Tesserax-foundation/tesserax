//! STARK proofs of shielded transactions.
//!
//! A proof shows that a transaction spends two notes of the tree and creates two notes while
//! preserving value, without revealing the notes. See `docs/privacy.md`.

pub mod air;
pub mod config;
pub mod layout;
#[cfg(feature = "std")]
mod prover;

#[cfg(feature = "std")]
pub use prover::{prove, InputWitness, OutputWitness, ProveError, Witness};

use crate::{
	hash::{Digest, Felt},
	note::MAX_VALUE,
};
use air::ShieldedAir;
use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use p3_field::PrimeCharacteristicRing;
use scale_info::TypeInfo;

/// Upper bound of an encoded proof, in bytes.
pub const MAX_PROOF_SIZE: usize = 512 * 1024;

/// Public data of a shielded transaction.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen)]
pub struct PublicInputs {
	/// Root of the note tree the input notes belong to.
	pub anchor: Digest,
	/// Nullifiers of the two input notes.
	pub nullifiers: [Digest; 2],
	/// Commitments of the two output notes.
	pub commitments: [Digest; 2],
	/// Amount moved from a transparent account into the pool.
	pub value_in: u64,
	/// Amount moved from the pool to a transparent account.
	pub value_out: u64,
	/// Fee paid from the pool.
	pub fee: u64,
	/// Hash of the data the proof is bound to: transparent recipient and encrypted notes.
	pub binding: Digest,
}

impl PublicInputs {
	/// Whether the public amounts are below the value bound, so that the balance equation of the
	/// circuit cannot wrap around the field modulus.
	pub fn amounts_in_range(&self) -> bool {
		self.value_in < MAX_VALUE && self.value_out < MAX_VALUE && self.fee < MAX_VALUE
	}

	/// Public values of the circuit, in the order of [`layout::public`].
	pub fn to_field_elements(&self) -> Vec<Felt> {
		let mut values = Vec::with_capacity(layout::public::COUNT);
		values.extend(self.anchor.to_felts());
		values.extend(self.nullifiers[0].to_felts());
		values.extend(self.nullifiers[1].to_felts());
		values.extend(self.commitments[0].to_felts());
		values.extend(self.commitments[1].to_felts());
		values.push(Felt::from_u64(self.value_in));
		values.push(Felt::from_u64(self.value_out));
		values.push(Felt::from_u64(self.fee));
		values.extend(self.binding.to_felts());
		debug_assert_eq!(values.len(), layout::public::COUNT);
		values
	}
}

/// Verify an encoded proof against public inputs.
pub fn verify(proof: &[u8], public: &PublicInputs) -> bool {
	if proof.len() > MAX_PROOF_SIZE || !public.amounts_in_range() {
		return false;
	}
	let Ok(proof) = postcard::from_bytes::<p3_uni_stark::Proof<config::Config>>(proof) else {
		return false;
	};
	p3_uni_stark::verify(
		&config::verifier_config(),
		&ShieldedAir::default(),
		&proof,
		&public.to_field_elements(),
	)
	.is_ok()
}

#[cfg(all(test, feature = "std"))]
mod tests;
