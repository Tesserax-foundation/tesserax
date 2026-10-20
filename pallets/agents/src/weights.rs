//! Weights of the agents pallet.
//!
//! Measured with `tessera benchmark pallet --steps 10 --repeat 20` on the compiled WebAssembly
//! runtime (Apple M1). Must be measured again on the reference hardware before launch.

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

/// Weight functions of the pallet.
pub trait WeightInfo {
	/// Creating or replacing a delegation with `t` targets.
	fn authorize(t: u32) -> Weight;
	/// Removing a delegation.
	fn revoke() -> Weight;
	/// Checking and recording a call made by an agent, on top of the call itself.
	fn execute() -> Weight;
}

/// Weights measured with the runtime benchmarks.
pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `Agents::Delegations`, `System::Account`, `Balances::Holds` (r:1 w:1 each)
	fn authorize(t: u32) -> Weight {
		Weight::from_parts(53_450_122, 4665)
			.saturating_add(Weight::from_parts(11_418, 0).saturating_mul(t.into()))
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Storage: `Agents::Delegations`, `System::Account`, `Balances::Holds` (r:1 w:1 each)
	fn revoke() -> Weight {
		Weight::from_parts(47_000_000, 4665)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Measured with a transfer as the call, also counted in the weight of the call: an upper
	/// bound. Storage: `Agents::Delegations` (r:1 w:1), `System::Account` (r:2 w:2)
	fn execute() -> Weight {
		Weight::from_parts(61_000_000, 6196)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}
}

impl WeightInfo for () {
	fn authorize(t: u32) -> Weight {
		Weight::from_parts(53_450_122, 4665)
			.saturating_add(Weight::from_parts(11_418, 0).saturating_mul(t.into()))
	}

	fn revoke() -> Weight {
		Weight::from_parts(47_000_000, 4665)
	}

	fn execute() -> Weight {
		Weight::from_parts(61_000_000, 6196)
	}
}
