// Copyright 2026 Tesserax developers
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! ML-DSA-87 (FIPS 204) node identity keys.

use crate::{
    error::{Error, ParseError},
    PeerId,
};

use aws_lc_rs::{
    encoding::AsDer,
    signature::{KeyPair as _, PqdsaKeyPair, UnparsedPublicKey, ML_DSA_87, ML_DSA_87_SIGNING},
};
use zeroize::Zeroize;

use std::{fmt, sync::Arc};

/// Size of an encoded ML-DSA-87 public key.
pub const PUBLIC_KEY_LENGTH: usize = 2592;

/// Size of an ML-DSA-87 signature.
pub const SIGNATURE_LENGTH: usize = 4627;

/// Size of the seed an ML-DSA-87 key pair is generated from.
pub const SECRET_KEY_LENGTH: usize = 32;

/// An ML-DSA-87 key pair.
#[derive(Clone)]
pub struct Keypair {
    secret: SecretKey,
    inner: Arc<PqdsaKeyPair>,
    public: PublicKey,
}

impl Keypair {
    /// Generate a new random key pair.
    pub fn generate() -> Keypair {
        Keypair::from(SecretKey::generate())
    }

    /// Sign a message using the private key of this key pair.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let mut signature = vec![0u8; SIGNATURE_LENGTH];
        let len = self
            .inner
            .sign(msg, &mut signature)
            .expect("signature buffer has the ML-DSA-87 signature size; qed");
        signature.truncate(len);
        signature
    }

    /// Get the public key of this key pair.
    pub fn public(&self) -> PublicKey {
        self.public.clone()
    }

    /// Get the secret key (seed) of this key pair.
    pub fn secret(&self) -> SecretKey {
        self.secret.clone()
    }

    /// The private key in PKCS#8 v1 DER encoding (seed form).
    pub(crate) fn to_pkcs8_der(&self) -> Vec<u8> {
        self.inner
            .to_pkcs8v1()
            .expect("key pairs built from a seed can be serialized; qed")
            .as_ref()
            .to_vec()
    }

    /// The public key as a DER encoded `SubjectPublicKeyInfo`.
    pub(crate) fn public_key_der(&self) -> Vec<u8> {
        self.inner
            .public_key()
            .as_der()
            .expect("ML-DSA-87 public keys can be encoded; qed")
            .as_ref()
            .to_vec()
    }
}

impl fmt::Debug for Keypair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keypair").field("public", &self.public).finish_non_exhaustive()
    }
}

impl From<Keypair> for SecretKey {
    fn from(keypair: Keypair) -> SecretKey {
        keypair.secret
    }
}

impl From<SecretKey> for Keypair {
    fn from(secret: SecretKey) -> Keypair {
        let inner = PqdsaKeyPair::from_seed(&ML_DSA_87_SIGNING, &secret.0)
            .expect("seed has the ML-DSA-87 seed size; qed");
        let public = PublicKey(inner.public_key().as_ref().to_vec());
        debug_assert_eq!(public.0.len(), PUBLIC_KEY_LENGTH);
        Keypair {
            secret,
            inner: Arc::new(inner),
            public,
        }
    }
}

/// An ML-DSA-87 public key.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PublicKey(Vec<u8>);

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PublicKey(ml-dsa-87, ")?;
        for byte in &self.0[..8] {
            write!(f, "{byte:02x}")?;
        }
        f.write_str("…)")
    }
}

