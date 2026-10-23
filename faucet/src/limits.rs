//! Limits of the faucet: one claim per address and a few per IP address every 24 hours, and a cap
//! on what it gives in 24 hours. Claims are kept in a file, so that a restart resets nothing.

use serde::{Deserialize, Serialize};
use std::{
	collections::VecDeque,
	path::{Path, PathBuf},
};

/// The window of every limit, in seconds.
pub const WINDOW: u64 = 24 * 3600;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claim {
	pub address: String,
	pub ip: String,
	/// Seconds since the Unix epoch.
	pub at: u64,
	/// Planck.
	pub amount: u128,
}

pub struct Limits {
	pub ip_claims: usize,
	/// Planck given at most in the window.
	pub cap: u128,
	claims: VecDeque<Claim>,
	file: PathBuf,
}

/// Why a claim is refused, and in how many seconds it would be accepted.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
	Address(u64),
	Ip(u64),
	Cap(u64),
}

impl Limits {
	/// The limits, with the claims of `file` if it exists.
	pub fn open(file: &Path, ip_claims: usize, cap: u128) -> std::io::Result<Self> {
		let claims = match std::fs::read(file) {
			Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other)?,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => VecDeque::new(),
			Err(e) => return Err(e),
		};
		Ok(Self { ip_claims, cap, claims, file: file.to_path_buf() })
	}

	fn prune(&mut self, now: u64) {
		while self.claims.front().is_some_and(|c| c.at + WINDOW <= now) {
			self.claims.pop_front();
		}
	}

	/// Given in the window.
	pub fn given(&mut self, now: u64) -> u128 {
		self.prune(now);
		self.claims.iter().map(|c| c.amount).sum()
	}

	/// Records a claim of `amount` by `address` from `ip`, unless a limit refuses it.
	pub fn claim(&mut self, address: &str, ip: &str, amount: u128, now: u64) -> Result<(), Refusal> {
		self.prune(now);
		let wait = |c: &Claim| c.at + WINDOW - now;
		if let Some(previous) = self.claims.iter().find(|c| c.address == address) {
			return Err(Refusal::Address(wait(previous)));
		}
		let from_ip: Vec<&Claim> = self.claims.iter().filter(|c| c.ip == ip).collect();
		if from_ip.len() >= self.ip_claims {
			return Err(Refusal::Ip(wait(from_ip[from_ip.len() - self.ip_claims])));
		}
		let given: u128 = self.claims.iter().map(|c| c.amount).sum();
		if given + amount > self.cap {
			return Err(Refusal::Cap(self.claims.front().map_or(WINDOW, wait)));
		}
		self.claims.push_back(Claim { address: address.into(), ip: ip.into(), at: now, amount });
		Ok(())
	}

	/// Forgets the claim of `address` at `at`, whose transfer failed.
	pub fn cancel(&mut self, address: &str, at: u64) {
		self.claims.retain(|c| !(c.address == address && c.at == at));
	}

	/// Writes the claims to the file, atomically.
	pub fn save(&self) -> std::io::Result<()> {
		let partial = self.file.with_extension("partial");
		std::fs::write(&partial, serde_json::to_vec(&self.claims).map_err(std::io::Error::other)?)?;
		std::fs::rename(&partial, &self.file)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn limits(dir: &tempfile::TempDir) -> Limits {
		Limits::open(&dir.path().join("claims.json"), 2, 25).unwrap()
	}

	#[test]
	fn one_claim_per_address_and_a_few_per_ip() {
		let dir = tempfile::tempdir().unwrap();
		let mut limits = limits(&dir);
		assert_eq!(limits.claim("a", "1.1.1.1", 10, 1_000), Ok(()));
		assert_eq!(limits.claim("a", "2.2.2.2", 10, 1_100), Err(Refusal::Address(WINDOW - 100)));
		assert_eq!(limits.claim("b", "1.1.1.1", 10, 1_200), Ok(()));
		assert_eq!(limits.claim("c", "1.1.1.1", 1, 1_300), Err(Refusal::Ip(WINDOW - 300)));
		// The cap: 20 given of 25.
		assert_eq!(limits.claim("d", "3.3.3.3", 10, 1_400), Err(Refusal::Cap(WINDOW - 400)));
		assert_eq!(limits.given(1_400), 20);
		// A day later, everything is allowed again.
		assert_eq!(limits.claim("a", "1.1.1.1", 10, 1_000 + WINDOW), Ok(()));
	}

	#[test]
	fn claims_survive_a_restart_and_failed_ones_are_forgotten() {
		let dir = tempfile::tempdir().unwrap();
		let mut first = limits(&dir);
		first.claim("a", "1.1.1.1", 10, 1_000).unwrap();
		first.claim("b", "2.2.2.2", 10, 1_001).unwrap();
		first.cancel("b", 1_001);
		first.save().unwrap();
		let mut second = limits(&dir);
		assert!(matches!(second.claim("a", "3.3.3.3", 10, 2_000), Err(Refusal::Address(_))));
		assert_eq!(second.claim("b", "2.2.2.2", 10, 2_000), Ok(()));
	}
}
