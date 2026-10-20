use crate::{mock::*, Agents, Error, Event, Payers, Profile, ReputationRecorder, Reputations};
use codec::Encode;
use frame_support::{assert_noop, assert_ok, traits::fungible::InspectHold};

const CUSTOMER: u64 = SHOP;

fn profile(name: &str, capabilities: &[&str]) -> Profile {
	Profile {
		name: name.as_bytes().to_vec().try_into().unwrap(),
		description: b"Translates documents".to_vec().try_into().unwrap(),
		endpoint: b"https://agent.example/mcp".to_vec().try_into().unwrap(),
		capabilities: capabilities
			.iter()
			.map(|c| c.as_bytes().to_vec().try_into().unwrap())
			.collect::<Vec<_>>()
			.try_into()
			.unwrap(),
		metadata: Some([7; 32]),
	}
}

fn held(who: u64) -> u64 {
	Balances::balance_on_hold(&crate::HoldReason::Registration.into(), &who)
}

fn register() {
	assert_ok!(Registry::register(
		RuntimeOrigin::signed(AGENT),
		profile("translator", &["translation", "fr-en"]),
		Some(OWNER)
	));
}

#[test]
fn agents_register_with_a_deposit() {
	new_test_ext().execute_with(|| {
		register();
		let p = profile("translator", &["translation", "fr-en"]);
		let info = Agents::<Test>::get(AGENT).unwrap();
		assert_eq!(info.profile, p);
		assert!(info.active && !info.owner_confirmed);
		assert!(info.profile.offers(b"fr-en"));
		assert_eq!(held(AGENT), DEPOSIT_BASE + p.encoded_size() as u64);
		System::assert_last_event(Event::Registered { agent: AGENT, owner: Some(OWNER) }.into());

		assert_noop!(
			Registry::register(RuntimeOrigin::signed(AGENT), p.clone(), None),
			Error::<Test>::AlreadyRegistered
		);
		assert_noop!(
			Registry::register(RuntimeOrigin::signed(OTHER), p, Some(OTHER)),
			Error::<Test>::SelfOwned
		);
	});
}

#[test]
fn profiles_are_validated() {
	new_test_ext().execute_with(|| {
		for bad in [profile("", &[]), profile("a", &["Upper"]), profile("a", &[""]), profile("a", &["a b"])] {
			assert_noop!(
				Registry::register(RuntimeOrigin::signed(AGENT), bad, None),
				Error::<Test>::InvalidProfile
			);
		}
		let mut p = profile("a", &[]);
		p.endpoint = b"https://a b".to_vec().try_into().unwrap();
		assert_noop!(Registry::register(RuntimeOrigin::signed(AGENT), p, None), Error::<Test>::InvalidProfile);
	});
}

#[test]
fn updating_the_profile_adjusts_the_deposit() {
	new_test_ext().execute_with(|| {
		register();
		let longer = profile("translator", &["translation", "fr-en", "de-en", "summaries"]);
		assert_ok!(Registry::update_profile(RuntimeOrigin::signed(AGENT), longer.clone()));
		assert_eq!(held(AGENT), DEPOSIT_BASE + longer.encoded_size() as u64);
		let shorter = profile("t", &[]);
		assert_ok!(Registry::update_profile(RuntimeOrigin::signed(AGENT), shorter.clone()));
		assert_eq!(held(AGENT), DEPOSIT_BASE + shorter.encoded_size() as u64);
		assert_noop!(
			Registry::update_profile(RuntimeOrigin::signed(OTHER), shorter),
			Error::<Test>::NotRegistered
		);
	});
}

#[test]
fn owners_confirm_their_agents() {
	new_test_ext().execute_with(|| {
		register();
		assert_noop!(
			Registry::confirm_owner(RuntimeOrigin::signed(OTHER), AGENT),
			Error::<Test>::NotProposedOwner
		);
		// An unconfirmed owner has no rights over the agent.
		assert_noop!(
			Registry::set_active(RuntimeOrigin::signed(OWNER), AGENT, false),
			Error::<Test>::NotAllowed
		);
		assert_ok!(Registry::confirm_owner(RuntimeOrigin::signed(OWNER), AGENT));
		assert!(Agents::<Test>::get(AGENT).unwrap().owner_confirmed);
		assert_ok!(Registry::set_active(RuntimeOrigin::signed(OWNER), AGENT, false));
		assert!(!Agents::<Test>::get(AGENT).unwrap().active);

		// Naming another owner withdraws the confirmation.
		assert_ok!(Registry::set_owner(RuntimeOrigin::signed(AGENT), Some(OTHER)));
		assert!(!Agents::<Test>::get(AGENT).unwrap().owner_confirmed);
		assert_noop!(
			Registry::set_active(RuntimeOrigin::signed(OWNER), AGENT, true),
			Error::<Test>::NotAllowed
		);
	});
}

