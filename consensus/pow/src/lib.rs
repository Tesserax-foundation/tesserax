//! RandomTESS proof of work for the Tessera node.
//!
//! A block is valid when `RandomTESS(key, pre_hash || nonce)`, read as a little-endian 256-bit
//! integer, multiplied by the block difficulty does not overflow 256 bits. `pre_hash` is the hash
//! of the header without its seal, `nonce` is the 64-bit little-endian nonce stored in the seal
//! and `key` is the hash of a past block, see [`seed_height`].
//!
//! The difficulty of each block is computed by the runtime (`pallet-difficulty`) and read through
//! [`sp_consensus_pow::DifficultyApi`].

use codec::{Decode, DecodeAll, Encode};
use parking_lot::Mutex;
use randomtess::{Cache, Flags, Vm};
use sc_consensus_pow::{Error, PowAlgorithm};
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{HeaderBackend, HeaderMetadata};
use sp_consensus_pow::DifficultyApi;
use sp_core::U256;
use sp_runtime::{generic::BlockId, traits::Block as BlockT};
use std::{marker::PhantomData, sync::Arc};

mod confirmation;
mod miner;
mod seed;

pub use confirmation::{finalize_at_depth, CONFIRMATION_DEPTH};
pub use miner::{start_miner, MinerConfig, MiningMode};
pub use seed::{seed_hash, seed_height, SEED_EPOCH_BLOCKS, SEED_EPOCH_LAG};

const LOG_TARGET: &str = "pow";

/// Proof-of-work difficulty: the expected number of hashes needed to find a block.
pub type Difficulty = U256;

/// Content of the PoW seal digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Seal {
	pub nonce: u64,
}

/// Number of bytes hashed for each nonce.
pub const WORK_INPUT_SIZE: usize = 40;

/// RandomTESS input for a header pre-hash and a nonce.
pub fn work_input(pre_hash: &[u8], nonce: u64) -> [u8; WORK_INPUT_SIZE] {
	let mut input = [0u8; WORK_INPUT_SIZE];
	input[..32].copy_from_slice(&pre_hash[..32]);
	input[32..].copy_from_slice(&nonce.to_le_bytes());
	input
}

/// Whether a RandomTESS hash satisfies `difficulty`.
pub fn hash_meets_difficulty(hash: &randomtess::Hash, difficulty: Difficulty) -> bool {
	if difficulty.is_zero() {
		return false;
	}
	let (_, overflow) = U256::from_little_endian(hash).overflowing_mul(difficulty);
	!overflow
}

/// Light mode virtual machines used for verification, most recently used first.
struct Verifier {
	flags: Flags,
	vms: Vec<Vm>,
}

impl Verifier {
	/// Two keys are live around an epoch change.
	const CAPACITY: usize = 2;

	fn hash(&mut self, key: &[u8], input: &[u8]) -> Result<randomtess::Hash, randomtess::Error> {
		match self.vms.iter().position(|vm| vm.key() == key) {
			Some(index) => {
				let vm = self.vms.remove(index);
				self.vms.insert(0, vm);
			},
			None => {
				log::debug!(target: LOG_TARGET, "Initializing RandomTESS cache for key {}", hex(key));
				let cache = Arc::new(Cache::new(self.flags, key)?);
				self.vms.insert(0, Vm::light(self.flags, cache)?);
				self.vms.truncate(Self::CAPACITY);
			},
		}
		Ok(self.vms[0].hash(input))
	}
}

/// The RandomTESS [`PowAlgorithm`].
pub struct TesseraPow<B, C> {
	client: Arc<C>,
	verifier: Arc<Mutex<Verifier>>,
	_marker: PhantomData<B>,
}

impl<B, C> Clone for TesseraPow<B, C> {
	fn clone(&self) -> Self {
		Self { client: self.client.clone(), verifier: self.verifier.clone(), _marker: PhantomData }
	}
}

impl<B, C> TesseraPow<B, C> {
	pub fn new(client: Arc<C>) -> Self {
		let verifier = Verifier { flags: Flags::recommended(), vms: Vec::new() };
		Self { client, verifier: Arc::new(Mutex::new(verifier)), _marker: PhantomData }
	}
}

impl<B, C> PowAlgorithm<B> for TesseraPow<B, C>
where
	B: BlockT,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
	C::Api: DifficultyApi<B, Difficulty>,
{
	type Difficulty = Difficulty;

	fn difficulty(&self, parent: B::Hash) -> Result<Difficulty, Error<B>> {
		self.client
			.runtime_api()
			.difficulty(parent)
			.map_err(|e| Error::Environment(format!("Fetching difficulty failed: {e}")))
	}

	fn verify(
		&self,
		parent: &BlockId<B>,
		pre_hash: &B::Hash,
		_pre_digest: Option<&[u8]>,
		seal: &sp_consensus_pow::Seal,
		difficulty: Difficulty,
	) -> Result<bool, Error<B>> {
		let Ok(seal) = Seal::decode_all(&mut &seal[..]) else { return Ok(false) };

		let parent = match parent {
			BlockId::Hash(hash) => *hash,
			BlockId::Number(number) => self
				.client
				.hash(*number)
				.map_err(Error::Client)?
				.ok_or_else(|| Error::Other(format!("Unknown parent block #{number}")))?,
		};
		let key = seed_hash::<B, _>(self.client.as_ref(), parent).map_err(Error::Client)?;

		let hash = self
			.verifier
			.lock()
			.hash(key.as_ref(), &work_input(pre_hash.as_ref(), seal.nonce))
			.map_err(|e| Error::Environment(e.to_string()))?;

		Ok(hash_meets_difficulty(&hash, difficulty))
	}
}

fn hex(bytes: &[u8]) -> String {
	bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn difficulty_threshold() {
		let mut hash = [0u8; 32];
		assert!(hash_meets_difficulty(&hash, U256::MAX));
		assert!(!hash_meets_difficulty(&hash, U256::zero()));

		// 2^254 passes difficulty 3, not 4.
		hash[31] = 0x40;
		assert!(hash_meets_difficulty(&hash, U256::from(3)));
		assert!(!hash_meets_difficulty(&hash, U256::from(4)));

		hash = [0xff; 32];
		assert!(hash_meets_difficulty(&hash, U256::one()));
		assert!(!hash_meets_difficulty(&hash, U256::from(2)));
	}

	#[test]
	fn work_input_layout() {
		let pre_hash = [7u8; 32];
		let input = work_input(&pre_hash, 0x0102030405060708);
		assert_eq!(&input[..32], &pre_hash);
		assert_eq!(&input[32..], &[8, 7, 6, 5, 4, 3, 2, 1]);
	}

	#[test]
	fn seal_encoding() {
		let seal = Seal { nonce: 42 };
		let encoded = seal.encode();
		assert_eq!(encoded.len(), 8);
		assert_eq!(Seal::decode_all(&mut &encoded[..]).unwrap(), seal);
		let mut trailing = encoded.clone();
		trailing.push(0);
		assert!(Seal::decode_all(&mut &trailing[..]).is_err());
	}
}
