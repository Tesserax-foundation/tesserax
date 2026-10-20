//! Against a running development chain:
//!
//! ```text
//! tesserax --dev --tmp --mine --author <address>
//! cargo test -p tesserax-sdk --test dev_chain -- --ignored --nocapture
//! ```
//!
//! The node URL defaults to `ws://127.0.0.1:9944` and can be set with `TESSERAX_URL`.

use tesserax_sdk::{Amount, Client, Error, Keypair, Permissions, Policy, Profile};

fn url() -> String {
	std::env::var("TESSERAX_URL").unwrap_or_else(|_| "ws://127.0.0.1:9944".into())
}

fn tess(text: &str) -> Amount {
	Amount::from_tess(text).unwrap()
}

#[tokio::test]
#[ignore]
async fn agents_on_a_development_chain() {
	let client = Client::connect(&url()).await.expect("a development node is running");
	let info = client.chain_info().await.unwrap();
	println!("{info:?}");
	assert_eq!(info.ss58_prefix, 2261);

	let owner = Keypair::from_uri("//Alice").unwrap();
	let merchant = Keypair::from_uri("//Bob").unwrap();
	// A fresh agent key for every run.
	let (agent, _) = Keypair::generate().unwrap();

	// The owner funds the agent's fees and delegates 10 TESS per day, 6 per call.
	client.transfer(&owner, &agent.account_id(), tess("1")).await.unwrap();
	let policy = Policy {
		budget: tess("10"),
		period_blocks: 4_800,
		max_per_call: tess("6"),
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: false },
		targets: vec![],
	};
	client.authorize_agent(&owner, &agent.account_id(), &policy).await.unwrap();
	let delegation = client.delegation(&owner.account_id(), &agent.account_id()).await.unwrap().unwrap();
	assert_eq!(delegation.policy, policy);
	assert_eq!(delegation.remaining, tess("10"));

	// The agent pays the merchant from the owner's funds.
	let before = client.balance(&merchant.account_id()).await.unwrap().free;
	client.agent_transfer(&agent, &owner.account_id(), &merchant.account_id(), tess("5")).await.unwrap();
	let after = client.balance(&merchant.account_id()).await.unwrap().free;
	assert_eq!(after.0 - before.0, tess("5").0);
	let delegation = client.delegation(&owner.account_id(), &agent.account_id()).await.unwrap().unwrap();
	assert_eq!(delegation.remaining, tess("5"));

	// Beyond the budget: refused by the chain.
	let refused = client.agent_transfer(&agent, &owner.account_id(), &merchant.account_id(), tess("6")).await;
	match refused {
		Err(Error::Dispatch(message)) => assert!(message.contains("BudgetExceeded"), "{message}"),
		other => panic!("expected a refusal, got {other:?}"),
	}

	// The merchant registers as an agent and is found by capability.
	let profile = Profile {
		name: "translator".into(),
		description: "French to English".into(),
		endpoint: "https://translator.example/mcp".into(),
		capabilities: vec!["translation".into()],
		metadata: None,
	};
	if client.agent(&merchant.account_id()).await.unwrap().is_none() {
		client.register_agent(&merchant, &profile, None).await.unwrap();
	}
	let found = client.find_agents(Some("translation"), true).await.unwrap();
	assert!(found.iter().any(|a| a.address == merchant.address()));

	// The agent pays the registered agent, which builds its reputation.
	let payments = client.agent(&merchant.account_id()).await.unwrap().unwrap().reputation.payments;
	client.agent_pay_agent(&agent, &owner.account_id(), &merchant.account_id(), tess("1")).await.unwrap();
	let record = client.agent(&merchant.account_id()).await.unwrap().unwrap();
	assert_eq!(record.reputation.payments, payments + 1);
	println!("{record:#?}");

	client.revoke_agent(&owner, &agent.account_id()).await.unwrap();
	assert!(client.delegation(&owner.account_id(), &agent.account_id()).await.unwrap().is_none());
}