#[test]
fn payments_build_the_reputation() {
	new_test_ext().execute_with(|| {
		register();
		assert_ok!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), AGENT, 10));
		assert_ok!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), AGENT, 20));
		assert_ok!(Registry::pay(RuntimeOrigin::signed(OTHER), AGENT, 5));
		assert_eq!(Balances::free_balance(AGENT), 1_000 + 35 - held(AGENT));
		System::assert_last_event(Event::Paid { agent: AGENT, payer: OTHER, amount: 5 }.into());

		let r = Reputations::<Test>::get(AGENT);
		assert_eq!((r.payments, r.volume, r.payers), (3, 35, 2));
		assert_eq!(Payers::<Test>::get(AGENT, CUSTOMER).unwrap().unrated, 2);

		assert_noop!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), AGENT, 4), Error::<Test>::PaymentTooSmall);
		assert_noop!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), OTHER, 10), Error::<Test>::NotRegistered);
	});
}

#[test]
fn agents_and_owners_cannot_pay_themselves() {
	new_test_ext().execute_with(|| {
		register();
		assert_noop!(Registry::pay(RuntimeOrigin::signed(AGENT), AGENT, 10), Error::<Test>::SelfPayment);
		assert_ok!(Registry::confirm_owner(RuntimeOrigin::signed(OWNER), AGENT));
		assert_noop!(Registry::pay(RuntimeOrigin::signed(OWNER), AGENT, 10), Error::<Test>::SelfPayment);
	});
}

#[test]
fn inactive_agents_are_not_paid() {
	new_test_ext().execute_with(|| {
		register();
		assert_ok!(Registry::set_active(RuntimeOrigin::signed(AGENT), AGENT, false));
		assert_noop!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), AGENT, 10), Error::<Test>::Inactive);
	});
}

#[test]
fn each_payment_allows_one_rating() {
	new_test_ext().execute_with(|| {
		register();
		assert_noop!(Registry::rate(RuntimeOrigin::signed(CUSTOMER), AGENT, true), Error::<Test>::NothingToRate);
		assert_ok!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), AGENT, 10));
		assert_ok!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), AGENT, 10));
		assert_ok!(Registry::rate(RuntimeOrigin::signed(CUSTOMER), AGENT, true));
		assert_ok!(Registry::rate(RuntimeOrigin::signed(CUSTOMER), AGENT, false));
		assert_noop!(Registry::rate(RuntimeOrigin::signed(CUSTOMER), AGENT, true), Error::<Test>::NothingToRate);
		let r = Reputations::<Test>::get(AGENT);
		assert_eq!((r.positive, r.negative), (1, 1));
		System::assert_last_event(Event::Rated { agent: AGENT, payer: CUSTOMER, positive: false }.into());
	});
}

#[test]
fn reputation_survives_deregistration() {
	new_test_ext().execute_with(|| {
		register();
		assert_ok!(Registry::pay(RuntimeOrigin::signed(CUSTOMER), AGENT, 10));
		assert_ok!(Registry::rate(RuntimeOrigin::signed(CUSTOMER), AGENT, false));
		assert_noop!(Registry::deregister(RuntimeOrigin::signed(OTHER), AGENT), Error::<Test>::NotAllowed);
		assert_ok!(Registry::deregister(RuntimeOrigin::signed(AGENT), AGENT));
		assert_eq!(held(AGENT), 0);
		assert!(Agents::<Test>::get(AGENT).is_none());

		register();
		assert_eq!(Reputations::<Test>::get(AGENT).negative, 1);
	});
}

#[test]
fn tasks_are_recorded() {
	new_test_ext().execute_with(|| {
		<Registry as ReputationRecorder<u64>>::record_task(&AGENT, true);
		<Registry as ReputationRecorder<u64>>::record_task(&AGENT, false);
		<Registry as ReputationRecorder<u64>>::record_task(&AGENT, true);
		let r = Reputations::<Test>::get(AGENT);
		assert_eq!((r.tasks_completed, r.tasks_failed), (2, 1));
	});
}
