//! ML-DSA-87 keys and Tesserax addresses.

use crate::{Error, Result, TesseraxConfig};
use sp_core::crypto::{Ss58AddressFormat, Ss58Codec};
use tesserax_crypto::Pair;

/// SS58 prefix of Tesserax addresses: they start with `te`.
pub const SS58_PREFIX: u16 = 2261;

/// An account: the BLAKE2-256 hash of an ML-DSA-87 public key.
pub type AccountId = subxt::utils::AccountId32;

/// Parse a Tesserax address (SS58 with prefix 2261) or a 32-byte hexadecimal account id.
pub fn parse_address(text: &str) -> Result<AccountId> {
	let text = text.trim();
	if let Some(hex) = text.strip_prefix("0x") {
		let bytes: [u8; 32] = hex_bytes(hex)
			.and_then(|b| b.try_into().ok())
			.ok_or_else(|| Error::InvalidAddress(text.to_string()))?;
		return Ok(AccountId::from(bytes));
	}
	let (account, format) = sp_core::crypto::AccountId32::from_ss58check_with_version(text)
		.map_err(|e| Error::InvalidAddress(format!("{text}: {e:?}")))?;
	if u16::from(format) != SS58_PREFIX {
		return Err(Error::InvalidAddress(format!("{text}: not a Tesserax address")));
	}
	Ok(AccountId::from(<[u8; 32]>::from(account)))
}

/// Format an account as a Tesserax address.
pub fn format_address(account: &AccountId) -> String {
	sp_core::crypto::AccountId32::from(account.0)
		.to_ss58check_with_version(Ss58AddressFormat::custom(SS58_PREFIX))
}

pub(crate) fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
	if !hex.len().is_multiple_of(2) {
		return None;
	}
	(0..hex.len())
		.step_by(2)
		.map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
		.collect()
}

/// An ML-DSA-87 key pair that signs transactions.
#[derive(Clone)]
pub struct Keypair(Pair);

impl Keypair {
	/// A key from a secret URI: a 24-word phrase with optional derivation (`phrase//agent`), a
	/// development key (`//Alice`), or a 32-byte hexadecimal seed.
	pub fn from_uri(uri: &str) -> Result<Self> {
		<Pair as sp_core::Pair>::from_string(uri.trim(), None)
			.map(Keypair)
			.map_err(|e| Error::InvalidKey(format!("{e:?}")))
	}

	/// A new random key and its 24-word phrase (256 bits of entropy).
	pub fn generate() -> Result<(Self, String)> {
		let mut entropy = [0u8; 32];
		getrandom::fill(&mut entropy).map_err(|e| Error::InvalidKey(format!("no randomness: {e}")))?;
		let phrase = bip39::Mnemonic::from_entropy(&entropy)
			.map_err(|e| Error::InvalidKey(e.to_string()))?
			.to_string();
		Ok((Self::from_uri(&phrase)?, phrase))
	}

	/// The 32-byte seed of the key. Anyone knowing it controls the account: it derives the
	/// shielded key of the account, and must never leave the wallet.
	pub fn seed(&self) -> [u8; 32] {
		self.0.seed()
	}

	/// The account of the key.
	pub fn account_id(&self) -> AccountId {
		AccountId::from(<[u8; 32]>::from(self.0.account_id()))
	}

	/// The address of the key.
	pub fn address(&self) -> String {
		format_address(&self.account_id())
	}

	/// The ML-DSA-87 public key.
	pub fn public_key(&self) -> [u8; tesserax_crypto::PUBLIC_KEY_SIZE] {
		<Pair as sp_core::Pair>::public(&self.0).0
	}

	/// Hedged ML-DSA-87 signature of `message` with the FIPS 204 context `context`, for messages
	/// that are not transactions, such as vouchers.
	pub fn sign_with_context(&self, message: &[u8], context: &[u8]) -> Vec<u8> {
		self.0.sign_with_context(message, context).0.to_vec()
	}
}

impl core::fmt::Debug for Keypair {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		write!(f, "Keypair({})", self.address())
	}
}

impl subxt::tx::Signer<TesseraxConfig> for Keypair {
	fn account_id(&self) -> AccountId {
		Keypair::account_id(self)
	}

	fn sign(&self, signer_payload: &[u8]) -> tesserax_crypto::AccountSignature {
		self.0.sign_account(signer_payload)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn addresses_roundtrip() {
		let key = Keypair::from_uri("//Alice").unwrap();
		let address = key.address();
		assert!(address.starts_with("te"), "{address}");
		assert_eq!(parse_address(&address).unwrap(), key.account_id());
		let hex = format!("0x{}", key.account_id().0.iter().map(|b| format!("{b:02x}")).collect::<String>());
		assert_eq!(parse_address(&hex).unwrap(), key.account_id());
		// The development key of the README.
		assert_eq!(address, "teqhTi5xkMrt7j8tAhU8wndjgz7rn4DfmY97Qbww7ar5KA1b5");
	}

	#[test]
	fn other_networks_are_rejected() {
		let polkadot = sp_core::crypto::AccountId32::from([1u8; 32])
			.to_ss58check_with_version(Ss58AddressFormat::custom(0));
		assert!(parse_address(&polkadot).is_err());
		assert!(parse_address("0x1234").is_err());
		assert!(parse_address("garbage").is_err());
	}

	#[test]
	fn keys_from_phrases() {
		let (key, phrase) = Keypair::generate().unwrap();
		assert_eq!(phrase.split_whitespace().count(), 24);
		assert_eq!(Keypair::from_uri(&phrase).unwrap().account_id(), key.account_id());
		let derived = Keypair::from_uri(&format!("{phrase}//agent")).unwrap();
		assert_ne!(derived.account_id(), key.account_id());
	}
}
