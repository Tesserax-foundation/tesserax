//! Smart contracts against a running development chain: deploys `fixtures/contracts/Token.sol`.
//!
//! ```text
//! tesserax --dev --tmp --mine --author <address>
//! cargo test -p tesserax-sdk --test contracts -- --ignored --nocapture
//! ```
//!
//! The node URL defaults to `ws://127.0.0.1:9944` and can be set with `TESSERAX_URL`.

use futures::StreamExt;
use std::time::Duration;
use tesserax_sdk::{
	contracts::{h160, DynSolValue, JsonAbi, U256},
	runtime, Client, Error, Keypair,
};

fn url() -> String {
	std::env::var("TESSERAX_URL").unwrap_or_else(|_| "ws://127.0.0.1:9944".into())
}

fn uint(value: u64) -> DynSolValue {
	DynSolValue::Uint(U256::from(value), 256)
}

fn token() -> (Vec<u8>, JsonAbi) {
	let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/contracts");
	let code = std::fs::read(format!("{dir}/Token.polkavm")).unwrap();
	let abi = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/Token.abi.json")).unwrap()).unwrap();
	(code, abi)
}

#[tokio::test]
#[ignore]
async fn token_on_a_development_chain() {
	let client = Client::connect(&url()).await.expect("a development node is running");
	let alice = Keypair::from_uri("//Alice").unwrap();
	let bob = Keypair::from_uri("//Bob").unwrap();
	let (alice_h160, bob_h160) = (h160(&alice.account_id()), h160(&bob.account_id()));

	// The address of an account in contracts is the one the runtime maps.
	let mapped = client
		.api()
		.runtime_api()
		.at_latest()
		.await
		.unwrap()
		.call(runtime::apis().revive_api().address(alice.account_id()))
		.await
		.unwrap();
	assert_eq!(mapped.0, alice_h160.into_array());

	// Deployment: the constructor mints the supply to the deployer.
	let (code, abi) = token();
	let args = [DynSolValue::String("Tesserax Coin".into()), uint(1_000_000)];
	let (token, deployed) = client.deploy_contract(&alice, &code, abi, &args, Default::default()).await.unwrap();
	println!("Token at {} in block #{}", token.address(), deployed.outcome.block_number);
	let minted = &deployed.events[0];
	assert_eq!(minted.name.as_deref(), Some("Transfer"));
	assert_eq!(minted.args[1], ("to".into(), DynSolValue::Address(alice_h160)));
	assert_eq!(minted.args[2], ("value".into(), uint(1_000_000)));

	// Reads: dry runs, without a transaction.
	assert_eq!(token.read("name", &[]).await.unwrap(), [DynSolValue::String("Tesserax Coin".into())]);
	assert_eq!(token.read("balanceOf", &[DynSolValue::Address(alice_h160)]).await.unwrap(), [uint(1_000_000)]);

	// Watch the events of the contract while transferring.
	let watcher = token.clone();
	let watched = tokio::spawn(async move {
		let mut events = Box::pin(watcher.watch_events().await.unwrap());
		events.next().await.unwrap().unwrap()
	});
	tokio::time::sleep(Duration::from_secs(1)).await;

	let sent = token.write(&alice, "transfer", &[DynSolValue::Address(bob_h160), uint(250)]).await.unwrap();
	let transfer = &sent.events[0];
	assert_eq!(transfer.name.as_deref(), Some("Transfer"));
	assert_eq!(transfer.args[0].1, DynSolValue::Address(alice_h160));
	assert_eq!(transfer.args[1].1, DynSolValue::Address(bob_h160));
	assert_eq!(transfer.args[2].1, uint(250));
	assert_eq!(token.read("balanceOf", &[DynSolValue::Address(bob_h160)]).await.unwrap(), [uint(250)]);

	let seen =
		tokio::time::timeout(Duration::from_secs(120), watched).await.expect("an event within two minutes").unwrap();
	assert_eq!(seen.name.as_deref(), Some("Transfer"));
	assert_eq!(seen.block_number, sent.outcome.block_number);

	// A call that would revert is stopped by its dry run, with the reason decoded.
	match token.write(&bob, "transfer", &[DynSolValue::Address(alice_h160), uint(1_000)]).await {
		Err(Error::ContractReverted(reason)) => {
			assert_eq!(reason, "InsufficientBalance(available: 250, required: 1000)")
		},
		other => panic!("expected a revert, got {other:?}"),
	}
	match token.write(&bob, "burn", &[uint(1_000)]).await {
		Err(Error::ContractReverted(reason)) => assert_eq!(reason, "burn exceeds balance"),
		other => panic!("expected a revert, got {other:?}"),
	}

	// Past events, from the deployment on.
	let events = token.events(deployed.outcome.block_number, sent.outcome.block_number).await.unwrap();
	let transfers: Vec<_> = events.iter().filter(|e| e.name.as_deref() == Some("Transfer")).collect();
	assert_eq!(transfers.len(), 2);
	assert_eq!(transfers[1].args[2].1, uint(250));
}
