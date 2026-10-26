//! ML-DSA keys, signatures and accounts for Tesserax.
//!
//! Tesserax uses ML-DSA-87 (FIPS 204, NIST security category 5) for every signature: there is no
//! sr25519, ed25519 or ECDSA account on the chain.
//!
//! An account is identified by the BLAKE2-256 hash of its ML-DSA public key. Since an account id
//! does not reveal the public key, a transaction signature ([`AccountSignature`]) carries the
//! public key next to the ML-DSA signature. It is valid when the public key hashes to the sender
//! account and the signature verifies under that key.
//!
//! All signatures are computed with the FIPS 204 context string [`SIGNING_CONTEXT`], which
//! separates Tesserax signatures from signatures made with the same key for other protocols.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use core::fmt;
use ml_dsa::{EncodedVerifyingKey, MlDsa87, SigningKey, VerifyingKey, B32};
use scale_info::TypeInfo;
use sp_core::crypto::{
	ByteArray, CryptoType, CryptoTypeId, Derive, DeriveError, DeriveJunction,
	Pair as TraitPair, SecretStringError, UncheckedFrom,
};
use sp_runtime::{
	traits::{IdentifyAccount, Lazy, Verify},
	AccountId32,
};

pub mod keyring;

/// Identifier of the ML-DSA crypto type.
pub const CRYPTO_ID: CryptoTypeId = CryptoTypeId(*b"mlds");

/// Size of an encoded ML-DSA-87 public key.
pub const PUBLIC_KEY_SIZE: usize = 2592;

/// Size of an encoded ML-DSA-87 signature.
pub const SIGNATURE_SIZE: usize = 4627;

/// Size of the seed an ML-DSA key pair is generated from.
pub const SEED_SIZE: usize = 32;

/// FIPS 204 context string of every Tesserax signature.
pub const SIGNING_CONTEXT: &[u8] = b"tesserax";

/// Seed of an ML-DSA key pair (FIPS 204 `xi`).
pub type Seed = [u8; SEED_SIZE];

macro_rules! byte_array {
	($name:ident, $size:expr, $doc:literal) => {
		#[doc = $doc]
		#[derive(
			Clone,
			PartialEq,
			Eq,
			PartialOrd,
			Ord,
			Hash,
			Encode,
			Decode,
			DecodeWithMemTracking,
			MaxEncodedLen,
			TypeInfo,
		)]
		pub struct $name(pub [u8; $size]);

		impl AsRef<[u8]> for $name {
			fn as_ref(&self) -> &[u8] {
				&self.0
			}
		}

		impl AsMut<[u8]> for $name {
			fn as_mut(&mut self) -> &mut [u8] {
				&mut self.0
			}
		}

		impl TryFrom<&[u8]> for $name {
			type Error = ();

			fn try_from(data: &[u8]) -> Result<Self, ()> {
				data.try_into().map(Self).map_err(|_| ())
			}
		}

		impl ByteArray for $name {
			const LEN: usize = $size;
		}

		impl UncheckedFrom<[u8; $size]> for $name {
			fn unchecked_from(data: [u8; $size]) -> Self {
				Self(data)
			}
		}

		impl From<[u8; $size]> for $name {
			fn from(data: [u8; $size]) -> Self {
				Self(data)
			}
		}

		impl fmt::Debug for $name {
			fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
				write!(f, "{}(0x", stringify!($name))?;
				for byte in &self.0[..8] {
					write!(f, "{byte:02x}")?;
				}
				write!(f, "…)")
			}
		}

		impl CryptoType for $name {
			type Pair = Pair;
		}
	};
}

byte_array!(Public, PUBLIC_KEY_SIZE, "An ML-DSA-87 public key.");
byte_array!(Signature, SIGNATURE_SIZE, "An ML-DSA-87 signature.");

impl Derive for Public {}
impl sp_core::crypto::Public for Public {}
impl sp_core::crypto::Signature for Signature {}

impl Public {
	/// The account controlled by this key: the BLAKE2-256 hash of the encoded key.
	pub fn to_account_id(&self) -> AccountId32 {
		AccountId32::new(sp_crypto_hashing::blake2_256(&self.0))
	}
}

impl IdentifyAccount for Public {
	type AccountId = AccountId32;

	fn into_account(self) -> AccountId32 {
		self.to_account_id()
	}
}

/// Verify an ML-DSA signature over `message` with the Tesserax signing context.
pub fn verify(signature: &Signature, message: &[u8], public: &Public) -> bool {
	verify_with_context(&signature.0, message, SIGNING_CONTEXT, &public.0)
}

/// Verify an encoded ML-DSA-87 signature over `message` with the FIPS 204 context `context`.
///
/// Returns `false` for malformed keys or signatures and for contexts longer than 255 bytes.
pub fn verify_with_context(signature: &[u8], message: &[u8], context: &[u8], public: &[u8]) -> bool {
	let Ok(signature) = ml_dsa::Signature::<MlDsa87>::try_from(signature) else {
		return false;
	};
	let Ok(encoded) = EncodedVerifyingKey::<MlDsa87>::try_from(public) else {
		return false;
	};
	VerifyingKey::<MlDsa87>::decode(&encoded).verify_with_context(message, context, &signature)
}

