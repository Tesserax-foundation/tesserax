//! Shielded pool primitives for Tesserax.
//!
//! Notes are committed with Poseidon2 over the Goldilocks field, spent by publishing nullifiers,
//! and encrypted for their recipient with ML-KEM-768. See `docs/privacy.md`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "std")]
pub mod encryption;
pub mod hash;
pub mod keys;
pub mod note;
pub mod proof;
pub mod tree;

pub use hash::Digest;
pub use note::{Note, MAX_VALUE};

/// Size of an ML-KEM-768 ciphertext.
pub const KEM_CIPHERTEXT_SIZE: usize = 1088;

/// Size of an encrypted note plaintext: value, rho and randomness.
pub const PLAINTEXT_SIZE: usize = 8 + 32 + 8 * note::RANDOMNESS_LEN;

/// Size of an encrypted note: KEM ciphertext, then the sealed plaintext and its 16-byte tag.
pub const ENCRYPTED_NOTE_SIZE: usize = KEM_CIPHERTEXT_SIZE + PLAINTEXT_SIZE + 16;

/// An encrypted note, as published on chain.
pub type EncryptedNote = [u8; ENCRYPTED_NOTE_SIZE];
