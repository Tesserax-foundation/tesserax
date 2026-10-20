//! RandomTESS proof of work for the Tesserax node.
//!
//! The work input of a block is 43 bytes: the hash of the header without its seal (`pre_hash`),
//! followed by an 11-byte nonce. The last 4 bytes of the nonce, at offset 39, are the nonce a
//! miner such as XMRig iterates; the 7 bytes before them are an extra nonce that a stratum server
//! sets differently for each connection.
//!
//! As in RandomX v2, the proof-of-work value is the commitment `Blake2b(input || hash)` of the
//! RandomTESS hash of the input. A block is valid when that value, read as a little-endian 256-bit
//! integer, multiplied by the block difficulty does not overflow 256 bits, and when the hash
//! carried by the seal is the RandomTESS hash of the input under the key of the block (the hash
//! of a past block, see [`seed_height`]). The commitment is checked first: an invalid proof of
//! work is rejected with a single Blake2b, before the virtual machine runs.
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
pub mod stratum;

pub use confirmation::{finalize_at_depth, CONFIRMATION_DEPTH};
pub use miner::{start_miner, MinerConfig, MiningMode};
pub use seed::{seed_hash, seed_height, SEED_EPOCH_BLOCKS, SEED_EPOCH_LAG};

const LOG_TARGET: &str = "pow";

/// Proof-of-work difficulty: the expected number of hashes needed to find a block.
pub type Difficulty = U256;

/// Size of the nonce of a seal.
pub const NONCE_SIZE: usize = 11;

/// Size of the extra nonce, the part of the nonce set by a stratum server.
pub const EXTRA_NONCE_SIZE: usize = 7;

/// Offset in the work input of the 4-byte nonce iterated by miners.
pub const MINER_NONCE_OFFSET: usize = 32 + EXTRA_NONCE_SIZE;

/// Number of bytes hashed for each nonce.
pub const WORK_INPUT_SIZE: usize = 32 + NONCE_SIZE;

/// Content of the PoW seal digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
pub struct Seal {
	/// Extra nonce followed by the miner nonce.
	pub nonce: [u8; NONCE_SIZE],
	/// RandomTESS hash of the work input.
	pub hash: randomtess::Hash,
}

/// RandomTESS input for a header pre-hash and a nonce.
pub fn work_input(pre_hash: &[u8], nonce: &[u8; NONCE_SIZE]) -> [u8; WORK_INPUT_SIZE] {
	let mut input = [0u8; WORK_INPUT_SIZE];
	input[..32].copy_from_slice(&pre_hash[..32]);
	input[32..].copy_from_slice(nonce);
	input
}

/// The proof-of-work value of a work input and its RandomTESS hash.
pub fn pow_value(input: &[u8], hash: &randomtess::Hash) -> randomtess::Hash {
	randomtess::commitment(input, hash)
}

/// Whether a proof-of-work value satisfies `difficulty`.
pub fn meets_difficulty(value: &randomtess::Hash, difficulty: Difficulty) -> bool {
	if difficulty.is_zero() {
		return false;
	}
	let (_, overflow) = U256::from_little_endian(value).overflowing_mul(difficulty);
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
pub struct TesseraxPow<B, C> {
	client: Arc<C>,
	verifier: Arc<Mutex<Verifier>>,
	_marker: PhantomData<B>,
}

impl<B, C> Clone for TesseraxPow<B, C> {
	fn clone(&self) -> Self {
		Self { client: self.client.clone(), verifier: self.verifier.clone(), _marker: PhantomData }
	}
}

impl<B, C> TesseraxPow<B, C> {
	pub fn new(client: Arc<C>) -> Self {
		let verifier = Verifier { flags: Flags::recommended(), vms: Vec::new() };
		Self { client, verifier: Arc::new(Mutex::new(verifier)), _marker: PhantomData }
	}
}

impl<B, C> PowAlgorithm<B> for TesseraxPow<B, C>
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
		let input = work_input(pre_hash.as_ref(), &seal.nonce);
		if !meets_difficulty(&pow_value(&input, &seal.hash), difficulty) {
			return Ok(false);
		}

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
			.hash(key.as_ref(), &input)
			.map_err(|e| Error::Environment(e.to_string()))?;

		Ok(hash == seal.hash)
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
		let mut value = [0u8; 32];
		assert!(meets_difficulty(&value, U256::MAX));
		assert!(!meets_difficulty(&value, U256::zero()));

		// 2^254 passes difficulty 3, not 4.
		value[31] = 0x40;
		assert!(meets_difficulty(&value, U256::from(3)));
		assert!(!meets_difficulty(&value, U256::from(4)));

		value = [0xff; 32];
		assert!(meets_difficulty(&value, U256::one()));
		assert!(!meets_difficulty(&value, U256::from(2)));
	}

	#[test]
	fn work_input_layout() {
		let pre_hash = [7u8; 32];
		let nonce = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
		let input = work_input(&pre_hash, &nonce);
		assert_eq!(input.len(), 43);
		assert_eq!(&input[..32], &pre_hash);
		assert_eq!(&input[32..MINER_NONCE_OFFSET], &[1, 2, 3, 4, 5, 6, 7]);
		// Where XMRig writes its 32-bit nonce.
		assert_eq!(MINER_NONCE_OFFSET, 39);
		assert_eq!(&input[39..], &[8, 9, 10, 11]);
	}

	#[test]
	fn pow_value_is_the_commitment() {
		let input = work_input(&[7u8; 32], &[0; NONCE_SIZE]);
		let hash = [9u8; 32];
		let expected = <sp_runtime::traits::BlakeTwo256 as sp_runtime::traits::Hash>::hash(
			&[&input[..], &hash].concat(),
		);
		assert_eq!(pow_value(&input, &hash), expected.0);
	}

	#[test]
	fn seal_encoding() {
		let seal = Seal { nonce: [42; NONCE_SIZE], hash: [7; 32] };
		let encoded = seal.encode();
		assert_eq!(encoded.len(), NONCE_SIZE + 32);
		assert_eq!(Seal::decode_all(&mut &encoded[..]).unwrap(), seal);
		let mut trailing = encoded.clone();
		trailing.push(0);
		assert!(Seal::decode_all(&mut &trailing[..]).is_err());
	}
}
