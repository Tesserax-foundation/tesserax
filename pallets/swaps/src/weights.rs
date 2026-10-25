//! Weights of `pallet_swaps`: estimated from the measures of `pallet_channels`, whose calls read
//! and write as much, until `scripts/benchmark.sh` measures them on the reference hardware.

#![allow(missing_docs)]

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::{constants::RocksDbWeight, Weight}};

/// Weight functions of `pallet_swaps`.
pub trait WeightInfo {
	fn offer() -> Weight;
	fn update_offer() -> Weight;
	fn cancel_offer() -> Weight;
	fn fill() -> Weight;
	fn authorize_claim() -> Weight;
	fn claim() -> Weight;
	fn refund() -> Weight;
}

/// Weights for the runtime, with its database weights.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	fn offer() -> Weight {
		Weight::from_parts(60_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(4))
	}
	fn update_offer() -> Weight {
		Weight::from_parts(60_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}
	fn cancel_offer() -> Weight {
		Weight::from_parts(55_000_000, 4_000)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}
	fn fill() -> Weight {
		Weight::from_parts(50_000_000, 4_500)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(4))
	}
	fn authorize_claim() -> Weight {
		Weight::from_parts(20_000_000, 3_000).saturating_add(T::DbWeight::get().reads(2))
	}
	fn claim() -> Weight {
		Weight::from_parts(80_000_000, 6_500)
			.saturating_add(T::DbWeight::get().reads(6))
			.saturating_add(T::DbWeight::get().writes(6))
	}
	fn refund() -> Weight {
		Weight::from_parts(60_000_000, 6_500)
			.saturating_add(T::DbWeight::get().reads(4))
			.saturating_add(T::DbWeight::get().writes(4))
	}
}

impl WeightInfo for () {
	fn offer() -> Weight {
		Weight::from_parts(60_000_000, 4_000).saturating_add(RocksDbWeight::get().reads_writes(3, 4))
	}
	fn update_offer() -> Weight {
		Weight::from_parts(60_000_000, 4_000).saturating_add(RocksDbWeight::get().reads_writes(3, 3))
	}
	fn cancel_offer() -> Weight {
		Weight::from_parts(55_000_000, 4_000).saturating_add(RocksDbWeight::get().reads_writes(3, 3))
	}
	fn fill() -> Weight {
		Weight::from_parts(50_000_000, 4_500).saturating_add(RocksDbWeight::get().reads_writes(3, 4))
	}
	fn authorize_claim() -> Weight {
		Weight::from_parts(20_000_000, 3_000).saturating_add(RocksDbWeight::get().reads(2))
	}
	fn claim() -> Weight {
		Weight::from_parts(80_000_000, 6_500).saturating_add(RocksDbWeight::get().reads_writes(6, 6))
	}
	fn refund() -> Weight {
		Weight::from_parts(60_000_000, 6_500).saturating_add(RocksDbWeight::get().reads_writes(4, 4))
	}
}
