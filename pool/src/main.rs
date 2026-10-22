//! Mining pool of Tesserax.
//!
//! Rigs running XMRig (`rx/tess`) connect to the stratum server of the pool with their payout
//! address. The pool hands each rig its own range of extra nonce and a share difficulty that
//! follows its hashrate, verifies every share with RandomTESS, and submits the blocks they find
//! to its node, which mines to the pool account. Rewards are split PPLNS once the blocks are
//! final, and paid from the pool account when a balance reaches the minimum payout.
//!
//! ```text
//! tesserax --chain <spec> --stratum 127.0.0.1:3334 --author <pool address>
//! TESSERAX_POOL_KEY=<secret phrase of the pool account> tesserax-pool --node-stratum 127.0.0.1:3334
//! ```

mod api;
mod chain;
mod ledger;
mod server;
mod upstream;
mod verify;
mod work;

use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tesserax_sdk::{Amount, Keypair};

#[derive(Parser, Debug)]
#[command(name = "tesserax-pool", version, about = "Mining pool of Tesserax")]
struct Cli {
	/// Stratum server of the node, started with `--stratum` and `--author` the pool address.
	#[arg(long, default_value = "127.0.0.1:3334")]
	node_stratum: SocketAddr,
	/// RPC of the node, to follow the blocks of the pool and send the payouts.
	#[arg(long, default_value = "ws://127.0.0.1:9944")]
	node_rpc: String,
	/// Stratum server of the pool, for the rigs.
	#[arg(long, default_value = "0.0.0.0:3333")]
	listen: SocketAddr,
	/// Dashboard and HTTP API.
	#[arg(long, default_value = "0.0.0.0:8080")]
	api: SocketAddr,
	/// SQLite database of shares, blocks, balances and payments.
	#[arg(long, default_value = "tesserax-pool.sqlite")]
	db: PathBuf,
	/// File holding the secret phrase of the pool account, readable by its owner only. The
	/// environment variable TESSERAX_POOL_KEY can hold it instead.
	#[arg(long)]
	key_file: Option<PathBuf>,
	/// Fee of the pool on each block, in percent. It also pays the fees of the payouts.
	#[arg(long, default_value_t = 1.0)]
	fee: f64,
	/// Smallest balance paid, in TESS.
	#[arg(long, default_value = "1")]
	min_payout: String,
	/// Minutes between two rounds of payouts.
	#[arg(long, default_value_t = 60)]
	payout_interval: u64,
	/// Blocks on top of a block before its reward is credited. Below 60, the depth past which
	/// the chain never reorganizes, a credited block may still leave the chain.
	#[arg(long, default_value_t = 60)]
	maturity: u64,
	/// PPLNS window, as a multiple of the block difficulty.
	#[arg(long, default_value_t = 2.0)]
	pplns_factor: f64,
	/// Share difficulty of a rig when it connects.
	#[arg(long, default_value_t = 5_000)]
	start_difficulty: u64,
	/// Smallest share difficulty.
	#[arg(long, default_value_t = 500)]
	min_difficulty: u64,
	/// Seconds between two shares of a rig, which its difficulty follows.
	#[arg(long, default_value_t = 15.0)]
	share_time: f64,
	/// Threads verifying shares with RandomTESS. Each verification costs about one hash.
	#[arg(long, default_value_t = 2)]
	verify_threads: usize,
	/// Rigs connected at once.
	#[arg(long, default_value_t = 10_000)]
	max_connections: usize,
	/// Rigs connected at once from one IP address.
	#[arg(long, default_value_t = 256)]
	max_per_ip: usize,
	/// Minutes an IP address sending invalid shares stays banned.
	#[arg(long, default_value_t = 10)]
	ban_minutes: u64,
	/// Name of the pool on its dashboard.
	#[arg(long, default_value = "Tesserax Pool")]
	name: String,
	/// Stratum address the dashboard tells miners to use, such as pool.example.com:3333.
	#[arg(long)]
	stratum_url: Option<String>,
	/// Explorer for the links of the dashboard, such as https://explorer.tesserax.app.
	#[arg(long)]
	explorer: Option<String>,
}

fn load_key(path: Option<&PathBuf>) -> Result<Keypair, String> {
	let secret = match path {
		Some(path) => {
			#[cfg(unix)]
			{
				use std::os::unix::fs::PermissionsExt;
				let mode = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.permissions().mode();
				if mode & 0o077 != 0 {
					return Err(format!("{} is readable by others: chmod 600 it", path.display()));
				}
			}
			std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?
		},
		None => std::env::var("TESSERAX_POOL_KEY").map_err(|_| "set --key-file or TESSERAX_POOL_KEY to the secret phrase of the pool account".to_string())?,
	};
	Keypair::from_uri(secret.trim()).map_err(|e| format!("invalid pool key: {e}"))
}

