//! Command line wallet of Tesserax.
//!
//! Keeps ML-DSA-87 keys encrypted in a wallet directory (`~/.tesserax/wallet` by default) and
//! sends transactions to a node: transfers, delegations to AI agents, the agent registry,
//! payment channels and the task market.

mod keystore;

use clap::{Args, Parser, Subcommand};
use keystore::Keystore;
use serde::Serialize;
use std::{io::IsTerminal, path::PathBuf};
use tesserax_sdk::{
	format_address, parse_address, AccountId, Amount, Client, Keypair, NewTask, Permissions, Policy,
	Profile, Voucher, BLOCKS_PER_HOUR,
};

/// Tesserax wallet: post-quantum accounts, transfers, AI agents, micropayments and tasks.
#[derive(Parser)]
#[command(name = "tesserax-wallet", version)]
struct Cli {
	/// WebSocket URL of a node.
	#[arg(long, global = true, env = "TESSERAX_URL", default_value = "ws://127.0.0.1:9944")]
	url: String,

	/// Wallet directory.
	#[arg(long, global = true, env = "TESSERAX_WALLET_DIR")]
	wallet_dir: Option<PathBuf>,

	/// Print results as JSON.
	#[arg(long, global = true)]
	json: bool,

	/// Do not ask for confirmation before sending a transaction.
	#[arg(long, short = 'y', global = true)]
	yes: bool,

	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Manage accounts.
	#[command(subcommand)]
	Account(AccountCommand),
	/// Balance of an account or an address.
	Balance {
		/// Account name or address.
		who: String,
	},
	/// Send TESS.
	Transfer {
		/// Account sending.
		#[arg(long)]
		from: String,
		/// Recipient: address or account name.
		to: String,
		/// Amount in TESS, such as 1.5.
		amount: String,
	},
	/// Delegations to AI agents.
	#[command(subcommand)]
	Agent(AgentCommand),
	/// Registry of AI agents.
	#[command(subcommand)]
	Registry(RegistryCommand),
	/// Payment channels.
	#[command(subcommand)]
	Channel(ChannelCommand),
	/// Task market.
	#[command(subcommand)]
	Task(TaskCommand),
	/// Information about the chain.
	Chain,
}

#[derive(Subcommand)]
enum AccountCommand {
	/// Create an account with a new 24-word phrase.
	New { name: String },
	/// Import an account from its phrase, read from the terminal or from standard input.
	Import { name: String },
	/// List the accounts.
	List,
	/// Show the address of an account.
	Show { name: String },
	/// Show the secret phrase of an account.
	Export { name: String },
	/// Delete an account from the wallet. Its funds stay on chain, reachable with its phrase.
	Remove { name: String },
}

/// Who signs: an account of the wallet, optionally acting for an owner through its delegation.
#[derive(Args)]
struct Signer {
	/// Account signing.
	#[arg(long)]
	from: String,
	/// Act for this owner through the delegation of the signing account.
	#[arg(long)]
	owner: Option<String>,
}

#[derive(Subcommand)]
enum AgentCommand {
	/// Delegate to an agent key, or change the limits of its delegation.
	Authorize {
		/// Owner account.
		#[arg(long)]
		from: String,
		/// Address of the agent.
		agent: String,
		/// Maximum spent per period, in TESS.
		#[arg(long)]
		budget: String,
		/// Length of a period, in hours.
		#[arg(long, default_value_t = 24)]
		period_hours: u32,
		/// Maximum spent by a single call, in TESS.
		#[arg(long)]
		max_per_call: String,
		/// Hours after which the delegation expires.
		#[arg(long)]
		expires_in_hours: Option<u32>,
		/// Allow calls of contracts.
		#[arg(long)]
		contracts: bool,
		/// Do not allow transfers.
		#[arg(long)]
		no_transfers: bool,
		/// Recipients and contracts allowed; any if none.
		#[arg(long = "target")]
		targets: Vec<String>,
	},
	/// Revoke the delegation to an agent, at once.
	Revoke {
		#[arg(long)]
		from: String,
		agent: String,
	},
	/// Show a delegation.
	Show {
		/// Owner: address or account name.
		owner: String,
		/// Agent: address or account name.
		agent: String,
	},
}

