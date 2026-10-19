//! Poseidon2 hashing over the Goldilocks field.

use codec::{Decode, DecodeWithMemTracking, Encode, Error, Input, MaxEncodedLen};
use p3_field::{PrimeCharacteristicRing, PrimeField64};
use p3_goldilocks::{default_goldilocks_poseidon2_12, Goldilocks};
use p3_symmetric::Permutation;
use scale_info::TypeInfo;

/// Field element.
pub type Felt = Goldilocks;

/// Width of the Poseidon2 permutation.
pub const WIDTH: usize = 12;

/// Number of field elements in a digest.
pub const DIGEST_LEN: usize = 4;

/// Order of the Goldilocks field.
pub const MODULUS: u64 = 0xFFFF_FFFF_0000_0001;

/// Hash domains.
pub mod domain {
	pub const NK: u64 = 1;
	pub const TAG: u64 = 2;
	pub const CM_OWNER: u64 = 3;
	pub const CM_VALUE: u64 = 4;
	pub const NF: u64 = 5;
	pub const NODE: u64 = 6;
	/// `RHO + j` for output `j`.
	pub const RHO: u64 = 7;
}

/// A 256-bit digest: four canonical field elements.
///
/// Decoding rejects non-canonical encodings, so that every digest has a single encoding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, TypeInfo)]
pub struct Digest(pub [u64; DIGEST_LEN]);

impl MaxEncodedLen for Digest {
	fn max_encoded_len() -> usize {
		8 * DIGEST_LEN
	}
}

impl Decode for Digest {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let limbs = <[u64; DIGEST_LEN]>::decode(input)?;
		Digest::from_canonical(limbs).ok_or_else(|| "non-canonical field element".into())
	}
}

impl DecodeWithMemTracking for Digest {}

impl Digest {
	/// The zero digest, also the empty leaf of the note tree.
	pub const ZERO: Digest = Digest([0; DIGEST_LEN]);

	/// A digest from four limbs, if they are all canonical field elements.
	pub fn from_canonical(limbs: [u64; DIGEST_LEN]) -> Option<Self> {
		limbs.iter().all(|limb| *limb < MODULUS).then_some(Digest(limbs))
	}

	/// The field elements of the digest.
	pub fn to_felts(self) -> [Felt; DIGEST_LEN] {
		self.0.map(Felt::from_u64)
	}

	/// The digest of four field elements.
	pub fn from_felts(felts: [Felt; DIGEST_LEN]) -> Self {
		Digest(felts.map(|f| f.as_canonical_u64()))
	}

	/// Read 32 bytes as a digest, reducing each little-endian limb modulo the field order.
	pub fn from_bytes_reduced(bytes: &[u8; 32]) -> Self {
		let mut limbs = [0u64; DIGEST_LEN];
		for (i, limb) in limbs.iter_mut().enumerate() {
			let chunk: [u8; 8] = bytes[i * 8..i * 8 + 8].try_into().expect("8 bytes; qed");
			*limb = Felt::from_u64(u64::from_le_bytes(chunk)).as_canonical_u64();
		}
		Digest(limbs)
	}

	/// Little-endian bytes of the limbs.
	pub fn to_bytes(self) -> [u8; 32] {
		let mut bytes = [0u8; 32];
		for (i, limb) in self.0.iter().enumerate() {
			bytes[i * 8..i * 8 + 8].copy_from_slice(&limb.to_le_bytes());
		}
		bytes
	}
}

/// The Poseidon2 permutation of width 12 with the Plonky3 Goldilocks constants.
pub fn permute(state: [Felt; WIDTH]) -> [Felt; WIDTH] {
	default_goldilocks_poseidon2_12().permute(state)
}

/// Input state of the permutation for `H(a, b, domain)`.
pub fn hash_input(a: &[Felt; DIGEST_LEN], b: &[Felt; DIGEST_LEN], domain: u64) -> [Felt; WIDTH] {
	let mut state = [Felt::ZERO; WIDTH];
	state[..4].copy_from_slice(a);
	state[4..8].copy_from_slice(b);
	state[8] = Felt::from_u64(domain);
	state
}

/// `H(a, b, domain)`: the first four elements of `P(a || b || [domain, 0, 0, 0])`.
pub fn hash_felts(a: &[Felt; DIGEST_LEN], b: &[Felt; DIGEST_LEN], domain: u64) -> [Felt; DIGEST_LEN] {
	let output = permute(hash_input(a, b, domain));
	[output[0], output[1], output[2], output[3]]
}

/// `H(a, b, domain)` on digests.
pub fn hash(a: &Digest, b: &Digest, domain: u64) -> Digest {
	Digest::from_felts(hash_felts(&a.to_felts(), &b.to_felts(), domain))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn non_canonical_digests_do_not_decode() {
		let canonical = Digest([1, 2, 3, MODULUS - 1]);
		assert_eq!(Digest::decode(&mut &canonical.encode()[..]).unwrap(), canonical);
		let non_canonical = [1u64, 2, 3, MODULUS].encode();
		assert!(Digest::decode(&mut &non_canonical[..]).is_err());
	}

	#[test]
	fn domains_separate() {
		let a = Digest([1, 2, 3, 4]);
		let b = Digest([5, 6, 7, 8]);
		assert_ne!(hash(&a, &b, domain::NF), hash(&a, &b, domain::NODE));
		assert_ne!(hash(&a, &b, domain::NODE), hash(&b, &a, domain::NODE));
		assert_eq!(hash(&a, &b, domain::NODE), hash(&a, &b, domain::NODE));
	}

	#[test]
	fn bytes_roundtrip() {
		let digest = Digest([1, 2, MODULUS - 1, 42]);
		assert_eq!(Digest::from_bytes_reduced(&digest.to_bytes()), digest);
	}
}
