//! Weights of the agent registry pallet.
//!
//! Measured with `tesserax benchmark pallet --steps 10 --repeat 20` on the compiled WebAssembly
//! runtime (Apple M1). Must be measured again on the reference hardware before launch.

use core::marker::PhantomData;
use frame_support::{traits::Get, weights::Weight};

/// Weight functions of the pallet.
pub trait WeightInfo {
	fn register() -> Weight;
	fn update_profile() -> Weight;
	fn set_owner() -> Weight;
	fn confirm_owner() -> Weight;
	fn set_active() -> Weight;
	fn deregister() -> Weight;
	fn pay() -> Weight;
	fn rate() -> Weight;
}

/// Weights measured with the runtime benchmarks.
pub struct SubstrateWeight<T>(PhantomData<T>);

impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Largest profile. Storage: `AgentRegistry::Agents`, `System::Account`, `Balances::Holds`
	fn register() -> Weight {
		Weight::from_parts(56_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Storage: `AgentRegistry::Agents`, `System::Account`, `Balances::Holds`
	fn update_profile() -> Weight {
		Weight::from_parts(60_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// Storage: `AgentRegistry::Agents`
	fn set_owner() -> Weight {
		Weight::from_parts(15_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}

	/// Storage: `AgentRegistry::Agents`
	fn confirm_owner() -> Weight {
		Weight::from_parts(15_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}

	/// Storage: `AgentRegistry::Agents`
	fn set_active() -> Weight {
		Weight::from_parts(15_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(1))
			.saturating_add(T::DbWeight::get().writes(1))
	}

	/// Storage: `AgentRegistry::Agents`, `System::Account`, `Balances::Holds`
	fn deregister() -> Weight {
		Weight::from_parts(49_000_000, 4968)
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}

	/// First payment of a payer. Storage: `AgentRegistry::Agents` (r:1), `System::Account` (r:2 w:2),
	/// `AgentRegistry::Payers` (r:1 w:1), `AgentRegistry::Reputations` (r:1 w:1)
	fn pay() -> Weight {
		Weight::from_parts(60_000_000, 6196)
			.saturating_add(T::DbWeight::get().reads(5))
			.saturating_add(T::DbWeight::get().writes(4))
	}

	/// Storage: `AgentRegistry::Payers`, `AgentRegistry::Reputations`
	fn rate() -> Weight {
		Weight::from_parts(16_000_000, 3569)
			.saturating_add(T::DbWeight::get().reads(2))
			.saturating_add(T::DbWeight::get().writes(2))
	}
}

impl WeightInfo for () {
	fn register() -> Weight {
		Weight::from_parts(56_000_000, 4968)
	}

	fn update_profile() -> Weight {
		Weight::from_parts(60_000_000, 4968)
	}

	fn set_owner() -> Weight {
		Weight::from_parts(15_000_000, 4968)
	}

	fn confirm_owner() -> Weight {
		Weight::from_parts(15_000_000, 4968)
	}

	fn set_active() -> Weight {
		Weight::from_parts(15_000_000, 4968)
	}

	fn deregister() -> Weight {
		Weight::from_parts(49_000_000, 4968)
	}

	fn pay() -> Weight {
		Weight::from_parts(60_000_000, 6196)
	}

	fn rate() -> Weight {
		Weight::from_parts(16_000_000, 3569)
	}
}