#[derive(Subcommand)]
enum RegistryCommand {
	/// Register an account as an agent.
	Register {
		#[arg(long)]
		from: String,
		#[arg(long)]
		name: String,
		#[arg(long, default_value = "")]
		description: String,
		/// Where the service of the agent is reached, such as an HTTPS or MCP URL.
		#[arg(long, default_value = "")]
		endpoint: String,
		/// Capabilities, such as translation.
		#[arg(long = "capability")]
		capabilities: Vec<String>,
		/// Owner of the agent, who must confirm.
		#[arg(long)]
		owner: Option<String>,
	},
	/// Confirm owning an agent.
	ConfirmOwner {
		#[arg(long)]
		from: String,
		agent: String,
	},
	/// Show an agent and its reputation.
	Show { agent: String },
	/// Find agents, best rated first.
	Find {
		#[arg(long)]
		capability: Option<String>,
	},
	/// Pay a registered agent; the payment counts in its reputation.
	Pay {
		#[command(flatten)]
		signer: Signer,
		agent: String,
		amount: String,
	},
	/// Rate an agent you paid, once per payment.
	Rate {
		#[arg(long)]
		from: String,
		agent: String,
		/// good or bad.
		rating: String,
	},
}

#[derive(Subcommand)]
enum ChannelCommand {
	/// Channels paid by an account.
	List { who: String },
	/// Show a channel.
	Show { id: u64 },
	/// Open a channel to a payee, signed by the vouchers of the signing account.
	Open {
		#[command(flatten)]
		signer: Signer,
		payee: String,
		deposit: String,
	},
	/// Add to the deposit of a channel.
	TopUp {
		#[command(flatten)]
		signer: Signer,
		id: u64,
		amount: String,
	},
	/// Request to close a channel as its payer: the rest returns after 6 hours.
	Close {
		#[command(flatten)]
		signer: Signer,
		id: u64,
	},
	/// Finalize the close of a channel after its close period.
	Finalize {
		#[arg(long)]
		from: String,
		id: u64,
	},
	/// Claim a voucher as the payee: the value of a Tesserax-Voucher header.
	Claim {
		#[arg(long)]
		from: String,
		voucher: String,
	},
	/// Close a channel as its payee: the rest returns to the payer.
	CloseAsPayee {
		#[arg(long)]
		from: String,
		id: u64,
	},
}

#[derive(Subcommand)]
enum TaskCommand {
	/// Open tasks, best paid first.
	List {
		#[arg(long)]
		capability: Option<String>,
	},
	/// Show a task.
	Show { id: u64 },
	/// Order work: the reward is held in escrow.
	Create {
		#[command(flatten)]
		signer: Signer,
		/// What to do: a URI or a short text.
		#[arg(long)]
		spec: String,
		#[arg(long)]
		capability: Option<String>,
		/// Reward in TESS.
		#[arg(long)]
		reward: String,
		/// The only worker allowed to accept.
		#[arg(long)]
		invited: Option<String>,
		/// Arbiter of disputes.
		#[arg(long)]
		arbiter: Option<String>,
		/// Fee of the arbiter, in TESS.
		#[arg(long, default_value = "0")]
		arbiter_fee: String,
		/// Bond of the worker, in TESS.
		#[arg(long, default_value = "0")]
		bond: String,
		/// Hours to deliver after accepting.
		#[arg(long)]
		delivery_hours: u32,
		/// Hours to approve or dispute after the delivery.
		#[arg(long)]
		review_hours: u32,
	},
	/// Cancel a task no worker accepted.
	Cancel {
		#[command(flatten)]
		signer: Signer,
		id: u64,
	},
	/// Accept a task as its worker.
	Accept {
		#[command(flatten)]
		signer: Signer,
		id: u64,
	},
	/// Deliver the result of a task.
	Deliver {
		#[command(flatten)]
		signer: Signer,
		id: u64,
		/// The result: a URI or a short text.
		result: String,
	},
	/// Approve a result: the worker is paid.
	Approve {
		#[command(flatten)]
		signer: Signer,
		id: u64,
	},
	/// Dispute a result during its review period, for tasks with an arbiter.
	Dispute {
		#[command(flatten)]
		signer: Signer,
		id: u64,
	},
	/// Decide a dispute as its arbiter.
	Resolve {
		#[arg(long)]
		from: String,
		id: u64,
		/// Share of the reward paid to the worker, in percent.
		worker_percent: u8,
	},
	/// Finalize a task whose time is up.
	Finalize {
		#[arg(long)]
		from: String,
		id: u64,
	},
}

