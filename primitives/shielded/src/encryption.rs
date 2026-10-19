//! Note encryption: ML-KEM-768 encapsulation and ChaCha20-Poly1305.

use crate::{
	hash::Digest,
	keys::{decapsulate, Address, SpendingKey},
	note::{Note, RANDOMNESS_LEN},
};
use chacha20poly1305::{
	aead::{Aead, KeyInit},
	ChaCha20Poly1305, Key, Nonce,
};
use ml_kem::array::Array;
use sha3::{Digest as _, Sha3_256};

/// Size of an ML-KEM-768 ciphertext.
pub const KEM_CIPHERTEXT_SIZE: usize = 1088;

/// Size of an encrypted note plaintext: value, rho and randomness.
pub const PLAINTEXT_SIZE: usize = 8 + 32 + 8 * RANDOMNESS_LEN;

/// Size of an encrypted note.
pub const ENCRYPTED_NOTE_SIZE: usize = KEM_CIPHERTEXT_SIZE + PLAINTEXT_SIZE + 16;

fn symmetric_key(shared: &[u8; 32]) -> Key {
	let mut hasher = Sha3_256::new();
	hasher.update(b"tessera-note-encryption");
	hasher.update(shared);
	Key::from(<[u8; 32]>::from(hasher.finalize()))
}

/// Encrypt `note` for `recipient`. Returns `None` if the address key is invalid.
pub fn encrypt(note: &Note, recipient: &Address) -> Option<Vec<u8>> {
	let key = recipient.kem_key()?;
	let mut m = [0u8; 32];
	getrandom::fill(&mut m).expect("the operating system RNG is available; qed");
	let (kem_ciphertext, shared) = key.encapsulate_deterministic(&Array::from(m));

	let mut plaintext = Vec::with_capacity(PLAINTEXT_SIZE);
	plaintext.extend_from_slice(&note.value.to_le_bytes());
	plaintext.extend_from_slice(&note.rho.to_bytes());
	for r in note.r {
		plaintext.extend_from_slice(&r.to_le_bytes());
	}

	// Each key encrypts a single message: the zero nonce is never reused.
	let cipher = ChaCha20Poly1305::new(&symmetric_key(&shared.into()));
	let sealed = cipher.encrypt(&Nonce::default(), plaintext.as_slice()).ok()?;

	let mut out = Vec::with_capacity(ENCRYPTED_NOTE_SIZE);
	out.extend_from_slice(kem_ciphertext.as_slice());
	out.extend_from_slice(&sealed);
	Some(out)
}

/// Try to decrypt an encrypted note with `key`. The note is returned only if its commitment is
/// `commitment`.
pub fn decrypt(encrypted: &[u8], commitment: &Digest, key: &SpendingKey) -> Option<Note> {
	if encrypted.len() != ENCRYPTED_NOTE_SIZE {
		return None;
	}
	let (kem_ciphertext, sealed) = encrypted.split_at(KEM_CIPHERTEXT_SIZE);
	let shared = decapsulate(&key.decapsulation_key(), kem_ciphertext)?;
	let cipher = ChaCha20Poly1305::new(&symmetric_key(&shared));
	let plaintext = cipher.decrypt(&Nonce::default(), sealed).ok()?;

	let value = u64::from_le_bytes(plaintext[..8].try_into().ok()?);
	let rho_bytes: [u8; 32] = plaintext[8..40].try_into().ok()?;
	let rho = Digest::from_canonical(core::array::from_fn(|i| {
		u64::from_le_bytes(rho_bytes[i * 8..i * 8 + 8].try_into().expect("8 bytes"))
	}))?;
	let r = core::array::from_fn(|i| {
		u64::from_le_bytes(plaintext[40 + i * 8..48 + i * 8].try_into().expect("8 bytes"))
	});

	let note = Note { value, tag: key.tag(), rho, r };
	(note.is_well_formed() && note.commitment() == *commitment).then_some(note)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::note::random_randomness;

	fn note_for(key: &SpendingKey) -> Note {
		Note { value: 42, tag: key.tag(), rho: Digest([1, 2, 3, 4]), r: random_randomness() }
	}

	#[test]
	fn recipient_decrypts() {
		let key = SpendingKey::generate();
		let note = note_for(&key);
		let encrypted = encrypt(&note, &key.address()).unwrap();
		assert_eq!(encrypted.len(), ENCRYPTED_NOTE_SIZE);
		assert_eq!(decrypt(&encrypted, &note.commitment(), &key), Some(note));
	}

	#[test]
	fn others_cannot_decrypt() {
		let key = SpendingKey::generate();
		let note = note_for(&key);
		let encrypted = encrypt(&note, &key.address()).unwrap();
		assert_eq!(decrypt(&encrypted, &note.commitment(), &SpendingKey::generate()), None);
	}

	#[test]
	fn commitment_is_checked() {
		let key = SpendingKey::generate();
		let note = note_for(&key);
		let encrypted = encrypt(&note, &key.address()).unwrap();
		assert_eq!(decrypt(&encrypted, &Digest([9, 9, 9, 9]), &key), None);
	}

	#[test]
	fn tampering_is_detected() {
		let key = SpendingKey::generate();
		let note = note_for(&key);
		let mut encrypted = encrypt(&note, &key.address()).unwrap();
		*encrypted.last_mut().unwrap() ^= 1;
		assert_eq!(decrypt(&encrypted, &note.commitment(), &key), None);
	}

	#[test]
	fn ciphertexts_are_randomized() {
		let key = SpendingKey::generate();
		let note = note_for(&key);
		assert_ne!(encrypt(&note, &key.address()), encrypt(&note, &key.address()));
	}
}