impl PublicKey {
    /// Verify an ML-DSA-87 signature for a message using this public key.
    #[must_use]
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        UnparsedPublicKey::new(&ML_DSA_87, &self.0).verify(msg, sig).is_ok()
    }

    /// Encoded public key.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    /// Encoded public key.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Decode a public key from its FIPS 204 encoding.
    pub fn try_from_bytes(bytes: &[u8]) -> Result<PublicKey, ParseError> {
        if bytes.len() != PUBLIC_KEY_LENGTH {
            return Err(ParseError::InvalidPublicKey);
        }
        Ok(PublicKey(bytes.to_vec()))
    }

    /// Decode a public key from a DER encoded `SubjectPublicKeyInfo`.
    ///
    /// The structure of an ML-DSA-87 `SubjectPublicKeyInfo` is fixed: a constant header followed
    /// by the encoded key. The header is checked byte for byte.
    pub fn try_from_spki_der(der: &[u8]) -> Result<PublicKey, ParseError> {
        let header = spki_header();
        if der.len() != header.len() + PUBLIC_KEY_LENGTH || &der[..header.len()] != header {
            return Err(ParseError::InvalidPublicKey);
        }
        Self::try_from_bytes(&der[header.len()..])
    }

    /// Convert the public key into a [`PeerId`].
    pub fn to_peer_id(&self) -> PeerId {
        crate::crypto::PublicKey::MlDsa(self.clone()).into()
    }
}

/// DER prefix of every ML-DSA-87 `SubjectPublicKeyInfo`.
fn spki_header() -> &'static [u8] {
    static HEADER: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    HEADER.get_or_init(|| {
        let der = Keypair::from(SecretKey([0u8; SECRET_KEY_LENGTH])).public_key_der();
        der[..der.len() - PUBLIC_KEY_LENGTH].to_vec()
    })
}

/// An ML-DSA-87 secret key: the 32 byte seed of FIPS 204 key generation.
#[derive(Clone)]
pub struct SecretKey([u8; SECRET_KEY_LENGTH]);

impl AsRef<[u8]> for SecretKey {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "SecretKey")
    }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl SecretKey {
    /// Generate a new random secret key.
    pub fn generate() -> SecretKey {
        let mut seed = [0u8; SECRET_KEY_LENGTH];
        aws_lc_rs::rand::fill(&mut seed).expect("the system random number generator works; qed");
        SecretKey(seed)
    }

    /// Create a secret key from a 32 byte seed. The input is zeroized.
    pub fn try_from_bytes(mut sk_bytes: impl AsMut<[u8]>) -> crate::Result<SecretKey> {
        let sk_bytes = sk_bytes.as_mut();
        let seed: [u8; SECRET_KEY_LENGTH] =
            sk_bytes.try_into().map_err(|_| Error::InvalidData)?;
        sk_bytes.zeroize();
        Ok(SecretKey(seed))
    }

    /// The 32 byte seed.
    pub fn to_bytes(&self) -> [u8; SECRET_KEY_LENGTH] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let keypair = Keypair::generate();
        let signature = keypair.sign(b"message");
        assert_eq!(signature.len(), SIGNATURE_LENGTH);
        assert!(keypair.public().verify(b"message", &signature));
        assert!(!keypair.public().verify(b"other", &signature));
        assert!(!Keypair::generate().public().verify(b"message", &signature));
    }

    #[test]
    fn deterministic_from_seed() {
        let seed = [7u8; SECRET_KEY_LENGTH];
        let a = Keypair::from(SecretKey::try_from_bytes(seed).unwrap());
        let b = Keypair::from(SecretKey::try_from_bytes(seed).unwrap());
        assert_eq!(a.public(), b.public());
        assert_eq!(a.public().as_bytes().len(), PUBLIC_KEY_LENGTH);
    }

    #[test]
    fn spki_roundtrip() {
        let keypair = Keypair::generate();
        let der = keypair.public_key_der();
        assert_eq!(PublicKey::try_from_spki_der(&der).unwrap(), keypair.public());

        let mut bad = der.clone();
        bad[2] ^= 1;
        assert!(PublicKey::try_from_spki_der(&bad).is_err());
        assert!(PublicKey::try_from_spki_der(&der[..der.len() - 1]).is_err());
    }

    #[test]
    fn peer_id_is_hashed() {
        let peer_id = Keypair::generate().public().to_peer_id();
        // Keys longer than 42 bytes are hashed with SHA-256.
        assert_eq!(peer_id.as_ref().code(), 0x12);
    }
}