type Result<T> = core::result::Result<T, String>;

fn error(e: impl std::fmt::Display) -> String {
	e.to_string()
}

struct Wallet {
	cli_url: String,
	json: bool,
	yes: bool,
	store: Keystore,
	client: Option<Client>,
}

fn default_dir() -> Result<PathBuf> {
	let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).ok_or("no home directory")?;
	Ok(PathBuf::from(home).join(".tesserax").join("wallet"))
}

/// A password from `TESSERAX_PASSWORD`, or typed without echo.
fn password(prompt: &str) -> Result<String> {
	if let Ok(password) = std::env::var("TESSERAX_PASSWORD") {
		return Ok(password);
	}
	rpassword::prompt_password(prompt).map_err(error)
}

fn new_password() -> Result<String> {
	if let Ok(password) = std::env::var("TESSERAX_PASSWORD") {
		return Ok(password);
	}
	let password = rpassword::prompt_password("New password: ").map_err(error)?;
	if password.len() < 8 {
		return Err("use a password of at least 8 characters".into());
	}
	if rpassword::prompt_password("Repeat the password: ").map_err(error)? != password {
		return Err("the passwords differ".into());
	}
	Ok(password)
}

fn tess(text: &str) -> Result<Amount> {
	Amount::from_tess(text).map_err(error)
}

impl Wallet {
	async fn client(&mut self) -> Result<&Client> {
		if self.client.is_none() {
			let client = Client::connect(&self.cli_url).await.map_err(|e| format!("cannot reach {}: {e}", self.cli_url))?;
			self.client = Some(client);
		}
		Ok(self.client.as_ref().expect("connected above; qed"))
	}

	/// An address, or the address of an account of the wallet.
	fn resolve(&self, who: &str) -> Result<AccountId> {
		match self.store.account(who) {
			Ok(account) => parse_address(&account.address).map_err(error),
			Err(_) => parse_address(who).map_err(|_| format!("{who} is neither an account of the wallet nor an address")),
		}
	}

	fn key(&self, name: &str) -> Result<Keypair> {
		let password = password(&format!("Password of {name}: "))?;
		self.store.keypair(name, &password).map_err(error)
	}

	/// Ask before sending `what`, unless `--yes`.
	fn confirm(&self, what: &str) -> Result<()> {
		if self.yes {
			return Ok(());
		}
		if !std::io::stdin().is_terminal() {
			return Err(format!("{what}: confirm with --yes"));
		}
		eprint!("{what}. Send? [y/N] ");
		let mut answer = String::new();
		std::io::stdin().read_line(&mut answer).map_err(error)?;
		if matches!(answer.trim(), "y" | "Y" | "yes") {
			Ok(())
		} else {
			Err("cancelled".into())
		}
	}

	fn print(&self, value: impl Serialize, text: impl FnOnce() -> String) {
		if self.json {
			println!("{}", serde_json::to_string_pretty(&value).expect("results serialize; qed"));
		} else {
			println!("{}", text());
		}
	}

	fn sent(&self, outcome: tesserax_sdk::TxOutcome) {
		let block = outcome.block_hash.clone();
		self.print(outcome, || format!("Included in block {block}"));
	}

	fn owner(&self, signer: &Signer) -> Result<Option<AccountId>> {
		signer.owner.as_deref().map(|o| self.resolve(o)).transpose()
	}
}

