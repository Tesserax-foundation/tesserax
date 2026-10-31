//! Atomic swaps: an offer, a fill by its maker, and the claim of the taker without signer nor fee;
//! a bid, a sale into it, and the claim of the buyer without signer nor fee.

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

fn claim_sale(sale: u64, secret: [u8; 32]) -> UncheckedExtrinsic {
	UncheckedExtrinsic::new_transaction(
		RuntimeCall::Swaps(pallet_swaps::Call::claim_sale { id: sale, secret }),
		tx_extension(0),
	)
}

#[test]
fn a_buyer_without_tess_claims_a_sale_with_the_secret_of_the_seller() {
	new_test_ext().execute_with(|| {
		let (alice, buyer) = (Keyring::Alice.to_account_id(), Keyring::Ferdie.to_account_id());
		// The buyer only needs the deposit of its bid.
		assert_ok!(Balances::transfer_keep_alive(RuntimeOrigin::signed(alice.clone()), buyer.clone().into(), UNIT));
		let usdc = Payment { chain: Chain::Base, asset: Asset::Usdc };
		assert_ok!(Swaps::bid(RuntimeOrigin::signed(buyer.clone()), usdc, 50_000, 100 * UNIT, UNIT));
		let free = Balances::balance(&buyer);
		let hash = sp_io::hashing::sha2_256(&SECRET);
		assert_ok!(Swaps::sell(RuntimeOrigin::signed(alice), 0, 10 * UNIT, hash, vec![0xab; 20].try_into().unwrap()));
		let sale = pallet_swaps::Sales::<Runtime>::get(0).unwrap();
		assert_eq!(sale.expires, System::block_number() + DAYS);
		assert_ok!(Swaps::pay(RuntimeOrigin::signed(buyer.clone()), 0, vec![1; 32].try_into().unwrap()));

		assert_eq!(
			apply(claim_sale(0, [0; 32])),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof))
		);
		let issuance = Balances::total_issuance();
		assert_eq!(apply(claim_sale(0, SECRET)), Ok(Ok(())));
		// The buyer has its TESS, and paid no fee for the claim.
		assert_eq!(Balances::balance(&buyer), free + 10 * UNIT);
		assert_eq!(Balances::total_issuance(), issuance);
		assert_eq!(apply(claim_sale(0, SECRET)), Err(TransactionValidityError::Invalid(InvalidTransaction::Stale)));
	});
}
