//! RandomTESS key schedule.
//!
//! The key changes every [`SEED_EPOCH_BLOCKS`] blocks and is the hash of a block buried at least
//! [`SEED_EPOCH_LAG`] blocks deep, so that miners can prepare the next dataset in advance and a
//! short reorganization never changes the key.

use sp_blockchain::{HeaderBackend, HeaderMetadata};
use sp_runtime::{
	traits::{Block as BlockT, NumberFor},
	SaturatedConversion,
};

/// Number of blocks sharing the same key.
pub const SEED_EPOCH_BLOCKS: u64 = 2048;

/// Number of blocks between the key block and the first block using it.
pub const SEED_EPOCH_LAG: u64 = 64;

/// Height of the block whose hash is the key of the block at `height`.
pub fn seed_height(height: u64) -> u64 {
	if height <= SEED_EPOCH_BLOCKS + SEED_EPOCH_LAG {
		0
	} else {
		(height - SEED_EPOCH_LAG - 1) & !(SEED_EPOCH_BLOCKS - 1)
	}
}

/// Key of the block built on top of `parent`.
pub fn seed_hash<B, C>(client: &C, parent: B::Hash) -> sp_blockchain::Result<B::Hash>
where
	B: BlockT,
	C: HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error> + ?Sized,
{
	let parent_meta = client.header_metadata(parent)?;
	let parent_number: u64 = parent_meta.number.saturated_into();
	let target: NumberFor<B> = seed_height(parent_number + 1).saturated_into();

	// Blocks extending the best chain resolve the key from the number index.
	if client.hash(parent_meta.number)? == Some(parent) {
		if let Some(hash) = client.hash(target)? {
			return Ok(hash);
		}
	}

	let mut current = parent_meta;
	while current.number > target {
		current = client.header_metadata(current.parent)?;
	}
	Ok(current.hash)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn first_epoch_uses_genesis() {
		for height in 0..=(SEED_EPOCH_BLOCKS + SEED_EPOCH_LAG) {
			assert_eq!(seed_height(height), 0);
		}
	}

	#[test]
	fn key_changes_after_lag() {
		let first = SEED_EPOCH_BLOCKS + SEED_EPOCH_LAG + 1;
		assert_eq!(seed_height(first), SEED_EPOCH_BLOCKS);
		assert_eq!(seed_height(first + SEED_EPOCH_BLOCKS - 1), SEED_EPOCH_BLOCKS);
		assert_eq!(seed_height(first + SEED_EPOCH_BLOCKS), 2 * SEED_EPOCH_BLOCKS);
	}

	#[test]
	fn key_block_is_buried() {
		for height in (SEED_EPOCH_BLOCKS..20 * SEED_EPOCH_BLOCKS).step_by(7) {
			let seed = seed_height(height);
			assert!(seed == 0 || height - seed > SEED_EPOCH_LAG);
			assert_eq!(seed % SEED_EPOCH_BLOCKS, 0);
		}
	}
}
