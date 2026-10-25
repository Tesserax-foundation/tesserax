//! Migrations of a running chain. A new chain needs none: its genesis has the state of this
//! runtime.

use crate::{
	genesis_config_presets, governance::TechnicalCollective, Preimage, Referenda, Runtime, Scheduler,
	TechnicalCommittee,
};
use frame_support::{
	migrations::RemovePallet,
	parameter_types,
	traits::{GetStorageVersion, InitializeMembers, OnRuntimeUpgrade, PalletInfoAccess, StorageVersion},
	weights::Weight,
};
use sp_core::H256;

parameter_types! {
	pub const SudoPalletName: &'static str = "Sudo";
}

/// Genesis hash of the public testnet.
const TESTNET_GENESIS: H256 =
	H256(genesis_config_presets::bytes32("c2f9cb03897c5ad30fff2f93debaa8b4a6e08f37a12270e107c71eb1ad39e5d1"));

/// Migrations of this runtime, in order.
pub type Migrations = (RemovePallet<SudoPalletName, <Runtime as frame_system::Config>::DbWeight>, AddGovernance);

/// The governance pallets on a chain started with sudo, the public testnet: their storage
/// versions, and the members of its technical committee, the signatories of its foundation. The
/// sudo key itself goes with the pallet of sudo.
pub struct AddGovernance;

fn put_version<P: GetStorageVersion<InCodeStorageVersion = StorageVersion> + PalletInfoAccess>() -> bool {
	if P::on_chain_storage_version() == StorageVersion::new(0) && P::in_code_storage_version() != StorageVersion::new(0) {
		P::in_code_storage_version().put::<P>();
		true
	} else {
		false
	}
}

impl OnRuntimeUpgrade for AddGovernance {
	fn on_runtime_upgrade() -> Weight {
		let db = <Runtime as frame_system::Config>::DbWeight::get();
		let mut writes = 0u64;
		for put in [
			put_version::<Preimage>,
			put_version::<Scheduler>,
			put_version::<Referenda>,
			put_version::<TechnicalCommittee>,
		] {
			writes += u64::from(put());
		}
		let members = pallet_collective::Members::<Runtime, TechnicalCollective>::get();
		if members.is_empty() && frame_system::BlockHash::<Runtime>::get(0) == TESTNET_GENESIS {
			let mut members = genesis_config_presets::testnet_committee().to_vec();
			members.sort();
			<TechnicalCommittee as InitializeMembers<_>>::initialize_members(&members);
			writes += 1;
		}
		db.reads_writes(6, writes)
	}
}
