//! The wallet as a user runs it, against a running development chain:
//!
//! ```text
//! tesserax --dev --tmp --mine --author <address>
//! cargo test -p tesserax-wallet --test dev_chain -- --ignored --nocapture
//! ```

use serde_json::Value;
use std::{
	io::Write,
	path::PathBuf,
	process::{Command, Stdio},
};

struct Wallet {
	dir: PathBuf,
}

impl Wallet {
	fn new() -> Self {
		let mut suffix = [0u8; 8];
		getrandom::fill(&mut suffix).unwrap();
		let dir = std::env::temp_dir().join(format!("tesserax-wallet-{}", u64::from_le_bytes(suffix)));
		Wallet { dir }
	}

	/// Run the wallet with `args`, `input` on standard input; returns success and output.
	fn run_with(&self, args: &[&str], input: &str, password: &str) -> (bool, String) {
		let mut child = Command::new(env!("CARGO_BIN_EXE_tesserax-wallet"))
			.args(["--json", "--yes"])
			.args(args)
			.env("TESSERAX_WALLET_DIR", &self.dir)
			.env("TESSERAX_PASSWORD", password)
			.env("TESSERAX_URL", std::env::var("TESSERAX_URL").unwrap_or_else(|_| "ws://127.0.0.1:9944".into()))
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
		let output = child.wait_with_output().unwrap();
		let text = String::from_utf8_lossy(&output.stdout).to_string() + &String::from_utf8_lossy(&output.stderr);
		println!("$ tesserax-wallet {} -> {}", args.join(" "), text.trim());
		(output.status.success(), String::from_utf8_lossy(&output.stdout).to_string())
	}

	fn run(&self, args: &[&str]) -> Value {
		let (ok, output) = self.run_with(args, "", "correct horse battery");
		assert!(ok, "{args:?} failed");
		serde_json::from_str(&output).unwrap_or(Value::String(output))
	}
}

impl Drop for Wallet {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.dir);
	}
}

#[test]
#[ignore]
fn a_user_runs_the_wallet() {
	let w = Wallet::new();
	let password = "correct horse battery";
	assert!(w.run_with(&["account", "import", "alice"], "//Alice\n", password).0);
	assert!(w.run_with(&["account", "import", "bob"], "//Bob\n", password).0);
	let agent = w.run(&["account", "new", "agent"])["address"].as_str().unwrap().to_string();
	assert_eq!(w.run(&["account", "list"]).as_array().unwrap().len(), 3);

	let balance = w.run(&["balance", "alice"]);
	assert_ne!(balance["transferable"], "0");

	// Fund the agent and delegate a budget to it.
	w.run(&["transfer", "--from", "alice", "agent", "1"]);
	assert_eq!(w.run(&["balance", &agent])["free"], "1");
	w.run(&["agent", "authorize", "--from", "alice", "agent", "--budget", "10", "--max-per-call", "5"]);
	assert_eq!(w.run(&["agent", "show", "alice", "agent"])["remaining"], "10");

	// Bob offers translations; the agent pays him and orders work from him for Alice.
	if !w.run_with(&["registry", "show", "bob"], "", password).0 {
		w.run(&["registry", "register", "--from", "bob", "--name", "translator", "--capability", "translation"]);
	}
	w.run(&["registry", "pay", "--from", "agent", "--owner", "alice", "bob", "0.5"]);
	let id = w.run(&[
		"task", "create", "--from", "agent", "--owner", "alice", "--spec", "Translate the contract", "--capability",
		"translation", "--reward", "2", "--delivery-hours", "1", "--review-hours", "1",
	])["task"]
		.as_u64()
		.unwrap()
		.to_string();
	assert!(w.run(&["task", "list", "--capability", "translation"]).as_array().unwrap().iter().any(|t| t["id"].to_string() == id));
	w.run(&["task", "accept", "--from", "bob", &id]);
	w.run(&["task", "deliver", "--from", "bob", &id, "ipfs://contract-en"]);
	assert_eq!(w.run(&["task", "show", &id])["status"]["state"], "delivered");
	w.run(&["task", "approve", "--from", "agent", "--owner", "alice", &id]);
	// 0.5 paid, 2 of reward and 0.1 of storage deposit held: the deposit returns to Alice but
	// stays counted in the budget of the period.
	assert_eq!(w.run(&["agent", "show", "alice", "agent"])["remaining"], "7.4");

	// A payment channel for micropayments.
	let channel = w.run(&["channel", "open", "--from", "agent", "--owner", "alice", "bob", "1"])["channel"]
		.as_u64()
		.unwrap()
		.to_string();
	assert!(w.run(&["channel", "list", "alice"]).as_array().unwrap().iter().any(|c| c["id"].to_string() == channel));
	w.run(&["channel", "close", "--from", "agent", "--owner", "alice", &channel]);

	// Secrets need the password.
	assert!(!w.run_with(&["account", "export", "alice"], "", "wrong password").0);
	let (ok, phrase) = w.run_with(&["account", "export", "alice"], "", password);
	assert!(ok && phrase.trim() == "//Alice");

	w.run(&["agent", "revoke", "--from", "alice", "agent"]);
}
