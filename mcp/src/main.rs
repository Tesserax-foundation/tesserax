//! MCP server of Tesserax.
//!
//! Exposes Tesserax to any AI agent that speaks the Model Context Protocol: reading balances and
//! the agent registry, paying, getting paid and building a reputation. It runs next to the agent
//! and talks to it over stdio.
//!
//! Configuration, from the environment:
//!
//! - `TESSERAX_URL`: WebSocket URL of a node, `ws://127.0.0.1:9944` by default;
//! - `TESSERAX_KEY`: secret URI of the agent's ML-DSA key (a 24-word phrase, optionally with a
//!   derivation path), or `TESSERAX_KEY_FILE`: a file containing it. Without a key, only the
//!   reading tools are available;
//! - `TESSERAX_OWNER`: address of the owner whose funds the agent spends through its delegation.
//!   Recommended: the chain then enforces the budget, the limit per call, the allowed recipients
//!   and the expiry set by the owner, and the owner can revoke the agent at once. Without it, the
//!   agent spends the funds of its own key.

mod server;

use rmcp::{transport::stdio, ServiceExt};
use tesserax_sdk::{parse_address, Client, Keypair};

fn env(name: &str) -> Option<String> {
	std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn key() -> Result<Option<Keypair>, String> {
	let uri = match (env("TESSERAX_KEY"), env("TESSERAX_KEY_FILE")) {
		(Some(uri), _) => uri,
		(None, Some(path)) =>
			std::fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?,
		(None, None) => return Ok(None),
	};
	Keypair::from_uri(&uri).map(Some).map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let url = env("TESSERAX_URL").unwrap_or_else(|| "ws://127.0.0.1:9944".into());
	let key = key()?;
	let owner = env("TESSERAX_OWNER").map(|o| parse_address(&o)).transpose()?;
	if owner.is_some() && key.is_none() {
		return Err("TESSERAX_OWNER requires the agent key in TESSERAX_KEY or TESSERAX_KEY_FILE".into());
	}

	let client = Client::connect(&url).await?;
	// Logs go to stderr: stdout carries the protocol.
	eprintln!(
		"tesserax-mcp: connected to {url}, key {}, {}",
		key.as_ref().map(|k| k.address()).unwrap_or_else(|| "none (read only)".into()),
		match &owner {
			Some(o) => format!("spending for owner {}", tesserax_sdk::format_address(o)),
			None => "spending its own funds".into(),
		}
	);

	let service = server::Tesserax::new(client, key, owner).serve(stdio()).await?;
	service.waiting().await?;
	Ok(())
}