async fn run(cli: Cli) -> Result<()> {
	let dir = match cli.wallet_dir {
		Some(dir) => dir,
		None => default_dir()?,
	};
	let mut w = Wallet { cli_url: cli.url, json: cli.json, yes: cli.yes, store: Keystore::open(dir).map_err(error)?, client: None };

	match cli.command {
		Command::Account(command) => account(&mut w, command).await,
		Command::Chain => {
			let info = w.client().await?.chain_info().await.map_err(error)?;
			let text = format!("{} at block {}, runtime {}", info.chain, info.best_block, info.spec_version);
			w.print(info, || text);
			Ok(())
		},
		Command::Balance { who } => {
			let account = w.resolve(&who)?;
			let balance = w.client().await?.balance(&account).await.map_err(error)?;
			let text = format!(
				"{}\n  transferable {} TESS\n  free {} TESS (frozen {})\n  reserved {} TESS",
				format_address(&account),
				balance.transferable,
				balance.free,
				balance.frozen,
				balance.reserved
			);
			w.print(balance, || text);
			Ok(())
		},
		Command::Transfer { from, to, amount } => {
			let to = w.resolve(&to)?;
			let amount = tess(&amount)?;
			w.confirm(&format!("Transfer {amount} TESS from {from} to {}", format_address(&to)))?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.transfer(&key, &to, amount).await.map_err(error)?;
			w.sent(outcome);
			Ok(())
		},
		Command::Agent(command) => agent(&mut w, command).await,
		Command::Registry(command) => registry(&mut w, command).await,
		Command::Channel(command) => channel(&mut w, command).await,
		Command::Task(command) => task(&mut w, command).await,
	}
}

async fn account(w: &mut Wallet, command: AccountCommand) -> Result<()> {
	match command {
		AccountCommand::New { name } => {
			let (key, phrase) = Keypair::generate().map_err(error)?;
			let password = new_password()?;
			let account = w.store.add(&name, &phrase, &password).map_err(error)?;
			debug_assert_eq!(account.address, key.address());
			eprintln!(
				"Write down this 24-word phrase and keep it offline: it is the only way to recover the \
				 account.\n\n{phrase}\n"
			);
			let address = account.address.clone();
			w.print(account, || format!("Created {name}: {address}"));
		},
		AccountCommand::Import { name } => {
			let secret = if std::io::stdin().is_terminal() {
				rpassword::prompt_password("Secret phrase: ").map_err(error)?
			} else {
				let mut line = String::new();
				std::io::stdin().read_line(&mut line).map_err(error)?;
				line
			};
			let password = new_password()?;
			let account = w.store.add(&name, secret.trim(), &password).map_err(error)?;
			let address = account.address.clone();
			w.print(account, || format!("Imported {name}: {address}"));
		},
		AccountCommand::List => {
			let accounts = w.store.list().map_err(error)?;
			let text = accounts.iter().map(|a| format!("{}  {}", a.address, a.name)).collect::<Vec<_>>().join("\n");
			w.print(accounts, || if text.is_empty() { "No account".into() } else { text });
		},
		AccountCommand::Show { name } => {
			let account = w.store.account(&name).map_err(error)?;
			let address = account.address.clone();
			w.print(account, || address);
		},
		AccountCommand::Export { name } => {
			let password = password(&format!("Password of {name}: "))?;
			let secret = w.store.secret(&name, &password).map_err(error)?;
			eprintln!("Anyone with this phrase controls the account.");
			println!("{secret}");
		},
		AccountCommand::Remove { name } => {
			let account = w.store.account(&name).map_err(error)?;
			w.confirm(&format!("Delete {name} ({}) from the wallet; keep its phrase to recover it", account.address))?;
			// The password proves the account is known before it is deleted.
			let password = password(&format!("Password of {name}: "))?;
			w.store.secret(&name, &password).map_err(error)?;
			w.store.remove(&name).map_err(error)?;
			w.print(account, || format!("Removed {name}"));
		},
	}
	Ok(())
}

