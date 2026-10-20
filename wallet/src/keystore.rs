//! Encrypted keystore: one file per account in the wallet directory.
//!
//! The secret URI of an account (its 24-word phrase) is encrypted with ChaCha20-Poly1305 under a
//! 256-bit key derived from the password with Argon2id. The address is authenticated with the
//! ciphertext, and checked against the decrypted key: a file cannot be edited to show another
//! address. Symmetric encryption with 256-bit keys keeps its security against quantum computers.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::{
	aead::{Aead, KeyInit, Payload},
	ChaCha20Poly1305, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use std::{
	fs,
	path::{Path, PathBuf},
};
use tesserax_sdk::Keypair;

/// Argon2id memory cost, in KiB: 64 MiB.
const MEMORY_KIB: u32 = 64 * 1024;
/// Argon2id passes.
const ITERATIONS: u32 = 3;
/// Argon2id lanes.
const PARALLELISM: u32 = 1;

/// Why a keystore operation failed.
#[derive(Debug, thiserror::Error)]
pub enum KeystoreError {
	#[error("no account named {0}")]
	NotFound(String),
	#[error("an account named {0} already exists")]
	Exists(String),
	#[error("invalid account name {0}: use letters, digits, '-' and '_'")]
	InvalidName(String),
	#[error("wrong password")]
	WrongPassword,
	#[error("the keystore file of {0} is corrupted")]
	Corrupted(String),
	#[error("{0}")]
	Io(#[from] std::io::Error),
	#[error("{0}")]
	Key(String),
}

#[derive(Serialize, Deserialize)]
struct Kdf {
	algorithm: String,
	memory_kib: u32,
	iterations: u32,
	parallelism: u32,
	salt: String,
}

#[derive(Serialize, Deserialize)]
struct Cipher {
	algorithm: String,
	nonce: String,
	ciphertext: String,
}

/// An account file.
#[derive(Serialize, Deserialize)]
struct AccountFile {
	version: u32,
	name: String,
	address: String,
	kdf: Kdf,
	cipher: Cipher,
}

/// An account of the wallet, without its secret.
#[derive(Clone, Debug, Serialize)]
pub struct Account {
	pub name: String,
	pub address: String,
}

/// The accounts of a wallet directory.
pub struct Keystore {
	dir: PathBuf,
}

fn random<const N: usize>() -> [u8; N] {
	let mut bytes = [0u8; N];
	getrandom::fill(&mut bytes).expect("the operating system RNG is available; qed");
	bytes
}

fn derive_key(password: &str, salt: &[u8], memory_kib: u32, iterations: u32, parallelism: u32) -> Result<Key, KeystoreError> {
	let params = Params::new(memory_kib, iterations, parallelism, Some(32))
		.map_err(|e| KeystoreError::Key(e.to_string()))?;
	let mut key = [0u8; 32];
	Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
		.hash_password_into(password.as_bytes(), salt, &mut key)
		.map_err(|e| KeystoreError::Key(e.to_string()))?;
	Ok(Key::from(key))
}

fn valid_name(name: &str) -> bool {
	!name.is_empty() &&
		name.len() <= 64 &&
		name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
	use std::os::unix::fs::PermissionsExt;
	fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
	Ok(())
}

impl Keystore {
	/// The keystore of `dir`, created readable by the user only.
	pub fn open(dir: impl Into<PathBuf>) -> Result<Self, KeystoreError> {
		let dir = dir.into();
		fs::create_dir_all(&dir)?;
		restrict(&dir, 0o700)?;
		Ok(Keystore { dir })
	}

	fn path(&self, name: &str) -> Result<PathBuf, KeystoreError> {
		if !valid_name(name) {
			return Err(KeystoreError::InvalidName(name.into()));
		}
		Ok(self.dir.join(format!("{name}.json")))
	}

	/// Encrypt `secret` (a secret URI) under `password` and store it as `name`.
	pub fn add(&self, name: &str, secret: &str, password: &str) -> Result<Account, KeystoreError> {
		let path = self.path(name)?;
		if path.exists() {
			return Err(KeystoreError::Exists(name.into()));
		}
		let key = Keypair::from_uri(secret).map_err(|e| KeystoreError::Key(e.to_string()))?;
		let address = key.address();

		let salt = random::<16>();
		let nonce = random::<12>();
		let cipher = ChaCha20Poly1305::new(&derive_key(password, &salt, MEMORY_KIB, ITERATIONS, PARALLELISM)?);
		let ciphertext = cipher
			.encrypt(&Nonce::from(nonce), Payload { msg: secret.trim().as_bytes(), aad: address.as_bytes() })
			.map_err(|_| KeystoreError::Key("encryption failed".into()))?;

		let file = AccountFile {
			version: 1,
			name: name.into(),
			address: address.clone(),
			kdf: Kdf {
				algorithm: "argon2id".into(),
				memory_kib: MEMORY_KIB,
				iterations: ITERATIONS,
				parallelism: PARALLELISM,
				salt: STANDARD.encode(salt),
			},
			cipher: Cipher {
				algorithm: "chacha20poly1305".into(),
				nonce: STANDARD.encode(nonce),
				ciphertext: STANDARD.encode(ciphertext),
			},
		};
		let json = serde_json::to_vec_pretty(&file).expect("an account file serializes; qed");
		// Written to a temporary file readable by the user only, then renamed.
		let temporary = self.dir.join(format!(".{name}.json.tmp"));
		fs::write(&temporary, json)?;
		restrict(&temporary, 0o600)?;
		fs::rename(&temporary, &path)?;
		Ok(Account { name: name.into(), address })
	}

