//! ML-DSA transaction signatures against the full runtime.

use super::*;

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
