use crate::{mock::*, *};
use frame_support::{
	assert_noop, assert_ok,
	traits::{fungible::InspectHold, fungible::Inspect as _},
};
use sp_runtime::{
	transaction_validity::{InvalidTransaction, TransactionSource},
	DispatchError,
};

const USDC_BASE: Payment = Payment { chain: Chain::Base, asset: Asset::Usdc };
/// 0.05 USDC, 6 decimals, for one TESS.
const PRICE: u128 = 50_000;
const SECRET: [u8; 32] = [7; 32];

fn hash() -> [u8; 32] {
	sp_io::hashing::sha2_256(&SECRET)
}

fn evm_address() -> Payout {
	vec![0xab; 20].try_into().unwrap()
}

fn lock(n: u8) -> PaymentLock {
	vec![n; 32].try_into().unwrap()
}

fn held(who: u64) -> u128 {
	Balances::balance_on_hold(&HoldReason::Swap.into(), &who)
}

fn authorized() -> RuntimeOrigin {
	frame_system::RawOrigin::Authorized.into()
}

/// An offer of 100,000 planck in trades of at least 10,000.
fn offer() -> OfferId {
	assert_ok!(Swaps::offer(RuntimeOrigin::signed(MAKER), USDC_BASE, PRICE, 100_000, 10_000, evm_address()));
	NextOfferId::<Test>::get() - 1
}

#[test]
fn payments_round_up() {
	// 1.5 TESS at 0.05 USDC: 75,000 units of USDC.
	assert_eq!(payment_due(1_500_000_000_000, PRICE), Some(75_000));
	// A planck still costs a unit.
	assert_eq!(payment_due(1, PRICE), Some(1));
	assert_eq!(payment_due(u128::MAX, 2), None);
}

#[test]
fn offers_hold_their_tess_and_check_what_they_ask() {
	new_test_ext().execute_with(|| {
		let id = offer();
		assert_eq!(held(MAKER), 100_000 + DEPOSIT);
		assert_eq!(Offers::<Test>::get(id).unwrap().available, 100_000);

		let maker = RuntimeOrigin::signed(MAKER);
		let btc_usdc = Payment { chain: Chain::Bitcoin, asset: Asset::Usdc };
		assert_noop!(Swaps::offer(maker.clone(), btc_usdc, PRICE, 100_000, 0, evm_address()), Error::<Test>::UnsupportedPayment);
		let base_usdt = Payment { chain: Chain::Base, asset: Asset::Usdt };
		assert_noop!(Swaps::offer(maker.clone(), base_usdt, PRICE, 100_000, 0, evm_address()), Error::<Test>::UnsupportedPayment);
		// An EVM address is 20 bytes, a Bitcoin public key 33.
		let btc = Payment { chain: Chain::Bitcoin, asset: Asset::Native };
		assert_noop!(Swaps::offer(maker.clone(), btc, PRICE, 100_000, 0, evm_address()), Error::<Test>::InvalidPayout);
		assert_ok!(Swaps::offer(maker.clone(), btc, PRICE, 100_000, 0, vec![2; 33].try_into().unwrap()));
		assert_noop!(Swaps::offer(maker.clone(), USDC_BASE, 0, 100_000, 0, evm_address()), Error::<Test>::InvalidPrice);
		assert_noop!(Swaps::offer(maker.clone(), USDC_BASE, PRICE, MIN_OFFER - 1, 0, evm_address()), Error::<Test>::InvalidAmount);
		assert_noop!(Swaps::offer(maker, USDC_BASE, PRICE, 100_000, 100_001, evm_address()), Error::<Test>::InvalidAmount);
	});
}

#[test]
fn makers_update_and_cancel_their_offers() {
	new_test_ext().execute_with(|| {
		let id = offer();
		assert_noop!(Swaps::update_offer(RuntimeOrigin::signed(OTHER), id, PRICE, 0, 0, 0), Error::<Test>::NotMaker);
		assert_ok!(Swaps::update_offer(RuntimeOrigin::signed(MAKER), id, 2 * PRICE, 5_000, 50_000, 20_000));
		let offer = Offers::<Test>::get(id).unwrap();
		assert_eq!((offer.price, offer.available, offer.min_fill), (2 * PRICE, 130_000, 5_000));
		assert_eq!(held(MAKER), 130_000 + DEPOSIT);

		assert_ok!(Swaps::cancel_offer(RuntimeOrigin::signed(MAKER), id));
		assert_eq!(held(MAKER), 0);
		assert_eq!(Balances::balance(&MAKER), 1_000_000);
		assert!(Offers::<Test>::get(id).is_none());
	});
}

