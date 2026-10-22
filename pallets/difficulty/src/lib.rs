//! # Difficulty pallet
//!
//! Computes the proof-of-work difficulty of the next block with a linearly weighted moving
//! average (LWMA) of the last `Window` solve times. Recent blocks weigh more than older ones,
//! which lets the difficulty follow sudden hashrate changes within a few blocks while staying
//! stable under normal conditions.
//!
//! The difficulty stored in [`CurrentDifficulty`] is the one the next block must satisfy. The node
//! reads it through `sp_consensus_pow::DifficultyApi` at the parent block.
//!
//! Solve times are capped at six target block times so that a single block with a timestamp far
//! in the future cannot crash the difficulty. Block timestamps are strictly increasing and
//! bounded in the future by `pallet_timestamp`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;
pub use weights::WeightInfo;

use sp_core::U256;

/// Proof-of-work difficulty: the expected number of hashes needed to find a block.
pub type Difficulty = U256;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

/// Maximum ratio between a solve time taken into account and the target block time.
const MAX_SOLVE_TIME_RATIO: u64 = 6;

/// Bound on how much the difficulty can grow at once, relative to the window average.
const MAX_INCREASE_RATIO: u64 = 10;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{BuildGenesisConfig, Time},
	};
	use frame_system::pallet_prelude::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Source of block timestamps, in milliseconds.
		type Time: Time<Moment = u64>;

		/// Target block time, in milliseconds.
		#[pallet::constant]
		type TargetBlockTime: Get<u64>;

		/// Number of solve times averaged.
		#[pallet::constant]
		type Window: Get<u32>;

		/// Lower bound of the difficulty.
		#[pallet::constant]
		type MinimumDifficulty: Get<Difficulty>;

		/// Weights of the adjustment.
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Difficulty the next block must satisfy.
	#[pallet::storage]
	pub type CurrentDifficulty<T: Config> = StorageValue<_, Difficulty, ValueQuery>;

	/// Timestamp and difficulty of the most recent blocks, oldest first. Holds at most
	/// `Window + 1` entries.
	#[pallet::storage]
	pub type History<T: Config> =
		StorageValue<_, BoundedVec<(u64, Difficulty), WindowPlusOne<T>>, ValueQuery>;

	pub struct WindowPlusOne<T>(PhantomData<T>);
	impl<T: Config> Get<u32> for WindowPlusOne<T> {
		fn get() -> u32 {
			T::Window::get().saturating_add(1)
		}
	}

	#[pallet::genesis_config]
	pub struct GenesisConfig<T: Config> {
		/// Difficulty of the first block.
		pub initial_difficulty: Difficulty,
		#[serde(skip)]
		pub _config: PhantomData<T>,
	}

	impl<T: Config> Default for GenesisConfig<T> {
		fn default() -> Self {
			Self { initial_difficulty: T::MinimumDifficulty::get(), _config: PhantomData }
		}
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			assert!(
				self.initial_difficulty >= T::MinimumDifficulty::get(),
				"initial difficulty is below the minimum difficulty",
			);
			CurrentDifficulty::<T>::put(self.initial_difficulty);
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn on_initialize(_n: BlockNumberFor<T>) -> Weight {
			// Accounts for `on_finalize`.
			T::WeightInfo::on_finalize()
		}

		fn on_finalize(_n: BlockNumberFor<T>) {
			let now = T::Time::now();
			let difficulty = CurrentDifficulty::<T>::get();

			let mut history = History::<T>::get().into_inner();
			history.push((now, difficulty));
			let excess = history.len().saturating_sub(WindowPlusOne::<T>::get() as usize);
			history.drain(..excess);

			let next = next_difficulty(
				&history,
				T::TargetBlockTime::get(),
				T::MinimumDifficulty::get(),
			)
			.unwrap_or(difficulty);

			CurrentDifficulty::<T>::put(next);
			History::<T>::put(BoundedVec::truncate_from(history));
		}

		fn integrity_test() {
			assert!(T::TargetBlockTime::get() > 0, "target block time must not be zero");
			assert!(T::Window::get() >= 2, "difficulty window must hold at least two blocks");
			assert!(!T::MinimumDifficulty::get().is_zero(), "minimum difficulty must not be zero");
		}
	}

	impl<T: Config> Pallet<T> {
		/// Difficulty the next block must satisfy.
		pub fn difficulty() -> Difficulty {
			CurrentDifficulty::<T>::get()
		}
	}
}

/// LWMA difficulty for the block following `history`.
///
/// `history` lists `(timestamp, difficulty)` of consecutive blocks, oldest first. Returns `None`
/// when there are not yet two blocks to measure a solve time.
pub fn next_difficulty(
	history: &[(u64, Difficulty)],
	target: u64,
	minimum: Difficulty,
) -> Option<Difficulty> {
	let k = history.len().checked_sub(1).filter(|k| *k > 0)? as u64;
	let max_solve_time = target.saturating_mul(MAX_SOLVE_TIME_RATIO);

	let mut weighted_solve_times = U256::zero();
	let mut sum_difficulties = U256::zero();
	for (i, pair) in history.windows(2).enumerate() {
		let solve_time = pair[1].0.saturating_sub(pair[0].0).clamp(1, max_solve_time);
		weighted_solve_times += U256::from(solve_time) * U256::from(i as u64 + 1);
		sum_difficulties = sum_difficulties.saturating_add(pair[1].1);
	}

	// With every solve time equal to `target`, the weighted sum is `target * k * (k + 1) / 2` and
	// the result is the average difficulty.
	let expected = U256::from(target) * U256::from(k) * U256::from(k + 1) / 2;
	let floor = expected / MAX_INCREASE_RATIO;
	let weighted_solve_times = weighted_solve_times.max(floor).max(U256::one());

	let next = sum_difficulties
		.saturating_mul(U256::from(target))
		.saturating_mul(U256::from(k + 1))
		/ (weighted_solve_times * 2);

	Some(next.max(minimum))
}