	fn read(&self, name: &str) -> Result<AccountFile, KeystoreError> {
		let path = self.path(name)?;
		let bytes = fs::read(&path).map_err(|e| match e.kind() {
			std::io::ErrorKind::NotFound => KeystoreError::NotFound(name.into()),
			_ => e.into(),
		})?;
		let file: AccountFile = serde_json::from_slice(&bytes).map_err(|_| KeystoreError::Corrupted(name.into()))?;
		if file.version != 1 || file.kdf.algorithm != "argon2id" || file.cipher.algorithm != "chacha20poly1305" {
			return Err(KeystoreError::Corrupted(name.into()));
		}
		Ok(file)
	}

	/// The secret URI of `name`.
	pub fn secret(&self, name: &str, password: &str) -> Result<String, KeystoreError> {
		let file = self.read(name)?;
		let corrupted = || KeystoreError::Corrupted(name.into());
		let salt = STANDARD.decode(&file.kdf.salt).map_err(|_| corrupted())?;
		let nonce: [u8; 12] = STANDARD
			.decode(&file.cipher.nonce)
			.ok()
			.and_then(|n| n.try_into().ok())
			.ok_or_else(corrupted)?;
		let ciphertext = STANDARD.decode(&file.cipher.ciphertext).map_err(|_| corrupted())?;
		let key = derive_key(password, &salt, file.kdf.memory_kib, file.kdf.iterations, file.kdf.parallelism)?;
		let secret = ChaCha20Poly1305::new(&key)
			.decrypt(&Nonce::from(nonce), Payload { msg: &ciphertext, aad: file.address.as_bytes() })
			.map_err(|_| KeystoreError::WrongPassword)?;
		let secret = String::from_utf8(secret).map_err(|_| corrupted())?;
		let key = Keypair::from_uri(&secret).map_err(|_| corrupted())?;
		if key.address() != file.address {
			return Err(corrupted());
		}
		Ok(secret)
	}

	/// The key of `name`.
	pub fn keypair(&self, name: &str, password: &str) -> Result<Keypair, KeystoreError> {
		Keypair::from_uri(&self.secret(name, password)?).map_err(|e| KeystoreError::Key(e.to_string()))
	}

	/// The account `name`, without decrypting it.
	pub fn account(&self, name: &str) -> Result<Account, KeystoreError> {
		let file = self.read(name)?;
		Ok(Account { name: file.name, address: file.address })
	}

	/// The accounts, by name.
	pub fn list(&self) -> Result<Vec<Account>, KeystoreError> {
		let mut accounts = Vec::new();
		for entry in fs::read_dir(&self.dir)? {
			let path = entry?.path();
			let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".json")) else {
				continue;
			};
			if valid_name(name) {
				accounts.push(self.account(name)?);
			}
		}
		accounts.sort_by(|a, b| a.name.cmp(&b.name));
		Ok(accounts)
	}

	/// Delete the account `name`.
	pub fn remove(&self, name: &str) -> Result<(), KeystoreError> {
		self.read(name)?;
		fs::remove_file(self.path(name)?)?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn keystore() -> (Keystore, PathBuf) {
		let dir = std::env::temp_dir().join(format!("tesserax-keystore-{}", hex(&random::<8>())));
		(Keystore::open(&dir).unwrap(), dir)
	}

	fn hex(bytes: &[u8]) -> String {
		bytes.iter().map(|b| format!("{b:02x}")).collect()
	}

	#[test]
	fn accounts_roundtrip() {
		let (store, dir) = keystore();
		let (key, phrase) = Keypair::generate().unwrap();
		let account = store.add("agent", &phrase, "correct horse").unwrap();
		assert_eq!(account.address, key.address());
		assert_eq!(store.secret("agent", "correct horse").unwrap(), phrase);
		assert_eq!(store.keypair("agent", "correct horse").unwrap().address(), key.address());
		assert_eq!(store.list().unwrap().len(), 1);
		// The phrase is not stored in clear.
		let file = fs::read_to_string(dir.join("agent.json")).unwrap();
		assert!(!file.contains(&phrase));
		store.remove("agent").unwrap();
		assert!(matches!(store.account("agent"), Err(KeystoreError::NotFound(_))));
		fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn wrong_passwords_and_edits_are_detected() {
		let (store, dir) = keystore();
		store.add("main", "//Alice", "password").unwrap();
		assert!(matches!(store.secret("main", "Password"), Err(KeystoreError::WrongPassword)));

		// Showing another address breaks the authentication of the ciphertext.
		let path = dir.join("main.json");
		let mut file: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
		file["address"] = serde_json::Value::String(Keypair::from_uri("//Bob").unwrap().address());
		fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
		assert!(store.secret("main", "password").is_err());
		fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn names_and_duplicates() {
		let (store, dir) = keystore();
		store.add("a", "//Alice", "p").unwrap();
		assert!(matches!(store.add("a", "//Bob", "p"), Err(KeystoreError::Exists(_))));
		for bad in ["", "../escape", "a b", "a/b"] {
			assert!(matches!(store.add(bad, "//Bob", "p"), Err(KeystoreError::InvalidName(_))), "{bad}");
		}
		fs::remove_dir_all(dir).unwrap();
	}

	#[cfg(unix)]
	#[test]
	fn files_are_private() {
		use std::os::unix::fs::PermissionsExt;
		let (store, dir) = keystore();
		store.add("a", "//Alice", "p").unwrap();
		assert_eq!(fs::metadata(dir.join("a.json")).unwrap().permissions().mode() & 0o777, 0o600);
		assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
		fs::remove_dir_all(dir).unwrap();
	}
}
