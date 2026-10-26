use super::*;
use crate::keyring::Keyring;
use sp_core::crypto::DEV_PHRASE;

fn pair(seed: u8) -> Pair {
	Pair::from_seed(&[seed; SEED_SIZE])
}

#[test]
fn sizes_match_ml_dsa_87() {
	let pair = pair(1);
	assert_eq!(pair.public().0.len(), 2592);
	assert_eq!(TraitPair::sign(&pair, b"message").0.len(), 4627);
	assert_eq!(AccountSignature::max_encoded_len(), 2592 + 4627);
}

#[test]
fn sign_and_verify() {
	let pair = pair(1);
	let signature = TraitPair::sign(&pair, b"message");
	assert!(Pair::verify(&signature, b"message", &pair.public()));
	assert!(!Pair::verify(&signature, b"other message", &pair.public()));
	assert!(!Pair::verify(&signature, b"message", &self::pair(2).public()));
}

#[test]
fn signatures_are_hedged() {
	let pair = pair(1);
	let a = TraitPair::sign(&pair, b"message");
	let b = TraitPair::sign(&pair, b"message");
	assert_ne!(a, b);
	assert!(Pair::verify(&a, b"message", &pair.public()));
	assert!(Pair::verify(&b, b"message", &pair.public()));
}

#[test]
fn signing_context_is_bound() {
	// A signature made without the Tesserax context must not verify.
	let pair = pair(1);
	let foreign = pair.signing_key().expanded_key().sign_deterministic(b"message", b"").unwrap();
	let mut encoded = [0u8; SIGNATURE_SIZE];
	encoded.copy_from_slice(foreign.encode().as_slice());
	assert!(!verify(&Signature(encoded), b"message", &pair.public()));
}

#[test]
fn matches_fips_204_verification() {
	// Signatures verify with the reference `verify_with_context` and the Tesserax context.
	let pair = pair(3);
	let signature = TraitPair::sign(&pair, b"payload");
	let decoded = ml_dsa::Signature::<MlDsa87>::try_from(&signature.0[..]).unwrap();
	use ml_dsa::signature::Keypair;
	assert!(pair.signing_key().verifying_key().verify_with_context(
		b"payload",
		SIGNING_CONTEXT,
		&decoded
	));
}

#[test]
fn malformed_inputs_do_not_verify() {
	let pair = pair(1);
	let mut signature = TraitPair::sign(&pair, b"message");
	signature.0[100] ^= 1;
	assert!(!Pair::verify(&signature, b"message", &pair.public()));
	assert!(!Pair::verify(&Signature([0xff; SIGNATURE_SIZE]), b"message", &pair.public()));
	assert!(!Pair::verify(&Signature([0; SIGNATURE_SIZE]), b"message", &pair.public()));
}

#[test]
fn account_signature() {
	let pair = pair(1);
	let signature = pair.sign_account(b"transaction");
	assert!(signature.verify(&b"transaction"[..], &pair.account_id()));
	assert!(!signature.verify(&b"transaction"[..], &self::pair(2).account_id()));
	assert!(!signature.verify(&b"other"[..], &pair.account_id()));

	// A valid signature with someone else's key does not authorize the account.
	let other = self::pair(2).sign_account(b"transaction");
	assert!(!other.verify(&b"transaction"[..], &pair.account_id()));
}

#[test]
fn account_id_is_hash_of_public_key() {
	let pair = pair(1);
	assert_eq!(
		pair.account_id(),
		AccountId32::new(sp_crypto_hashing::blake2_256(&pair.public().0))
	);
	assert_eq!(pair.public().into_account(), pair.account_id());
}

#[test]
fn key_generation_is_deterministic() {
	assert_eq!(pair(7).public(), pair(7).public());
	assert_ne!(pair(7).public(), pair(8).public());
}

#[test]
fn hard_derivation() {
	let alice = Pair::from_string("//Alice", None).unwrap();
	assert_eq!(alice.public(), Pair::from_string(&format!("{DEV_PHRASE}//Alice"), None).unwrap().public());
	assert_ne!(alice.public(), Pair::from_string("//Bob", None).unwrap().public());
	assert_ne!(
		alice.public(),
		Pair::from_string("//Alice///password", None).unwrap().public()
	);
}

#[test]
fn soft_derivation_is_rejected() {
	assert!(Pair::from_string("//Alice/soft", None).is_err());
}

#[test]
fn keyring_accounts_are_distinct() {
	let ids = Keyring::account_ids();
	let mut unique = ids.clone();
	unique.sort();
	unique.dedup();
	assert_eq!(unique.len(), ids.len());
	assert_eq!(Keyring::Alice.pair().public(), Pair::from_string("//Alice", None).unwrap().public());
}

#[test]
fn encoding_roundtrip() {
	let signature = pair(1).sign_account(b"message");
	let encoded = signature.encode();
	assert_eq!(encoded.len(), PUBLIC_KEY_SIZE + SIGNATURE_SIZE);
	assert_eq!(AccountSignature::decode(&mut &encoded[..]).unwrap(), signature);
}
