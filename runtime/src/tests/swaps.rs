//! Atomic swaps: an offer, a fill by its maker, and the claim of the taker without signer nor fee.

use super::*;
use frame_support::assert_ok;
use pallet_swaps::{Asset, Chain, Payment};

const SECRET: [u8; 32] = [42; 32];

fn claim(trade: u64, secret: [u8; 32]) -> UncheckedExtrinsic {
	UncheckedExtrinsic::new_transaction(RuntimeCall::Swaps(pallet_swaps::Call::claim { id: trade, secret }), tx_extension(0))
}

#[test]
fn a_taker_without_tess_claims_with_its_secret() {
	new_test_ext().execute_with(|| {
		let (alice, taker) = (Keyring::Alice.to_account_id(), Keyring::Ferdie.to_account_id());
		assert_eq!(Balances::balance(&taker), 0);
		let usdc = Payment { chain: Chain::Base, asset: Asset::Usdc };
		assert_ok!(Swaps::offer(
			RuntimeOrigin::signed(alice.clone()),
			usdc,
			50_000,
			100 * UNIT,
			UNIT,
			vec![0xab; 20].try_into().unwrap()
		));
		let hash = sp_io::hashing::sha2_256(&SECRET);
		assert_ok!(Swaps::fill(
			RuntimeOrigin::signed(alice),
			0,
			taker.clone(),
			10 * UNIT,
			hash,
			vec![1; 32].try_into().unwrap()
		));

		// A wrong secret never enters a block.
		assert_eq!(
			apply(claim(0, [0; 32])),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof))
		);
		let issuance = Balances::total_issuance();
		assert_eq!(apply(claim(0, SECRET)), Ok(Ok(())));
		// The taker has its TESS, and paid no fee.
		assert_eq!(Balances::balance(&taker), 10 * UNIT);
		assert_eq!(Balances::total_issuance(), issuance);
		assert_eq!(apply(claim(0, SECRET)), Err(TransactionValidityError::Invalid(InvalidTransaction::Stale)));
	});
}
