//! Runtime tests: ML-DSA transactions, smart contracts and private transfers.

mod contracts;
mod shielded;
mod signatures;

pub(super) use crate::*;
pub(super) use codec::Encode;
pub(super) use frame_support::traits::fungible::Inspect;
pub(super) use sp_runtime::{
	generic::Era,
	traits::Header as _,
	transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidityError},
	ApplyExtrinsicResult, BuildStorage,
};
pub(super) use tessera_crypto::{keyring::Keyring, Pair};

pub(super) fn new_test_ext() -> sp_io::TestExternalities {
	let storage = RuntimeGenesisConfig {
		balances: pallet_balances::GenesisConfig {
			balances: vec![
				(Keyring::Alice.to_account_id(), 1_000 * UNIT),
				(Keyring::Bob.to_account_id(), 1_000 * UNIT),
			],
			..Default::default()
		},
		..Default::default()
	}
	.build_storage()
	.unwrap();
	let mut ext = sp_io::TestExternalities::new(storage);
	ext.execute_with(|| {
		let header = Header::new(
			1,
			Default::default(),
			Default::default(),
			System::block_hash(0),
			Default::default(),
		);
		Executive::initialize_block(&header);
	});
	ext
}

pub(super) fn tx_extension(nonce: Nonce) -> TxExtension {
	(
		frame_system::AuthorizeCall::new(),
		frame_system::CheckNonZeroSender::new(),
		frame_system::CheckSpecVersion::new(),
		frame_system::CheckTxVersion::new(),
		frame_system::CheckGenesis::new(),
		frame_system::CheckEra::from(Era::Immortal),
		frame_system::CheckNonce::from(nonce),
		frame_system::CheckWeight::new(),
		pallet_transaction_payment::ChargeTransactionPayment::from(0),
		frame_metadata_hash_extension::CheckMetadataHash::new(false),
		frame_system::WeightReclaim::new(),
	)
}

pub(super) fn transfer_call(dest: &Keyring, value: Balance) -> RuntimeCall {
	RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
		dest: dest.to_account_id().into(),
		value,
	})
}

/// A transaction from `sender`, signed by `signer`.
pub(super) fn signed(call: RuntimeCall, sender: AccountId, signer: &Pair, nonce: Nonce) -> UncheckedExtrinsic {
	let extension = tx_extension(nonce);
	let payload = SignedPayload::new(call.clone(), extension.clone()).unwrap();
	let signature = payload.using_encoded(|bytes| signer.sign_account(bytes));
	UncheckedExtrinsic::new_signed(call, sender.into(), signature, extension)
}

pub(super) fn apply(xt: UncheckedExtrinsic) -> ApplyExtrinsicResult {
	Executive::apply_extrinsic(xt)
}