async fn agent(w: &mut Wallet, command: AgentCommand) -> Result<()> {
	match command {
		AgentCommand::Authorize {
			from,
			agent,
			budget,
			period_hours,
			max_per_call,
			expires_in_hours,
			contracts,
			no_transfers,
			targets,
		} => {
			let agent = w.resolve(&agent)?;
			let expires_at = match expires_in_hours {
				Some(hours) => Some(
					w.client().await?.best_block_number().await.map_err(error)? + hours * BLOCKS_PER_HOUR,
				),
				None => None,
			};
			let targets = targets.iter().map(|t| w.resolve(t).map(|a| format_address(&a))).collect::<Result<Vec<_>>>()?;
			let policy = Policy {
				budget: tess(&budget)?,
				period_blocks: period_hours * BLOCKS_PER_HOUR,
				max_per_call: tess(&max_per_call)?,
				expires_at,
				permissions: Permissions { transfers: !no_transfers, contracts },
				targets,
			};
			w.confirm(&format!(
				"Let agent {} spend up to {} TESS every {period_hours} h for {from}, at most {} per call",
				format_address(&agent),
				policy.budget,
				policy.max_per_call
			))?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.authorize_agent(&key, &agent, &policy).await.map_err(error)?;
			w.sent(outcome);
		},
		AgentCommand::Revoke { from, agent } => {
			let agent = w.resolve(&agent)?;
			w.confirm(&format!("Revoke agent {}", format_address(&agent)))?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.revoke_agent(&key, &agent).await.map_err(error)?;
			w.sent(outcome);
		},
		AgentCommand::Show { owner, agent } => {
			let (owner, agent) = (w.resolve(&owner)?, w.resolve(&agent)?);
			match w.client().await?.delegation(&owner, &agent).await.map_err(error)? {
				Some(d) => {
					let text = format!(
						"Budget {} TESS per {} h, at most {} per call\nSpent {} this period, {} left{}",
						d.policy.budget,
						d.policy.period_blocks / BLOCKS_PER_HOUR,
						d.policy.max_per_call,
						d.spent,
						d.remaining,
						if d.expired { " (expired)" } else { "" }
					);
					w.print(d, || text);
				},
				None => return Err("no delegation".into()),
			}
		},
	}
	Ok(())
}

async fn registry(w: &mut Wallet, command: RegistryCommand) -> Result<()> {
	match command {
		RegistryCommand::Register { from, name, description, endpoint, capabilities, owner } => {
			let owner = owner.as_deref().map(|o| w.resolve(o)).transpose()?;
			let profile = Profile { name, description, endpoint, capabilities, metadata: None };
			w.confirm(&format!("Register {from} as agent {}; a deposit is held", profile.name))?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.register_agent(&key, &profile, owner.as_ref()).await.map_err(error)?;
			w.sent(outcome);
		},
		RegistryCommand::ConfirmOwner { from, agent } => {
			let agent = w.resolve(&agent)?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.confirm_owner(&key, &agent).await.map_err(error)?;
			w.sent(outcome);
		},
		RegistryCommand::Show { agent } => {
			let agent = w.resolve(&agent)?;
			let record = w.client().await?.agent(&agent).await.map_err(error)?.ok_or("not a registered agent")?;
			let text = format!(
				"{} ({})\n{}\nEndpoint {}\nCapabilities {}\nPaid {} times by {} payers, {} TESS; rated {} good, {} bad; tasks {} completed, {} failed",
				record.profile.name,
				record.address,
				record.profile.description,
				record.profile.endpoint,
				record.profile.capabilities.join(", "),
				record.reputation.payments,
				record.reputation.payers,
				record.reputation.volume,
				record.reputation.positive,
				record.reputation.negative,
				record.reputation.tasks_completed,
				record.reputation.tasks_failed
			);
			w.print(record, || text);
		},
		RegistryCommand::Find { capability } => {
			let agents = w.client().await?.find_agents(capability.as_deref(), true).await.map_err(error)?;
			let text = agents
				.iter()
				.map(|a| format!("{}  {}  [{}]  +{}/-{}", a.address, a.profile.name, a.profile.capabilities.join(", "), a.reputation.positive, a.reputation.negative))
				.collect::<Vec<_>>()
				.join("\n");
			w.print(agents, || if text.is_empty() { "No agent".into() } else { text });
		},
		RegistryCommand::Pay { signer, agent, amount } => {
			let (agent, amount, owner) = (w.resolve(&agent)?, tess(&amount)?, w.owner(&signer)?);
			w.confirm(&format!("Pay {amount} TESS to agent {}", format_address(&agent)))?;
			let key = w.key(&signer.from)?;
			let client = w.client().await?;
			let outcome = match &owner {
				Some(owner) => client.agent_pay_agent(&key, owner, &agent, amount).await,
				None => client.pay_agent(&key, &agent, amount).await,
			}
			.map_err(error)?;
			w.sent(outcome);
		},
		RegistryCommand::Rate { from, agent, rating } => {
			let positive = match rating.as_str() {
				"good" => true,
				"bad" => false,
				_ => return Err("rate good or bad".into()),
			};
			let agent = w.resolve(&agent)?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.rate_agent(&key, &agent, positive).await.map_err(error)?;
			w.sent(outcome);
		},
	}
	Ok(())
}

