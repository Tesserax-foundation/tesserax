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
