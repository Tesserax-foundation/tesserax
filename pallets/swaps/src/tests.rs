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

const BUYER: u64 = OTHER;
const SELLER: u64 = MAKER;

/// A bid for 100,000 planck in sales of at least 10,000.
fn bid() -> BidId {
	assert_ok!(Swaps::bid(RuntimeOrigin::signed(BUYER), USDC_BASE, PRICE, 100_000, 10_000));
	NextBidId::<Test>::get() - 1
}

fn sell(bid: BidId, amount: u128) -> SaleId {
	assert_ok!(Swaps::sell(RuntimeOrigin::signed(SELLER), bid, amount, hash(), evm_address()));
	NextSaleId::<Test>::get() - 1
}

#[test]
fn bids_hold_only_a_deposit_and_check_what_they_ask() {
	new_test_ext().execute_with(|| {
		let id = bid();
		assert_eq!(held(BUYER), DEPOSIT);
		let bid = Bids::<Test>::get(id).unwrap();
		assert_eq!((bid.buyer, bid.wanted, bid.min_sale, bid.price), (BUYER, 100_000, 10_000, PRICE));

		let buyer = RuntimeOrigin::signed(BUYER);
		let btc_usdc = Payment { chain: Chain::Bitcoin, asset: Asset::Usdc };
		assert_noop!(Swaps::bid(buyer.clone(), btc_usdc, PRICE, 100_000, 0), Error::<Test>::UnsupportedPayment);
		assert_noop!(Swaps::bid(buyer.clone(), USDC_BASE, 0, 100_000, 0), Error::<Test>::InvalidPrice);
		assert_noop!(Swaps::bid(buyer.clone(), USDC_BASE, PRICE, MIN_OFFER - 1, 0), Error::<Test>::InvalidAmount);
		assert_noop!(Swaps::bid(buyer, USDC_BASE, PRICE, 100_000, 100_001), Error::<Test>::InvalidAmount);
	});
}

#[test]
fn buyers_update_and_cancel_their_bids() {
	new_test_ext().execute_with(|| {
		let id = bid();
		assert_noop!(Swaps::update_bid(RuntimeOrigin::signed(SELLER), id, PRICE, 0, 0), Error::<Test>::NotBuyer);
		assert_ok!(Swaps::update_bid(RuntimeOrigin::signed(BUYER), id, 2 * PRICE, 50_000, 5_000));
		let bid = Bids::<Test>::get(id).unwrap();
		assert_eq!((bid.price, bid.wanted, bid.min_sale), (2 * PRICE, 50_000, 5_000));
		assert_noop!(
			Swaps::update_bid(RuntimeOrigin::signed(BUYER), id, PRICE, 50_000, 50_001),
			Error::<Test>::InvalidAmount
		);

		assert_noop!(Swaps::cancel_bid(RuntimeOrigin::signed(SELLER), id), Error::<Test>::NotBuyer);
		assert_ok!(Swaps::cancel_bid(RuntimeOrigin::signed(BUYER), id));
		assert_eq!(held(BUYER), 0);
		assert_eq!(Balances::balance(&BUYER), 100);
		assert!(Bids::<Test>::get(id).is_none());
	});
}

#[test]
fn a_sale_is_paid_then_claimed_by_the_buyer_without_signer() {
	new_test_ext().execute_with(|| {
		let id = bid();
		let sale = sell(id, 20_000);
		assert_eq!(held(SELLER), 20_000);
		assert_eq!(Bids::<Test>::get(id).unwrap().wanted, 80_000);
		let recorded = Sales::<Test>::get(sale).unwrap();
		assert_eq!((recorded.seller, recorded.buyer, recorded.amount), (SELLER, BUYER, 20_000));
		assert_eq!(recorded.expires, 1 + SALE_TIMEOUT);
		assert!(recorded.payment_lock.is_empty());

		// The buyer records its payment once.
		assert_noop!(Swaps::pay(RuntimeOrigin::signed(SELLER), sale, lock(1)), Error::<Test>::NotBuyer);
		assert_ok!(Swaps::pay(RuntimeOrigin::signed(BUYER), sale, lock(1)));
		assert_eq!(Sales::<Test>::get(sale).unwrap().payment_lock, lock(1));
		assert_noop!(Swaps::pay(RuntimeOrigin::signed(BUYER), sale, lock(2)), Error::<Test>::AlreadyPaid);
		System::assert_last_event(Event::SalePaid { sale, payment_lock: lock(1) }.into());

		// Only the secret authorizes the claim.
		assert_eq!(
			Swaps::authorize_claim_sale(TransactionSource::External, &sale, &[8; 32]).unwrap_err(),
			InvalidTransaction::BadProof.into()
		);
		assert!(Swaps::authorize_claim_sale(TransactionSource::External, &sale, &SECRET).is_ok());
		assert_noop!(Swaps::claim_sale(RuntimeOrigin::signed(BUYER), sale, SECRET), DispatchError::BadOrigin);

		assert_ok!(Swaps::claim_sale(authorized(), sale, SECRET));
		// Its deposit stays held while the bid is open.
		assert_eq!(Balances::balance(&BUYER), 100 - DEPOSIT + 20_000);
		assert_eq!(held(SELLER), 0);
		assert_eq!(Balances::balance(&SELLER), 1_000_000 - 20_000);
		assert!(Sales::<Test>::get(sale).is_none());
		let bid = Bids::<Test>::get(id).unwrap();
		assert_eq!((bid.sales, bid.completed), (1, 1));
		System::assert_last_event(Event::SaleClaimed { sale, buyer: BUYER, amount: 20_000, secret: SECRET }.into());
		assert_eq!(
			Swaps::authorize_claim_sale(TransactionSource::External, &sale, &SECRET).unwrap_err(),
			InvalidTransaction::Stale.into()
		);
	});
}