async fn channel(w: &mut Wallet, command: ChannelCommand) -> Result<()> {
	match command {
		ChannelCommand::List { who } => {
			let payer = w.resolve(&who)?;
			let channels = w.client().await?.channels_of(&payer).await.map_err(error)?;
			let text = channels
				.iter()
				.map(|c| format!("#{}  to {}  deposit {}  claimed {}{}", c.id, c.payee, c.deposit, c.claimed, if c.closes_at.is_some() { "  closing" } else { "" }))
				.collect::<Vec<_>>()
				.join("\n");
			w.print(channels, || if text.is_empty() { "No channel".into() } else { text });
		},
		ChannelCommand::Show { id } => {
			let channel = w.client().await?.channel(id).await.map_err(error)?.ok_or("no such channel")?;
			let text = format!(
				"#{} from {} to {}\nDeposit {} TESS, claimed {}\nVouchers signed by {}{}",
				channel.id,
				channel.payer,
				channel.payee,
				channel.deposit,
				channel.claimed,
				channel.signer,
				channel.closes_at.map(|b| format!("\nCloses at block {b}")).unwrap_or_default()
			);
			w.print(channel, || text);
		},
		ChannelCommand::Open { signer, payee, deposit } => {
			let (payee, deposit, owner) = (w.resolve(&payee)?, tess(&deposit)?, w.owner(&signer)?);
			w.confirm(&format!("Open a channel to {} with {deposit} TESS", format_address(&payee)))?;
			let key = w.key(&signer.from)?;
			let client = w.client().await?;
			let id = match &owner {
				Some(owner) => client.agent_open_channel(&key, owner, &payee, deposit).await,
				None => client.open_channel(&key, &payee, deposit).await,
			}
			.map_err(error)?;
			w.print(serde_json::json!({ "channel": id }), || format!("Opened channel #{id}"));
		},
		ChannelCommand::TopUp { signer, id, amount } => {
			let (amount, owner) = (tess(&amount)?, w.owner(&signer)?);
			w.confirm(&format!("Add {amount} TESS to channel #{id}"))?;
			let key = w.key(&signer.from)?;
			let outcome = w.client().await?.top_up_channel(&key, owner.as_ref(), id, amount).await.map_err(error)?;
			w.sent(outcome);
		},
		ChannelCommand::Close { signer, id } => {
			let owner = w.owner(&signer)?;
			w.confirm(&format!("Request to close channel #{id}"))?;
			let key = w.key(&signer.from)?;
			let outcome = w.client().await?.request_close_channel(&key, owner.as_ref(), id).await.map_err(error)?;
			w.sent(outcome);
		},
		ChannelCommand::Finalize { from, id } => {
			let key = w.key(&from)?;
			let outcome = w.client().await?.finalize_close_channel(&key, id).await.map_err(error)?;
			w.sent(outcome);
		},
		ChannelCommand::Claim { from, voucher } => {
			let voucher = Voucher::from_header(&voucher).map_err(error)?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.claim_channel(&key, &voucher).await.map_err(error)?;
			w.sent(outcome);
		},
		ChannelCommand::CloseAsPayee { from, id } => {
			w.confirm(&format!("Close channel #{id} as its payee"))?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.close_channel(&key, id).await.map_err(error)?;
			w.sent(outcome);
		},
	}
	Ok(())
}

