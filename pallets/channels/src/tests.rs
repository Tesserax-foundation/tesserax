use crate::{
	mock::*, voucher_message, Channels as ChannelStore, Error, Event, HoldReason, VOUCHER_CONTEXT,
};
use frame_support::{assert_noop, assert_ok, traits::fungible::InspectHold};
use tesserax_crypto::{Pair, Public, Signature};

fn signer() -> Pair {
	<Pair as sp_core::Pair>::from_seed(&[7u8; 32])
}

fn public(pair: &Pair) -> Public {
	sp_core::Pair::public(pair)
}

/// A voucher of cumulative `amount` on channel `id`, signed by `pair`.
fn voucher(pair: &Pair, id: u64, amount: u128) -> Signature {
	let genesis = System::block_hash(0);
	pair.sign_with_context(&voucher_message(&genesis, id, &amount), VOUCHER_CONTEXT)
}

fn held() -> u128 {
	Balances::balance_on_hold(&HoldReason::Channel.into(), &PAYER)
}

fn open(deposit: u128) -> u64 {
	let id = crate::NextChannelId::<Test>::get();
	assert_ok!(Channels::open(RuntimeOrigin::signed(PAYER), PAYEE, public(&signer()), deposit));
	id
}

#[test]
fn opening_holds_the_deposit() {
	new_test_ext().execute_with(|| {
		let id = open(1_000);
		assert_eq!(id, 0);
		assert_eq!(held(), 1_000 + STORAGE_DEPOSIT);
		assert_eq!(Balances::free_balance(PAYER), 10_000 - 1_000 - STORAGE_DEPOSIT);
		System::assert_last_event(Event::Opened { id, payer: PAYER, payee: PAYEE, deposit: 1_000 }.into());
		assert_eq!(open(500), 1);

		assert_noop!(
			Channels::open(RuntimeOrigin::signed(PAYER), PAYER, public(&signer()), 1_000),
			Error::<Test>::SelfChannel
		);
		assert_noop!(
			Channels::open(RuntimeOrigin::signed(PAYER), PAYEE, public(&signer()), MIN_DEPOSIT - 1),
			Error::<Test>::DepositTooLow
		);
	});
}

#[test]
fn payee_claims_cumulative_vouchers() {
	new_test_ext().execute_with(|| {
		let id = open(1_000);
		let key = signer();
		// Many payments off chain; the payee claims the latest voucher only.
		assert_ok!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 250, voucher(&key, id, 250)));
		assert_eq!(Balances::free_balance(PAYEE), 10 + 250);
		System::assert_last_event(Event::Claimed { id, claimed: 250, paid: 250 }.into());

		assert_ok!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 400, voucher(&key, id, 400)));
		assert_eq!(Balances::free_balance(PAYEE), 10 + 400);
		assert_eq!(held(), 1_000 - 400 + STORAGE_DEPOSIT);

		// An older voucher is worth nothing more.
		assert_noop!(
			Channels::claim(RuntimeOrigin::signed(PAYEE), id, 250, voucher(&key, id, 250)),
			Error::<Test>::NothingToClaim
		);
		assert_noop!(
			Channels::claim(RuntimeOrigin::signed(PAYEE), id, 1_001, voucher(&key, id, 1_001)),
			Error::<Test>::ExceedsDeposit
		);
		assert_noop!(
			Channels::claim(RuntimeOrigin::signed(OTHER), id, 500, voucher(&key, id, 500)),
			Error::<Test>::NotPayee
		);
	});
}

#[test]
fn forged_vouchers_are_rejected() {
	new_test_ext().execute_with(|| {
		let id = open(1_000);
		let other = open(1_000);
		let key = signer();
		// Another key, another amount, another channel, another chain.
		let stranger = <Pair as sp_core::Pair>::from_seed(&[8u8; 32]);
		assert_noop!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 300, voucher(&stranger, id, 300)), Error::<Test>::InvalidVoucher);
		assert_noop!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 300, voucher(&key, id, 200)), Error::<Test>::InvalidVoucher);
		assert_noop!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 300, voucher(&key, other, 300)), Error::<Test>::InvalidVoucher);
		let elsewhere = key.sign_with_context(&voucher_message(&[9u8; 32], id, &300u128), VOUCHER_CONTEXT);
		assert_noop!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 300, elsewhere), Error::<Test>::InvalidVoucher);
		// A transaction signature is not a voucher.
		let transaction = sp_core::Pair::sign(&key, &voucher_message(&System::block_hash(0), id, &300u128));
		assert_noop!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 300, transaction), Error::<Test>::InvalidVoucher);
	});
}

