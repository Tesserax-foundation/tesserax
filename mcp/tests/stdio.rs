//! The MCP server as an agent sees it, against a running development chain:
//!
//! ```text
//! tesserax --dev --tmp --mine --author <address>
//! cargo test -p tesserax-mcp --test stdio -- --ignored --nocapture
//! ```
//!
//! An owner delegates a budget to a fresh agent key; the agent then uses the server over stdio.

use rmcp::{
	model::{CallToolRequestParams, CallToolResult},
	service::RunningService,
	transport::{ConfigureCommandExt, TokioChildProcess},
	RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use tesserax_sdk::{Amount, Client, Keypair, Permissions, Policy};

fn url() -> String {
	std::env::var("TESSERAX_URL").unwrap_or_else(|_| "ws://127.0.0.1:9944".into())
}

async fn call(client: &RunningService<RoleClient, ()>, tool: &str, arguments: Value) -> (bool, String) {
	let mut params = CallToolRequestParams::new(tool.to_string());
	if let Value::Object(map) = arguments {
		params = params.with_arguments(map);
	}
	let result: CallToolResult = client.call_tool(params).await.expect("the server answers");
	let text = result
		.content
		.iter()
		.filter_map(|c| c.as_text().map(|t| t.text.clone()))
		.collect::<Vec<_>>()
		.join("\n");
	println!("{tool} -> {text}");
	(result.is_error != Some(true), text)
}

#[tokio::test]
#[ignore]
async fn an_agent_uses_tesserax_through_mcp() {
	// The owner sets up a delegation with the SDK.
	let chain = Client::connect(&url()).await.expect("a development node is running");
	let owner = Keypair::from_uri("//Alice").unwrap();
	let merchant = Keypair::from_uri("//Bob").unwrap();
	let (agent, phrase) = Keypair::generate().unwrap();
	chain.transfer(&owner, &agent.account_id(), Amount::from_tess("1").unwrap()).await.unwrap();
	let policy = Policy {
		budget: Amount::from_tess("3").unwrap(),
		period_blocks: 4_800,
		max_per_call: Amount::from_tess("2").unwrap(),
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: false },
		targets: vec![],
	};
	chain.authorize_agent(&owner, &agent.account_id(), &policy).await.unwrap();

	// The agent starts the MCP server with its own key, spending for the owner.
	let command = tokio::process::Command::new(env!("CARGO_BIN_EXE_tesserax-mcp")).configure(|cmd| {
		cmd.env("TESSERAX_URL", url())
			.env("TESSERAX_KEY", &phrase)
			.env("TESSERAX_OWNER", owner.address());
	});
	let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();

	let tools = client.list_all_tools().await.unwrap();
	let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
	for expected in ["tesserax_wallet", "tesserax_transfer", "tesserax_find_agents", "tesserax_pay_agent"] {
		assert!(names.iter().any(|n| n == expected), "{expected} in {names:?}");
	}

	let (ok, wallet) = call(&client, "tesserax_wallet", json!({})).await;
	assert!(ok && wallet.contains("\"delegated\"") && wallet.contains(&agent.address()));

	// Within the limits, then beyond the limit per payment, then beyond the budget.
	let (ok, _) = call(&client, "tesserax_transfer", json!({"to": merchant.address(), "amount": "1.5"})).await;
	assert!(ok);
	let (ok, error) = call(&client, "tesserax_transfer", json!({"to": merchant.address(), "amount": "2.5"})).await;
	assert!(!ok && error.contains("CallLimitExceeded"), "{error}");
	let (ok, error) = call(&client, "tesserax_transfer", json!({"to": merchant.address(), "amount": "2"})).await;
	assert!(!ok && error.contains("BudgetExceeded"), "{error}");

	// Invalid input is reported to the agent, not crashed on.
	let (ok, error) = call(&client, "tesserax_transfer", json!({"to": "nowhere", "amount": "1"})).await;
	assert!(!ok && error.contains("invalid address"), "{error}");

	let (ok, delegation) = call(
		&client,
		"tesserax_delegation",
		json!({"owner": owner.address(), "agent": agent.address()}),
	)
	.await;
	assert!(ok && delegation.contains("\"remaining\": \"1.5\""), "{delegation}");

	let (ok, _) = call(&client, "tesserax_find_agents", json!({"capability": "translation"})).await;
	assert!(ok);

	client.cancel().await.unwrap();
	chain.revoke_agent(&owner, &agent.account_id()).await.unwrap();
}