async fn task(w: &mut Wallet, command: TaskCommand) -> Result<()> {
	let describe = |t: &tesserax_sdk::TaskInfo| {
		format!(
			"#{}  {} TESS  [{}]  {:?}\n    {}",
			t.id,
			t.reward,
			t.capability.clone().unwrap_or_default(),
			t.status,
			t.spec
		)
	};
	match command {
		TaskCommand::List { capability } => {
			let tasks = w.client().await?.open_tasks(capability.as_deref()).await.map_err(error)?;
			let text = tasks.iter().map(describe).collect::<Vec<_>>().join("\n");
			w.print(tasks, || if text.is_empty() { "No open task".into() } else { text });
		},
		TaskCommand::Show { id } => {
			let task = w.client().await?.task(id).await.map_err(error)?.ok_or("no task in progress with this id")?;
			let text = describe(&task);
			w.print(task, || text);
		},
		TaskCommand::Create {
			signer,
			spec,
			capability,
			reward,
			invited,
			arbiter,
			arbiter_fee,
			bond,
			delivery_hours,
			review_hours,
		} => {
			let owner = w.owner(&signer)?;
			let task = NewTask {
				spec,
				capability,
				reward: tess(&reward)?,
				invited: invited.as_deref().map(|i| w.resolve(i).map(|a| format_address(&a))).transpose()?,
				arbiter: match arbiter {
					Some(a) => Some((format_address(&w.resolve(&a)?), tess(&arbiter_fee)?)),
					None => None,
				},
				bond: tess(&bond)?,
				delivery_blocks: delivery_hours * BLOCKS_PER_HOUR,
				review_blocks: review_hours * BLOCKS_PER_HOUR,
			};
			w.confirm(&format!("Order a task for {} TESS, held in escrow", task.reward))?;
			let key = w.key(&signer.from)?;
			let id = w.client().await?.create_task(&key, owner.as_ref(), &task).await.map_err(error)?;
			w.print(serde_json::json!({ "task": id }), || format!("Created task #{id}"));
		},
		TaskCommand::Cancel { signer, id } => {
			let owner = w.owner(&signer)?;
			let key = w.key(&signer.from)?;
			let outcome = w.client().await?.cancel_task(&key, owner.as_ref(), id).await.map_err(error)?;
			w.sent(outcome);
		},
		TaskCommand::Accept { signer, id } => {
			let owner = w.owner(&signer)?;
			w.confirm(&format!("Accept task #{id}; its bond is held until delivery"))?;
			let key = w.key(&signer.from)?;
			let outcome = w.client().await?.accept_task(&key, owner.as_ref(), id).await.map_err(error)?;
			w.sent(outcome);
		},
		TaskCommand::Deliver { signer, id, result } => {
			let owner = w.owner(&signer)?;
			let key = w.key(&signer.from)?;
			let outcome = w.client().await?.deliver_task(&key, owner.as_ref(), id, &result).await.map_err(error)?;
			w.sent(outcome);
		},
		TaskCommand::Approve { signer, id } => {
			let owner = w.owner(&signer)?;
			w.confirm(&format!("Approve task #{id}: the worker is paid"))?;
			let key = w.key(&signer.from)?;
			let outcome = w.client().await?.approve_task(&key, owner.as_ref(), id).await.map_err(error)?;
			w.sent(outcome);
		},
		TaskCommand::Dispute { signer, id } => {
			let owner = w.owner(&signer)?;
			w.confirm(&format!("Dispute task #{id}: its arbiter decides"))?;
			let key = w.key(&signer.from)?;
			let outcome = w.client().await?.dispute_task(&key, owner.as_ref(), id).await.map_err(error)?;
			w.sent(outcome);
		},
		TaskCommand::Resolve { from, id, worker_percent } => {
			w.confirm(&format!("Pay {worker_percent}% of the reward of task #{id} to its worker"))?;
			let key = w.key(&from)?;
			let outcome = w.client().await?.resolve_task(&key, id, worker_percent).await.map_err(error)?;
			w.sent(outcome);
		},
		TaskCommand::Finalize { from, id } => {
			let key = w.key(&from)?;
			let outcome = w.client().await?.finalize_task(&key, id).await.map_err(error)?;
			w.sent(outcome);
		},
	}
	Ok(())
}

#[tokio::main]
async fn main() {
	if let Err(message) = run(Cli::parse()).await {
		eprintln!("error: {message}");
		std::process::exit(1);
	}
}
