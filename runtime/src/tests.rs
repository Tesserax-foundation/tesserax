//! Transaction signature checks against the full runtime.

use crate::*;
use codec::Encode;
use frame_support::traits::fungible::Inspect;
use sp_runtime::{
	generic::Era,
	traits::Header as _,
	transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidityError},
	ApplyExtrinsicResult, BuildStorage,
};
use tessera_crypto::{keyring::Keyring, Pair};

fn new_test_ext() -> sp_io::TestExternalities {
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

fn tx_extension(nonce: Nonce) -> TxExtension {
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

fn transfer_call(dest: &Keyring, value: Balance) -> RuntimeCall {
	RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
		dest: dest.to_account_id().into(),
		value,
	})
}

/// A transaction from `sender`, signed by `signer`.
fn signed(call: RuntimeCall, sender: AccountId, signer: &Pair, nonce: Nonce) -> UncheckedExtrinsic {
	let extension = tx_extension(nonce);
	let payload = SignedPayload::new(call.clone(), extension.clone()).unwrap();
	let signature = payload.using_encoded(|bytes| signer.sign_account(bytes));
	UncheckedExtrinsic::new_signed(call, sender.into(), signature, extension)
}

fn apply(xt: UncheckedExtrinsic) -> ApplyExtrinsicResult {
	Executive::apply_extrinsic(xt)
}

const BAD_PROOF: TransactionValidityError =
	TransactionValidityError::Invalid(InvalidTransaction::BadProof);

#[test]
fn ml_dsa_transfer_is_applied() {
	new_test_ext().execute_with(|| {
		let alice = Keyring::Alice;
		let xt = signed(transfer_call(&Keyring::Bob, 10 * UNIT), alice.to_account_id(), &alice.pair(), 0);
		assert_eq!(apply(xt), Ok(Ok(())));
		assert_eq!(Balances::balance(&Keyring::Bob.to_account_id()), 1_010 * UNIT);
		assert_eq!(System::account_nonce(alice.to_account_id()), 1);
	});
}

#[test]
fn signature_of_another_key_is_rejected() {
	new_test_ext().execute_with(|| {
		// Bob signs a transaction that claims to come from Alice.
		let xt = signed(
			transfer_call(&Keyring::Bob, 10 * UNIT),
			Keyring::Alice.to_account_id(),
			&Keyring::Bob.pair(),
			0,
		);
		assert_eq!(apply(xt), Err(BAD_PROOF));
		assert_eq!(Balances::balance(&Keyring::Bob.to_account_id()), 1_000 * UNIT);
	});
}

#[test]
fn tampered_signature_is_rejected() {
	new_test_ext().execute_with(|| {
		let alice = Keyring::Alice;
		let mut xt =
			signed(transfer_call(&Keyring::Bob, 10 * UNIT), alice.to_account_id(), &alice.pair(), 0);
		if let sp_runtime::generic::Preamble::Signed(_, ref mut signature, _) = xt.preamble {
			signature.signature.0[0] ^= 1;
		}
		assert_eq!(apply(xt), Err(BAD_PROOF));
	});
}

#[test]
fn tampered_call_is_rejected() {
	new_test_ext().execute_with(|| {
		let alice = Keyring::Alice;
		let mut xt =
			signed(transfer_call(&Keyring::Bob, 10 * UNIT), alice.to_account_id(), &alice.pair(), 0);
		xt.function = transfer_call(&Keyring::Bob, 999 * UNIT);
		assert_eq!(apply(xt), Err(BAD_PROOF));
	});
}

#[test]
fn transaction_pool_rejects_bad_signatures() {
	new_test_ext().execute_with(|| {
		let xt = signed(
			transfer_call(&Keyring::Bob, 10 * UNIT),
			Keyring::Alice.to_account_id(),
			&Keyring::Charlie.pair(),
			0,
		);
		let validity =
			Executive::validate_transaction(TransactionSource::External, xt, System::block_hash(0));
		assert_eq!(validity, Err(BAD_PROOF));
	});
}