/// Signature of a transaction: the signer's public key and its ML-DSA signature.
#[derive(
	Clone, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Debug,
)]
pub struct AccountSignature {
	pub signer: Public,
	pub signature: Signature,
}

impl Verify for AccountSignature {
	type Signer = Public;

	fn verify<L: Lazy<[u8]>>(&self, mut message: L, account: &AccountId32) -> bool {
		self.signer.to_account_id() == *account &&
			verify(&self.signature, message.get(), &self.signer)
	}
}

/// An ML-DSA-87 key pair.
#[derive(Clone)]
pub struct Pair {
	seed: Seed,
	public: Public,
}

impl Pair {
	/// The seed this pair was generated from. Anyone knowing it controls the account.
	pub fn seed(&self) -> Seed {
		self.seed
	}

	/// The account controlled by this pair.
	pub fn account_id(&self) -> AccountId32 {
		self.public.to_account_id()
	}

	/// Sign `message` and attach the public key, as expected in a transaction.
	#[cfg(feature = "full_crypto")]
	pub fn sign_account(&self, message: &[u8]) -> AccountSignature {
		AccountSignature { signer: self.public.clone(), signature: TraitPair::sign(self, message) }
	}

	/// Sign `message` with the FIPS 204 context `context` instead of the transaction context,
	/// for signatures verified by contracts. `context` must not exceed 255 bytes.
	#[cfg(feature = "full_crypto")]
	pub fn sign_with_context(&self, message: &[u8], context: &[u8]) -> Signature {
		assert!(context.len() <= 255, "FIPS 204 contexts are at most 255 bytes");
		let mut rnd = [0u8; 32];
		#[cfg(feature = "std")]
		getrandom::fill(&mut rnd).expect("the operating system RNG is available; qed");

		// `M' = 0 || len(ctx) || ctx || M`, FIPS 204 Algorithm 2.
		let signature = self.signing_key().expanded_key().sign_internal(
			&[&[0u8], &[context.len() as u8], context, message],
			&B32::from(rnd),
		);

		let mut encoded = [0u8; SIGNATURE_SIZE];
		encoded.copy_from_slice(signature.encode().as_slice());
		Signature(encoded)
	}

	#[cfg(feature = "full_crypto")]
	fn signing_key(&self) -> SigningKey<MlDsa87> {
		SigningKey::from_seed(&B32::from(self.seed))
	}
}

impl fmt::Debug for Pair {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("Pair").field("public", &self.public).finish_non_exhaustive()
	}
}

impl CryptoType for Pair {
	type Pair = Pair;
}

/// Hard derivation: the child seed is a hash of the parent seed and the junction.
fn derive_hard_junction(seed: &Seed, chain_code: &[u8; 32]) -> Seed {
	("MlDsaHDKD", seed, chain_code).using_encoded(sp_crypto_hashing::blake2_256)
}

impl TraitPair for Pair {
	type Public = Public;
	type Seed = Seed;
	type Signature = Signature;
	type ProofOfPossession = Signature;

	fn from_seed_slice(seed: &[u8]) -> Result<Pair, SecretStringError> {
		let seed: Seed = seed.try_into().map_err(|_| SecretStringError::InvalidSeedLength)?;
		let signing_key = SigningKey::<MlDsa87>::from_seed(&B32::from(seed));
		use ml_dsa::signature::Keypair;
		let mut public = [0u8; PUBLIC_KEY_SIZE];
		public.copy_from_slice(signing_key.verifying_key().encode().as_slice());
		Ok(Pair { seed, public: Public(public) })
	}

	/// ML-DSA has no soft derivation: only hard junctions are supported.
	fn derive<Iter: Iterator<Item = DeriveJunction>>(
		&self,
		path: Iter,
		_seed: Option<Seed>,
	) -> Result<(Pair, Option<Seed>), DeriveError> {
		let mut seed = self.seed;
		for junction in path {
			match junction {
				DeriveJunction::Soft(_) => return Err(DeriveError::SoftKeyInPath),
				DeriveJunction::Hard(chain_code) => seed = derive_hard_junction(&seed, &chain_code),
			}
		}
		Ok((Self::from_seed(&seed), Some(seed)))
	}

	fn public(&self) -> Public {
		self.public.clone()
	}

	/// Hedged signing (FIPS 204 default) when an operating system RNG is available, the
	/// deterministic variant otherwise.
	#[cfg(feature = "full_crypto")]
	fn sign(&self, message: &[u8]) -> Signature {
		self.sign_with_context(message, SIGNING_CONTEXT)
	}

	fn verify<M: AsRef<[u8]>>(signature: &Signature, message: M, public: &Public) -> bool {
		verify(signature, message.as_ref(), public)
	}

	fn to_raw_vec(&self) -> Vec<u8> {
		self.seed.to_vec()
	}
}

#[cfg(test)]
mod tests;