fn temporary_dir() -> std::path::PathBuf {
	let (_, phrase) = Keypair::generate().unwrap();
	let suffix: String = phrase.split(' ').take(3).collect::<Vec<_>>().join("-");
	let dir = std::env::temp_dir().join(format!("tesserax-channels-{suffix}"));
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

/// A payer and a service restart from their state files: the payer resumes its channel, the
/// service claims what it accepted before and refuses vouchers it already accepted.
#[tokio::test]
#[ignore]
async fn channel_state_survives_restarts() {
	use tesserax_sdk::{ChannelPayer, ChannelReceiver, PaymentRequired};

	let client = Client::connect(&url()).await.expect("a development node is running");
	let funder = Keypair::from_uri("//Alice").unwrap();
	let (payer_key, _) = Keypair::generate().unwrap();
	let (service_key, _) = Keypair::generate().unwrap();
	client.transfer(&funder, &payer_key.account_id(), tess("10")).await.unwrap();
	client.transfer(&funder, &service_key.account_id(), tess("1")).await.unwrap();

	let dir = temporary_dir();
	let (payer_file, service_file) = (dir.join("payer.json"), dir.join("service.json"));
	let price = tess("0.1");
	async fn pay(payer: &ChannelPayer, receiver: &ChannelReceiver) -> tesserax_sdk::Voucher {
		let header = receiver.payment_required(tess("0.1"), None);
		payer.pay(&PaymentRequired::from_header(&header).unwrap(), tess("1")).await.unwrap()
	}

	let payer = ChannelPayer::with_file(client.clone(), payer_key.clone(), None, tess("2"), &payer_file).unwrap();
	let receiver = ChannelReceiver::with_file(client.clone(), service_key.clone(), &service_file).unwrap();
	let first = pay(&payer, &receiver).await;
	assert_eq!(receiver.verify(&first.to_header(), price).await, Ok(tess("0.1")));
	let second = pay(&payer, &receiver).await;
	assert_eq!(receiver.verify(&second.to_header(), price).await, Ok(tess("0.2")));
	drop((payer, receiver));

	// Both restart.
	let payer = ChannelPayer::with_file(client.clone(), payer_key.clone(), None, tess("2"), &payer_file).unwrap();
	let receiver = ChannelReceiver::with_file(client.clone(), service_key.clone(), &service_file).unwrap();
	assert_eq!(receiver.unclaimed().await, tess("0.2"));

	// A voucher accepted before the restart is refused.
	let replay = receiver.verify(&second.to_header(), price).await.unwrap_err();
	assert_eq!(replay.reason, "the voucher does not pay the price");
	assert_eq!(replay.accepted, Some(tess("0.2")));

	// The payer goes on with the same channel.
	let third = pay(&payer, &receiver).await;
	assert_eq!(third.channel, first.channel);
	assert_eq!(third.amount, tess("0.3"));
	assert_eq!(receiver.verify(&third.to_header(), price).await, Ok(tess("0.3")));

	// What was accepted before and after the restart is claimed in one transaction.
	assert_eq!(receiver.claim_all().await.unwrap().len(), 1);
	assert_eq!(receiver.unclaimed().await, Amount(0));
	assert_eq!(client.channel(first.channel).await.unwrap().unwrap().claimed, tess("0.3"));
	drop(receiver);
	let receiver = ChannelReceiver::with_file(client.clone(), service_key.clone(), &service_file).unwrap();
	assert_eq!(receiver.unclaimed().await, Amount(0));
	assert!(receiver.claim_all().await.unwrap().is_empty());

	// A state file belongs to one key.
	let (other, _) = Keypair::generate().unwrap();
	assert!(matches!(ChannelPayer::with_file(client.clone(), other.clone(), None, tess("2"), &payer_file), Err(Error::Storage(_))));
	assert!(matches!(ChannelReceiver::with_file(client.clone(), other, &service_file), Err(Error::Storage(_))));

	std::fs::remove_dir_all(dir).unwrap();
}