#[tokio::main]
async fn main() {
	tracing_subscriber::fmt()
		.with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
		.init();
	if let Err(e) = run(Cli::parse()).await {
		tracing::error!("{e}");
		std::process::exit(1);
	}
}

async fn run(cli: Cli) -> Result<(), String> {
	if !(0.0..=100.0).contains(&cli.fee) {
		return Err("the fee is a percentage from 0 to 100".into());
	}
	if cli.maturity < 60 {
		tracing::warn!("Maturity of {} blocks: below 60, credited blocks may still leave the chain", cli.maturity);
	}
	let min_payout = Amount::from_tess(&cli.min_payout).map_err(|e| e.to_string())?.planck();
	let key = load_key(cli.key_file.as_ref())?;
	let address = key.address();
	tracing::info!("Pool account {address}");
	// The secret leaves the environment once read.
	std::env::remove_var("TESSERAX_POOL_KEY");

	let ledger = Arc::new(ledger::Ledger::open(&cli.db).map_err(|e| format!("{}: {e}", cli.db.display()))?);
	let interrupted = ledger.interrupted_payments().map_err(|e| e.to_string())?;
	match interrupted {
		0 => {},
		1 => tracing::error!("A payment was interrupted while sending: check it on the chain (status unknown in the database)"),
		n => tracing::error!("{n} payments were interrupted while sending: check them on the chain (status unknown in the database)"),
	}

	let chain = chain::Chain::connect(&cli.node_rpc).await.map_err(|e| format!("RPC of the node at {}: {e}", cli.node_rpc))?;
	let (upstream, templates) = upstream::start(cli.node_stratum, address.clone());
	let verifier = verify::Verifier::start(cli.verify_threads);
	let fee_bps = (cli.fee * 100.0).round() as u32;

	let pool = server::Pool::new(
		server::ServerConfig {
			listen: cli.listen,
			vardiff: work::Vardiff { share_seconds: cli.share_time, min: cli.min_difficulty.max(1), max: u64::MAX },
			start_difficulty: cli.start_difficulty.max(cli.min_difficulty).max(1),
			max_connections: cli.max_connections,
			max_per_ip: cli.max_per_ip,
			ban: Duration::from_secs(cli.ban_minutes * 60),
			pplns_factor: cli.pplns_factor.max(0.1),
		},
		templates,
		upstream,
		verifier,
		ledger.clone(),
	);

	// Shares are written every second, and pruned once no PPLNS window can use them.
	tokio::spawn({
		let pool = pool.clone();
		async move {
			let mut interval = tokio::time::interval(Duration::from_secs(1));
			let mut ticks = 0u64;
			loop {
				interval.tick().await;
				pool.flush();
				ticks += 1;
				if ticks % 600 == 0 {
					let difficulty = pool.templates.borrow().as_ref().map(|t| t.difficulty);
					if let Some(difficulty) = difficulty {
						let keep = pool.window(difficulty).saturating_mul(2);
						let day_ago = chain::now_ms() - 24 * 3600 * 1000;
						if let Err(e) = pool.ledger.prune_shares(keep, day_ago) {
							tracing::warn!("Pruning shares: {e}");
						}
					}
				}
			}
		}
	});

	tokio::spawn(chain::confirm_blocks(chain.clone(), ledger.clone(), key.account_id(), cli.maturity, fee_bps));
	tokio::spawn(chain::pay_miners(
		chain,
		ledger,
		key,
		Arc::new(chain::Payouts { min_payout, interval: Duration::from_secs(cli.payout_interval.max(1) * 60) }),
	));

	let info = api::Info {
		name: cli.name,
		address,
		stratum: cli.stratum_url.unwrap_or_else(|| cli.listen.to_string()),
		explorer: cli.explorer.map(|e| e.trim_end_matches('/').to_string()),
		fee_percent: cli.fee,
		min_payout: min_payout.to_string(),
		payout_interval_minutes: cli.payout_interval,
		maturity: cli.maturity,
		pplns_factor: cli.pplns_factor,
		block_reward: Amount::from_tess("0.85").expect("a valid amount; qed").planck().to_string(),
	};
	tokio::spawn(api::run(cli.api, pool.clone(), info));

	tokio::select! {
		result = server::run(pool.clone()) => result.map_err(|e| format!("stratum server on {}: {e}", cli.listen)),
		_ = stop() => {
			tracing::info!("Stopping: writing the last shares");
			pool.flush();
			Ok(())
		},
	}
}

/// Ctrl-C, or the termination signal of a service manager such as systemd.
async fn stop() {
	#[cfg(unix)]
	{
		let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal handlers install; qed");
		tokio::select! {
			_ = tokio::signal::ctrl_c() => {},
			_ = terminate.recv() => {},
		}
	}
	#[cfg(not(unix))]
	let _ = tokio::signal::ctrl_c().await;
}