#[test]
fn payer_tops_up() {
	new_test_ext().execute_with(|| {
		let id = open(100);
		assert_noop!(Channels::top_up(RuntimeOrigin::signed(OTHER), id, 50), Error::<Test>::NotPayer);
		assert_ok!(Channels::top_up(RuntimeOrigin::signed(PAYER), id, 900));
		assert_eq!(ChannelStore::<Test>::get(id).unwrap().deposit, 1_000);
		assert_ok!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 800, voucher(&signer(), id, 800)));
	});
}

#[test]
fn payee_closes_and_the_rest_returns_to_the_payer() {
	new_test_ext().execute_with(|| {
		let id = open(1_000);
		assert_ok!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 300, voucher(&signer(), id, 300)));
		assert_noop!(Channels::close(RuntimeOrigin::signed(PAYER), id), Error::<Test>::NotPayee);
		assert_ok!(Channels::close(RuntimeOrigin::signed(PAYEE), id));
		System::assert_last_event(Event::Closed { id, refunded: 700 }.into());
		assert_eq!(held(), 0);
		assert_eq!(Balances::free_balance(PAYER), 10_000 - 300);
		assert!(ChannelStore::<Test>::get(id).is_none());
	});
}

#[test]
fn payer_closes_after_the_close_period() {
	new_test_ext().execute_with(|| {
		let id = open(1_000);
		assert_noop!(Channels::request_close(RuntimeOrigin::signed(PAYEE), id), Error::<Test>::NotPayer);
		assert_ok!(Channels::request_close(RuntimeOrigin::signed(PAYER), id));
		System::assert_last_event(Event::CloseRequested { id, closes_at: 1 + CLOSE_PERIOD }.into());
		assert_noop!(Channels::top_up(RuntimeOrigin::signed(PAYER), id, 10), Error::<Test>::Closing);
		assert_noop!(Channels::finalize_close(RuntimeOrigin::signed(PAYER), id), Error::<Test>::NotClosable);

		// The payee still claims during the close period.
		System::set_block_number(CLOSE_PERIOD);
		assert_ok!(Channels::claim(RuntimeOrigin::signed(PAYEE), id, 600, voucher(&signer(), id, 600)));

		System::set_block_number(1 + CLOSE_PERIOD);
		assert_ok!(Channels::finalize_close(RuntimeOrigin::signed(OTHER), id));
		assert_eq!(Balances::free_balance(PAYER), 10_000 - 600);
		assert_eq!(held(), 0);
		assert_noop!(
			Channels::claim(RuntimeOrigin::signed(PAYEE), id, 700, voucher(&signer(), id, 700)),
			Error::<Test>::UnknownChannel
		);
	});
}

#[test]
fn channels_that_are_not_closing_cannot_be_finalized() {
	new_test_ext().execute_with(|| {
		let id = open(1_000);
		System::set_block_number(1_000);
		assert_noop!(Channels::finalize_close(RuntimeOrigin::signed(PAYER), id), Error::<Test>::NotClosable);
	});
}

/// Voucher used by the benchmarks: key of seed `[7; 32]`, genesis hash `[1; 32]`, channel 0,
/// 1 TESS. Regenerate with `cargo test -p pallet-channels benchmark_fixture -- --ignored`.
#[test]
#[ignore]
fn benchmark_fixture() {
	use codec::Encode;
	let key = signer();
	let signature = key.sign_with_context(
		&voucher_message(&sp_core::H256([1u8; 32]), 0, &1_000_000_000_000u128),
		VOUCHER_CONTEXT,
	);
	let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/fixtures/voucher.scale");
	std::fs::write(path, (public(&key), signature).encode()).unwrap();
}
