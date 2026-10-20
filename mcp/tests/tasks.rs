//! The task market as agents see it, against a running development chain:
//!
//! ```text
//! tesserax --dev --tmp --mine --author <address>
//! cargo test -p tesserax-mcp --test tasks -- --ignored --nocapture
//! ```
//!
//! A client agent spending for its owner orders a translation; a worker agent finds it, accepts
//! it and delivers; the client approves and the worker is paid from escrow.

use rmcp::{
	model::CallToolRequestParams,
	service::RunningService,
	transport::{ConfigureCommandExt, TokioChildProcess},
	RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use tesserax_sdk::{Amount, Client, Keypair, Permissions, Policy};

fn url() -> String {
	std::env::var("TESSERAX_URL").unwrap_or_else(|_| "ws://127.0.0.1:9944".into())
}

fn tess(text: &str) -> Amount {
	Amount::from_tess(text).unwrap()
}

async fn agent(key: &str, owner: Option<&str>) -> RunningService<RoleClient, ()> {
	let command = tokio::process::Command::new(env!("CARGO_BIN_EXE_tesserax-mcp")).configure(|cmd| {
		cmd.env("TESSERAX_URL", url()).env("TESSERAX_KEY", key);
		if let Some(owner) = owner {
			cmd.env("TESSERAX_OWNER", owner);
		}
	});
	().serve(TokioChildProcess::new(command).unwrap()).await.unwrap()
}

async fn call(client: &RunningService<RoleClient, ()>, tool: &str, arguments: Value) -> (bool, Value) {
	let mut params = CallToolRequestParams::new(tool.to_string());
	if let Value::Object(map) = arguments {
		params = params.with_arguments(map);
	}
	let result = client.call_tool(params).await.expect("the server answers");
	let text: String = result.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect();
	println!("{tool} -> {text}");
	(result.is_error != Some(true), serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

#[tokio::test]
#[ignore]
async fn agents_trade_work_through_the_task_market() {
	let chain = Client::connect(&url()).await.expect("a development node is running");
	let owner = Keypair::from_uri("//Alice").unwrap();
	let worker = Keypair::from_uri("//Bob").unwrap();
	let (client_agent, phrase) = Keypair::generate().unwrap();

	chain.transfer(&owner, &client_agent.account_id(), tess("1")).await.unwrap();
	let policy = Policy {
		budget: tess("10"),
		period_blocks: 4_800,
		max_per_call: tess("5"),
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: false },
		targets: vec![],
	};
	chain.authorize_agent(&owner, &client_agent.account_id(), &policy).await.unwrap();

	let client = agent(&phrase, Some(&owner.address())).await;
	let contractor = agent("//Bob", None).await;

	// The client agent orders a translation for 2 TESS.
	let (ok, created) = call(
		&client,
		"tesserax_create_task",
		json!({
			"spec": "Translate https://example.com/contract-fr.txt to English",
			"capability": "translation",
			"reward": "2",
			"bond": "0.1",
			"delivery_minutes": 30,
			"review_minutes": 30
		}),
	)
	.await;
	assert!(ok, "{created}");
	let id = created["id"].as_u64().unwrap();

	// The worker finds it and takes it.
	let (ok, open) = call(&contractor, "tesserax_find_tasks", json!({"capability": "translation"})).await;
	assert!(ok && open.as_array().unwrap().iter().any(|t| t["id"] == id));
	let (ok, _) = call(&contractor, "tesserax_accept_task", json!({"id": id})).await;
	assert!(ok);
	let (ok, _) = call(&contractor, "tesserax_deliver_task", json!({"id": id, "result": "ipfs://contract-en"})).await;
	assert!(ok);

	// The client sees the result and approves it.
	let (ok, mine) = call(&client, "tesserax_my_tasks", json!({})).await;
	let task = mine.as_array().unwrap().iter().find(|t| t["id"] == id).unwrap().clone();
	assert!(ok);
	assert_eq!(task["status"]["state"], "delivered");
	assert_eq!(task["status"]["result"], "ipfs://contract-en");
	// Without arbiter, a result cannot be disputed.
	let (ok, error) = call(&client, "tesserax_dispute_task", json!({"id": id})).await;
	assert!(!ok && error.as_str().unwrap().contains("NoArbiter"), "{error}");

	let before = chain.balance(&worker.account_id()).await.unwrap().free;
	let completed = chain.agent(&worker.account_id()).await.unwrap().map(|a| a.reputation.tasks_completed);
	let (ok, _) = call(&client, "tesserax_approve_task", json!({"id": id})).await;
	assert!(ok);
	let after = chain.balance(&worker.account_id()).await.unwrap().free;
	assert_eq!(after.0 - before.0, tess("2").0 + tess("0.1").0, "the reward and the bond");
	assert!(chain.task(id).await.unwrap().is_none());
	if let Some(completed) = completed {
		assert_eq!(chain.agent(&worker.account_id()).await.unwrap().unwrap().reputation.tasks_completed, completed + 1);
	}

	client.cancel().await.unwrap();
	contractor.cancel().await.unwrap();
	chain.revoke_agent(&owner, &client_agent.account_id()).await.unwrap();
}
