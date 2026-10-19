//! Shielded keys and addresses.

use crate::hash::{domain, hash, Digest};

/// Nullifier key derived from a wallet seed: `H(seed, 0, NK)`.
pub fn nullifier_key(seed: &[u8; 32]) -> Digest {
	hash(&Digest::from_bytes_reduced(seed), &Digest::ZERO, domain::NK)
}

/// Owner tag of a nullifier key: `H(nk, 0, TAG)`.
pub fn owner_tag(nk: &Digest) -> Digest {
	hash(nk, &Digest::ZERO, domain::TAG)
}

#[cfg(feature = "std")]
pub use wallet::*;

#[cfg(feature = "std")]
mod wallet {
	use super::*;
	use ml_kem::{array::Array, ml_kem_768, Decapsulate, KeyExport, MlKem768};
	use sha3::{Digest as _, Sha3_512};

	/// Size of an encoded ML-KEM-768 encapsulation key.
	pub const ENCAPSULATION_KEY_SIZE: usize = 1184;

	/// Size of an encoded shielded address: owner tag and encapsulation key.
	pub const ADDRESS_SIZE: usize = 32 + ENCAPSULATION_KEY_SIZE;

	/// Shielded spending key: everything is derived from a 32 byte seed.
	#[derive(Clone)]
	pub struct SpendingKey {
		seed: [u8; 32],
	}

	/// Public shielded address.
	#[derive(Clone, Debug, PartialEq, Eq)]
	pub struct Address {
		pub tag: Digest,
		pub encapsulation_key: Vec<u8>,
	}

	impl SpendingKey {
		/// A spending key from a 32 byte seed.
		pub fn from_seed(seed: [u8; 32]) -> Self {
			SpendingKey { seed }
		}

		/// A new random spending key.
		pub fn generate() -> Self {
			let mut seed = [0u8; 32];
			getrandom::fill(&mut seed).expect("the operating system RNG is available; qed");
			SpendingKey { seed }
		}

		/// The seed of this key.
		pub fn seed(&self) -> [u8; 32] {
			self.seed
		}

		/// Nullifier key: required to spend.
		pub fn nullifier_key(&self) -> Digest {
			nullifier_key(&self.seed)
		}

		/// Owner tag of the notes received by this key.
		pub fn tag(&self) -> Digest {
			owner_tag(&self.nullifier_key())
		}

		/// ML-KEM-768 decapsulation key: the incoming viewing key.
		pub fn decapsulation_key(&self) -> ml_kem_768::DecapsulationKey {
			let mut hasher = Sha3_512::new();
			hasher.update(b"tessera-shielded-kem");
			hasher.update(self.seed);
			let kem_seed: [u8; 64] = hasher.finalize().into();
			ml_kem_768::DecapsulationKey::from_seed(Array::from(kem_seed))
		}

		/// Public address of this key.
		pub fn address(&self) -> Address {
			Address {
				tag: self.tag(),
				encapsulation_key: self
					.decapsulation_key()
					.encapsulation_key()
					.to_bytes()
					.to_vec(),
			}
		}
	}

	impl Address {
		/// Encoded address: owner tag followed by the encapsulation key.
		pub fn to_bytes(&self) -> Vec<u8> {
			let mut bytes = self.tag.to_bytes().to_vec();
			bytes.extend_from_slice(&self.encapsulation_key);
			bytes
		}

		/// Decode and validate an address.
		pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
			if bytes.len() != ADDRESS_SIZE {
				return None;
			}
			let tag_bytes: [u8; 32] = bytes[..32].try_into().ok()?;
			let tag = Digest::from_canonical(core::array::from_fn(|i| {
				u64::from_le_bytes(tag_bytes[i * 8..i * 8 + 8].try_into().expect("8 bytes"))
			}))?;
			let key = &bytes[32..];
			ml_kem_768::EncapsulationKey::new(key.try_into().ok()?).ok()?;
			Some(Address { tag, encapsulation_key: key.to_vec() })
		}

		/// The ML-KEM encapsulation key.
		pub(crate) fn kem_key(&self) -> Option<ml_kem_768::EncapsulationKey> {
			ml_kem_768::EncapsulationKey::new(self.encapsulation_key.as_slice().try_into().ok()?)
				.ok()
		}
	}

	/// Decapsulate with an incoming viewing key.
	pub(crate) fn decapsulate(
		key: &ml_kem_768::DecapsulationKey,
		ciphertext: &[u8],
	) -> Option<[u8; 32]> {
		let ciphertext: &ml_kem::Ciphertext<MlKem768> = ciphertext.try_into().ok()?;
		Some(key.decapsulate(ciphertext).into())
	}
}

#[cfg(all(test, feature = "std"))]
mod tests {
	use super::*;

	#[test]
	fn keys_are_deterministic() {
		let key = SpendingKey::from_seed([7; 32]);
		assert_eq!(key.address(), SpendingKey::from_seed([7; 32]).address());
		assert_ne!(key.address(), SpendingKey::from_seed([8; 32]).address());
		assert_eq!(key.tag(), owner_tag(&key.nullifier_key()));
	}

	#[test]
	fn address_roundtrip() {
		let address = SpendingKey::generate().address();
		let bytes = address.to_bytes();
		assert_eq!(bytes.len(), ADDRESS_SIZE);
		assert_eq!(Address::from_bytes(&bytes), Some(address));
		assert_eq!(Address::from_bytes(&bytes[1..]), None);
	}
}
