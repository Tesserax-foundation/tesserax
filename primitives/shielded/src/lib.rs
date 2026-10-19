//! Shielded pool primitives for Tessera.
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
pub mod tree;

pub use hash::Digest;
pub use note::{Note, MAX_VALUE};
