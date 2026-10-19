//! Notes, commitments and nullifiers.

use crate::hash::{domain, hash, Digest};
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

/// Number of bits of a note value: values are below `2^60` planck.
pub const VALUE_BITS: usize = 60;

/// Exclusive upper bound of note values and public amounts.
pub const MAX_VALUE: u64 = 1 << VALUE_BITS;

/// Number of field elements of the note randomness.
pub const RANDOMNESS_LEN: usize = 3;

/// A shielded note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen)]
pub struct Note {
	/// Value in planck, below [`MAX_VALUE`].
	pub value: u64,
	/// Owner tag of the recipient.
	pub tag: Digest,
	/// Uniqueness digest, derived from the nullifiers of the transaction that created the note.
	pub rho: Digest,
	/// Hiding randomness: three canonical field elements.
	pub r: [u64; RANDOMNESS_LEN],
}

impl Note {
	/// The note commitment: `H(H(tag, rho, CM_OWNER), [v, r0, r1, r2], CM_VALUE)`.
	pub fn commitment(&self) -> Digest {
		let owner = hash(&self.tag, &self.rho, domain::CM_OWNER);
		let value = Digest([self.value, self.r[0], self.r[1], self.r[2]]);
		hash(&owner, &value, domain::CM_VALUE)
	}

	/// Whether the value and the randomness are in range.
	pub fn is_well_formed(&self) -> bool {
		self.value < MAX_VALUE && self.r.iter().all(|r| *r < crate::hash::MODULUS)
	}
}

/// Nullifier of the note with commitment `commitment`, owned by `nk`.
pub fn nullifier(nk: &Digest, commitment: &Digest) -> Digest {
	hash(nk, commitment, domain::NF)
}

/// Uniqueness digest of output `index` of a transaction spending `nullifiers`.
pub fn output_rho(nullifiers: &[Digest; 2], index: u64) -> Digest {
	hash(&nullifiers[0], &nullifiers[1], domain::RHO + index)
}

/// Random note randomness.
#[cfg(feature = "std")]
pub fn random_randomness() -> [u64; RANDOMNESS_LEN] {
	let mut bytes = [0u8; 8 * RANDOMNESS_LEN];
	getrandom::fill(&mut bytes).expect("the operating system RNG is available; qed");
	core::array::from_fn(|i| {
		let chunk: [u8; 8] = bytes[i * 8..i * 8 + 8].try_into().expect("8 bytes; qed");
		use p3_field::{PrimeCharacteristicRing, PrimeField64};
		crate::hash::Felt::from_u64(u64::from_le_bytes(chunk)).as_canonical_u64()
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn note(value: u64) -> Note {
		Note { value, tag: Digest([1, 2, 3, 4]), rho: Digest([5, 6, 7, 8]), r: [9, 10, 11] }
	}

	#[test]
	fn commitment_binds_every_field() {
		let base = note(5);
		let cm = base.commitment();
		assert_ne!(cm, note(6).commitment());
		assert_ne!(cm, Note { tag: Digest([1, 2, 3, 5]), ..base }.commitment());
		assert_ne!(cm, Note { rho: Digest([5, 6, 7, 9]), ..base }.commitment());
		assert_ne!(cm, Note { r: [9, 10, 12], ..base }.commitment());
	}

	#[test]
	fn nullifier_depends_on_key_and_note() {
		let cm = note(5).commitment();
		let nk = Digest([1, 1, 1, 1]);
		assert_ne!(nullifier(&nk, &cm), nullifier(&Digest([1, 1, 1, 2]), &cm));
		assert_ne!(nullifier(&nk, &cm), nullifier(&nk, &note(6).commitment()));
	}

	#[test]
	fn output_rhos_differ() {
		let nullifiers = [Digest([1, 2, 3, 4]), Digest([5, 6, 7, 8])];
		assert_ne!(output_rho(&nullifiers, 0), output_rho(&nullifiers, 1));
	}

	#[test]
	fn value_range() {
		assert!(note(MAX_VALUE - 1).is_well_formed());
		assert!(!note(MAX_VALUE).is_well_formed());
	}
}
