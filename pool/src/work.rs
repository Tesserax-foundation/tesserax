//! Proof of work as the pool sees it: work inputs, difficulties and the share targets of XMRig.
//!
//! These follow the node (`consensus/pow`): a proof-of-work value meets a difficulty when, read as
//! a little-endian 256-bit integer, its product with the difficulty does not overflow.

use primitive_types::U256;

/// Bytes hashed for each nonce: the header pre-hash and an 11-byte nonce.
pub const WORK_INPUT_SIZE: usize = 43;

/// Offset of the bytes of the extra nonce the pool sets for each rig.
pub const RESERVED_OFFSET: usize = 35;

/// Size of the bytes the pool sets for each rig.
pub const RESERVED_SIZE: usize = 4;

/// Offset of the 4-byte nonce a rig iterates.
pub const MINER_NONCE_OFFSET: usize = 39;

/// Whether the proof-of-work `value` meets `difficulty`.
pub fn meets(value: &[u8; 32], difficulty: U256) -> bool {
	!difficulty.is_zero() && !U256::from_little_endian(value).overflowing_mul(difficulty).1
}

/// The 64-bit target XMRig compares the last 8 bytes of a value with, for a share difficulty.
/// Every value meeting the difficulty passes the target; the pool checks the full 256 bits.
pub fn target(difficulty: u64) -> u64 {
	if difficulty <= 1 {
		return u64::MAX;
	}
	let top = (U256::MAX / U256::from(difficulty)) >> 192;
	top.low_u64().saturating_add(1)
}

/// The work input of a job blob with the extra nonce of a rig and its nonce.
pub fn work_input(blob: &[u8; WORK_INPUT_SIZE], reserved: [u8; RESERVED_SIZE], nonce: [u8; 4]) -> [u8; WORK_INPUT_SIZE] {
	let mut input = *blob;
	input[RESERVED_OFFSET..MINER_NONCE_OFFSET].copy_from_slice(&reserved);
	input[MINER_NONCE_OFFSET..].copy_from_slice(&nonce);
	input
}

/// Share difficulty of each rig, adjusted so that it submits a share every `share_seconds`.
#[derive(Clone, Copy, Debug)]
pub struct Vardiff {
	pub share_seconds: f64,
	pub min: u64,
	pub max: u64,
}

impl Vardiff {
	/// The difficulty after `shares` shares in `elapsed` seconds at `current`, or `None` to keep
	/// it. Each step changes it at most fourfold, and only when off by more than a third.
	pub fn retarget(&self, current: u64, shares: u32, elapsed: f64) -> Option<u64> {
		if elapsed <= 0.0 {
			return None;
		}
		let hashrate = current as f64 * f64::from(shares.max(1)) / elapsed;
		let ideal = (hashrate * self.share_seconds).clamp(current as f64 / 4.0, current as f64 * 4.0);
		let next = (ideal as u64).clamp(self.min, self.max.max(self.min));
		let ratio = next as f64 / current as f64;
		(!(0.75..=1.33).contains(&ratio)).then_some(next)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn targets_pass_every_share() {
		for difficulty in [2u64, 3, 1000, 123_456, u64::MAX] {
			let max = U256::MAX / U256::from(difficulty);
			let value = max.to_little_endian();
			assert!(meets(&value, U256::from(difficulty)));
			let last = u64::from_le_bytes(value[24..].try_into().unwrap());
			assert!(last < target(difficulty), "{difficulty}");
		}
		assert_eq!(target(1), u64::MAX);
		assert_eq!(target(2), 1 << 63);
	}

	#[test]
	fn values_meet_difficulties() {
		let mut value = [0u8; 32];
		assert!(meets(&value, U256::from(u64::MAX)));
		value[31] = 0x80;
		assert!(meets(&value, U256::one()));
		assert!(meets(&value, U256::from(1)));
		assert!(!meets(&value, U256::from(2)));
		assert!(!meets(&value, U256::zero()));
	}

	#[test]
	fn inputs_place_the_extra_nonce_and_the_nonce() {
		let blob = [0xaa; WORK_INPUT_SIZE];
		let input = work_input(&blob, [1, 2, 3, 4], [5, 6, 7, 8]);
		assert_eq!(&input[..35], &[0xaa; 35]);
		assert_eq!(&input[35..], &[1, 2, 3, 4, 5, 6, 7, 8]);
	}

	#[test]
	fn vardiff_follows_the_hashrate() {
		let vardiff = Vardiff { share_seconds: 15.0, min: 100, max: 1_000_000 };
		// 600 H/s at difficulty 1000: one share every 1.7 s, so about 9000 is ideal.
		assert_eq!(vardiff.retarget(1000, 18, 30.0), Some(4000));
		assert_eq!(vardiff.retarget(4000, 5, 30.0), Some(10_000));
		// On target: no change.
		assert_eq!(vardiff.retarget(9000, 2, 30.0), None);
		// Slow rig, floor.
		assert_eq!(vardiff.retarget(500, 1, 600.0), Some(125).map(|d: u64| d.max(100)));
		assert_eq!(vardiff.retarget(100, 1, 600.0), None);
		assert_eq!(vardiff.retarget(1000, 3, 0.0), None);
	}
}