#[test]
fn sales_follow_their_bid() {
	new_test_ext().execute_with(|| {
		let id = bid();
		let seller = RuntimeOrigin::signed(SELLER);
		assert_noop!(Swaps::sell(seller.clone(), 9, 20_000, hash(), evm_address()), Error::<Test>::UnknownBid);
		assert_noop!(Swaps::sell(seller.clone(), id, 5_000, hash(), evm_address()), Error::<Test>::BelowMinSale);
		assert_noop!(Swaps::sell(seller.clone(), id, 100_001, hash(), evm_address()), Error::<Test>::NotEnoughWanted);
		assert_noop!(Swaps::sell(seller.clone(), id, 0, hash(), evm_address()), Error::<Test>::InvalidAmount);
		// Paid on Base: an EVM address of 20 bytes.
		assert_noop!(
			Swaps::sell(seller.clone(), id, 20_000, hash(), vec![2; 33].try_into().unwrap()),
			Error::<Test>::InvalidPayout
		);
		assert_noop!(
			Swaps::sell(RuntimeOrigin::signed(BUYER), id, 20_000, hash(), evm_address()),
			Error::<Test>::SelfTrade
		);
		// A seller sells what it has.
		assert!(Swaps::sell(RuntimeOrigin::signed(TAKER), id, 20_000, hash(), evm_address()).is_err());

		// The last TESS wanted sell below the smallest sale.
		sell(id, 95_000);
		sell(id, 5_000);
		assert_noop!(Swaps::sell(seller, id, 1, hash(), evm_address()), Error::<Test>::NotEnoughWanted);
	});
}

#[test]
fn a_payment_lock_pays_one_sale_before_it_expires() {
	new_test_ext().execute_with(|| {
		let id = bid();
		let first = sell(id, 20_000);
		let second = sell(id, 20_000);
		assert_ok!(Swaps::pay(RuntimeOrigin::signed(BUYER), first, lock(1)));
		assert_noop!(Swaps::pay(RuntimeOrigin::signed(BUYER), second, lock(1)), Error::<Test>::AlreadyPaid);
		assert_noop!(Swaps::pay(RuntimeOrigin::signed(BUYER), second, Default::default()), Error::<Test>::InvalidAmount);

		System::set_block_number(1 + SALE_TIMEOUT + 1);
		assert_noop!(Swaps::pay(RuntimeOrigin::signed(BUYER), second, lock(2)), Error::<Test>::Expired);
	});
}

#[test]
fn expired_sales_return_to_the_seller_and_the_bid() {
	new_test_ext().execute_with(|| {
		let id = bid();
		let first = sell(id, 20_000);
		let second = sell(id, 30_000);
		assert_eq!(Bids::<Test>::get(id).unwrap().wanted, 50_000);
		assert_noop!(Swaps::refund_sale(RuntimeOrigin::signed(BUYER), first), Error::<Test>::NotExpired);

		System::set_block_number(1 + SALE_TIMEOUT + 1);
		assert_noop!(Swaps::claim_sale(authorized(), first, SECRET), Error::<Test>::Expired);
		assert_eq!(
			Swaps::authorize_claim_sale(TransactionSource::External, &first, &SECRET).unwrap_err(),
			InvalidTransaction::Stale.into()
		);
		// Back to the seller, and wanted again by the open bid.
		assert_ok!(Swaps::refund_sale(RuntimeOrigin::signed(BUYER), first));
		assert_eq!(held(SELLER), 30_000);
		assert_eq!(Bids::<Test>::get(id).unwrap().wanted, 70_000);
		System::assert_last_event(Event::SaleRefunded { sale: first, seller: SELLER, amount: 20_000 }.into());

		// Once the bid is cancelled, only to the seller.
		assert_ok!(Swaps::cancel_bid(RuntimeOrigin::signed(BUYER), id));
		assert_ok!(Swaps::refund_sale(RuntimeOrigin::signed(BUYER), second));
		assert_eq!(held(SELLER), 0);
		assert_eq!(Balances::balance(&SELLER), 1_000_000);
	});
}