#[test]
fn a_trade_is_claimed_with_its_secret_without_signer() {
	new_test_ext().execute_with(|| {
		let id = offer();
		assert_noop!(
			Swaps::fill(RuntimeOrigin::signed(OTHER), id, TAKER, 20_000, hash(), lock(1)),
			Error::<Test>::NotMaker
		);
		assert_noop!(
			Swaps::fill(RuntimeOrigin::signed(MAKER), id, TAKER, 5_000, hash(), lock(1)),
			Error::<Test>::BelowMinFill
		);
		assert_ok!(Swaps::fill(RuntimeOrigin::signed(MAKER), id, TAKER, 20_000, hash(), lock(1)));
		assert_eq!(Offers::<Test>::get(id).unwrap().available, 80_000);
		// A payment fills one trade.
		assert_noop!(
			Swaps::fill(RuntimeOrigin::signed(MAKER), id, TAKER, 20_000, hash(), lock(1)),
			Error::<Test>::LockAlreadyFilled
		);

		// Only the secret authorizes the claim.
		assert!(Swaps::authorize_claim(TransactionSource::External, &0, &[8; 32]).is_err());
		assert_eq!(
			Swaps::authorize_claim(TransactionSource::External, &0, &[8; 32]).unwrap_err(),
			InvalidTransaction::BadProof.into()
		);
		assert!(Swaps::authorize_claim(TransactionSource::External, &0, &SECRET).is_ok());
		assert_noop!(Swaps::claim(RuntimeOrigin::signed(TAKER), 0, SECRET), DispatchError::BadOrigin);

		assert_ok!(Swaps::claim(authorized(), 0, SECRET));
		assert_eq!(Balances::balance(&TAKER), 1 + 20_000);
		assert_eq!(held(MAKER), 80_000 + DEPOSIT);
		assert!(Trades::<Test>::get(0).is_none());
		let offer = Offers::<Test>::get(id).unwrap();
		assert_eq!((offer.fills, offer.completed), (1, 1));
		System::assert_last_event(Event::Claimed { trade: 0, taker: TAKER, amount: 20_000, secret: SECRET }.into());
		// Claimed once.
		assert_eq!(
			Swaps::authorize_claim(TransactionSource::External, &0, &SECRET).unwrap_err(),
			InvalidTransaction::Stale.into()
		);
	});
}

#[test]
fn the_last_tess_of_an_offer_fill_below_the_minimum() {
	new_test_ext().execute_with(|| {
		let id = offer();
		assert_ok!(Swaps::fill(RuntimeOrigin::signed(MAKER), id, TAKER, 95_000, hash(), lock(1)));
		assert_ok!(Swaps::fill(RuntimeOrigin::signed(MAKER), id, OTHER, 5_000, hash(), lock(2)));
		assert_noop!(
			Swaps::fill(RuntimeOrigin::signed(MAKER), id, OTHER, 1, hash(), lock(3)),
			Error::<Test>::NotEnoughOffered
		);
	});
}

#[test]
fn expired_trades_return_to_the_offer_or_the_maker() {
	new_test_ext().execute_with(|| {
		let id = offer();
		assert_ok!(Swaps::fill(RuntimeOrigin::signed(MAKER), id, TAKER, 20_000, hash(), lock(1)));
		assert_ok!(Swaps::fill(RuntimeOrigin::signed(MAKER), id, TAKER, 30_000, hash(), lock(2)));
		assert_noop!(Swaps::refund(RuntimeOrigin::signed(OTHER), 0), Error::<Test>::NotExpired);

		System::set_block_number(1 + TIMEOUT + 1);
		assert_noop!(Swaps::claim(authorized(), 0, SECRET), Error::<Test>::Expired);
		assert_eq!(
			Swaps::authorize_claim(TransactionSource::External, &0, &SECRET).unwrap_err(),
			InvalidTransaction::Stale.into()
		);
		// Back into the open offer.
		assert_ok!(Swaps::refund(RuntimeOrigin::signed(OTHER), 0));
		assert_eq!(Offers::<Test>::get(id).unwrap().available, 70_000);

		// Back to the maker once the offer is cancelled.
		assert_ok!(Swaps::cancel_offer(RuntimeOrigin::signed(MAKER), id));
		assert_eq!(held(MAKER), 30_000);
		assert_ok!(Swaps::refund(RuntimeOrigin::signed(OTHER), 1));
		assert_eq!(held(MAKER), 0);
		assert_eq!(Balances::balance(&MAKER), 1_000_000);
	});
}

#[test]
fn makers_do_not_trade_with_themselves() {
	new_test_ext().execute_with(|| {
		let id = offer();
		assert_noop!(
			Swaps::fill(RuntimeOrigin::signed(MAKER), id, MAKER, 20_000, hash(), lock(1)),
			Error::<Test>::SelfTrade
		);
	});
}
