//! Micropayments as an agent sees them, against a running development chain:
//!
//! ```text
//! tesserax --dev --tmp --mine --author <address>
//! cargo test -p tesserax-mcp --test paid_http -- --ignored --nocapture
//! ```
//!
//! A service charging 0.001 TESS per request runs in the test; an agent spending for its owner
//! calls it through `tesserax_fetch`, paying with vouchers on a payment channel.

use rmcp::{
	model::CallToolRequestParams,
	service::RunningService,
	transport::{ConfigureCommandExt, TokioChildProcess},
	RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tesserax_sdk::{Amount, ChannelReceiver, Client, Keypair, Permissions, Policy, VOUCHER_HEADER};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::TcpListener,
};

fn url() -> String {
	std::env::var("TESSERAX_URL").unwrap_or_else(|_| "ws://127.0.0.1:9944".into())
}

fn tess(text: &str) -> Amount {
	Amount::from_tess(text).unwrap()
}

/// A minimal HTTP service charging `price` per request.
async fn serve(receiver: Arc<ChannelReceiver>, price: Amount) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = format!("http://{}", listener.local_addr().unwrap());
	tokio::spawn(async move {
		let mut served = 0u32;
		loop {
			let (mut socket, _) = listener.accept().await.unwrap();
			let mut request = Vec::new();
			let mut buffer = [0u8; 8192];
			while !request.windows(4).any(|w| w == b"\r\n\r\n") {
				let n = socket.read(&mut buffer).await.unwrap();
				if n == 0 {
					break;
				}
				request.extend_from_slice(&buffer[..n]);
			}
			let text = String::from_utf8_lossy(&request).to_string();
			let voucher = text.lines().find_map(|line| {
				let (name, value) = line.split_once(':')?;
				name.eq_ignore_ascii_case(VOUCHER_HEADER).then(|| value.trim().to_string())
			});
			let response = match voucher {
				None => format!(
					"HTTP/1.1 402 Payment Required\r\nTesserax-Payment: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
					receiver.payment_required(price, None)
				),
				Some(voucher) => match receiver.verify(&voucher, price).await {
					Ok(_) => {
						served += 1;
						let body = format!("translation number {served}");
						format!(
							"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
							body.len()
						)
					},
					Err(rejection) => format!(
						"HTTP/1.1 402 Payment Required\r\nTesserax-Payment: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
						receiver.payment_required(price, Some(&rejection))
					),
				},
			};
			socket.write_all(response.as_bytes()).await.unwrap();
			socket.shutdown().await.ok();
		}
	});
	address
}

async fn call(client: &RunningService<RoleClient, ()>, tool: &str, arguments: Value) -> (bool, Value) {
	let mut params = CallToolRequestParams::new(tool.to_string());
	if let Value::Object(map) = arguments {
		params = params.with_arguments(map);
	}
	let result = client.call_tool(params).await.expect("the server answers");
	let text: String = result.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect();
	println!("{tool} -> {text}");
	let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
	(result.is_error != Some(true), value)
}

#[tokio::test]
#[ignore]
async fn an_agent_pays_a_service_per_request() {
	let chain = Client::connect(&url()).await.expect("a development node is running");
	let owner = Keypair::from_uri("//Alice").unwrap();
	let service_key = Keypair::from_uri("//Bob").unwrap();
	let (agent, phrase) = Keypair::generate().unwrap();

	// The owner funds the agent's fees and delegates 5 TESS a day to it.
	chain.transfer(&owner, &agent.account_id(), tess("1")).await.unwrap();
	let policy = Policy {
		budget: tess("5"),
		period_blocks: 4_800,
		max_per_call: tess("3"),
		expires_at: None,
		permissions: Permissions { transfers: true, contracts: false },
		targets: vec![],
	};
	chain.authorize_agent(&owner, &agent.account_id(), &policy).await.unwrap();

	let receiver = Arc::new(ChannelReceiver::new(chain.clone(), service_key.clone()));
	let service = serve(receiver.clone(), tess("0.001")).await;
	let expensive = serve(Arc::new(ChannelReceiver::new(chain.clone(), service_key.clone())), tess("0.5")).await;

	let command = tokio::process::Command::new(env!("CARGO_BIN_EXE_tesserax-mcp")).configure(|cmd| {
		cmd.env("TESSERAX_URL", url())
			.env("TESSERAX_KEY", &phrase)
			.env("TESSERAX_OWNER", owner.address())
			.env("TESSERAX_CHANNEL_DEPOSIT", "2");
	});
	let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();

	// Five paid requests: one transaction to open the channel, then vouchers only.
	let started = std::time::Instant::now();
	for request in 1..=5u32 {
		let (ok, result) = call(&client, "tesserax_fetch", json!({"url": format!("{service}/translate")})).await;
		assert!(ok);
		assert_eq!(result["status"], 200);
		assert_eq!(result["body"], format!("translation number {request}"));
		assert_eq!(result["payment"]["total"], format!("0.00{request}"));
	}
	println!("5 paid requests in {:?}", started.elapsed());

	// A price above the maximum is refused.
	let (ok, error) = call(&client, "tesserax_fetch", json!({"url": expensive})).await;
	assert!(!ok && error.as_str().unwrap().contains("more than the maximum"), "{error}");

	let (ok, channels) = call(&client, "tesserax_channels", json!({})).await;
	assert!(ok);
	let channel = channels.as_array().unwrap().iter().find(|c| c["payee"] == service_key.address()).unwrap();
	assert_eq!(channel["deposit"], "2");
	assert_eq!(channel["payer"], owner.address());

	// The service claims everything it was paid in one transaction.
	assert_eq!(receiver.unclaimed().await, tess("0.005"));
	let before = chain.balance(&service_key.account_id()).await.unwrap().free;
	receiver.claim_all().await.unwrap();
	let after = chain.balance(&service_key.account_id()).await.unwrap().free;
	// One claim fee (about 0.002 TESS) for all the payments.
	let fee = before.0 + tess("0.005").0 - after.0;
	println!("claimed 0.005 TESS for a fee of {}", Amount(fee));
	assert!(fee < tess("0.005").0, "{}", Amount(fee));

	let id = channel["id"].as_u64().unwrap();
	let (ok, _) = call(&client, "tesserax_close_channel", json!({"channel": id})).await;
	assert!(ok);
	assert!(chain.channel(id).await.unwrap().unwrap().closes_at.is_some());

	client.cancel().await.unwrap();
	chain.revoke_agent(&owner, &agent.account_id()).await.unwrap();
}
