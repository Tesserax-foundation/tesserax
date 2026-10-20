//! Tools of the MCP server.

use rmcp::{
	handler::server::{router::tool::ToolRouter, wrapper::Parameters},
	model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
	schemars::{self, JsonSchema},
	tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tesserax_sdk::{
	format_address, parse_address, AccountId, Amount, ChannelPayer, Client, Keypair,
	PaymentRequired, Profile, PAYMENT_HEADER, VOUCHER_HEADER,
};

/// Largest response body returned to the agent, in bytes.
const MAX_BODY: usize = 100_000;

/// Payments of HTTP services over payment channels.
pub struct Payments {
	payer: Option<ChannelPayer>,
	max_price: Amount,
	http: reqwest::Client,
}

impl Payments {
	pub async fn new(
		client: &Client,
		key: Option<&Keypair>,
		owner: Option<AccountId>,
		deposit: Amount,
		max_price: Amount,
		file: Option<PathBuf>,
	) -> Result<Self, Box<dyn std::error::Error>> {
		let payer = match (key, file) {
			(Some(key), Some(path)) =>
				Some(ChannelPayer::with_file(client.clone(), key.clone(), owner, deposit, path)?),
			(Some(key), None) => Some(ChannelPayer::new(client.clone(), key.clone(), owner, deposit)),
			(None, _) => None,
		};
		let http = reqwest::Client::builder().user_agent("tesserax-mcp").build()?;
		Ok(Payments { payer, max_price, http })
	}
}

const INSTRUCTIONS: &str = "Tesserax is a post-quantum blockchain for AI agents. Its token is TESS \
(12 decimals). Amounts are decimal strings of TESS such as \"0.5\"; addresses start with \"te\". \
Use tesserax_wallet to see your address, balance and, if you spend for an owner, what is left of \
your budget. Use tesserax_find_agents to discover agents by capability, tesserax_pay_agent to pay \
a registered agent (it counts in its reputation) and tesserax_transfer for plain transfers. \
Use tesserax_fetch to call HTTP services that charge per request: it pays them with \
micropayments, without a transaction per request. The task market lets agents order work from \
each other with the reward held in escrow: tesserax_find_tasks lists work you can do, \
tesserax_create_task orders work. \
Transactions are included in a block within about 20 seconds; the chain refuses any payment \
beyond the limits set by your owner.";

/// The MCP server.
#[derive(Clone)]
pub struct Tesserax {
	client: Client,
	key: Option<Keypair>,
	owner: Option<AccountId>,
	payments: Arc<Payments>,
	tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, JsonSchema)]
pub struct FetchParams {
	/// URL of the request.
	url: String,
	/// HTTP method, GET by default.
	method: Option<String>,
	/// Request body, if any.
	body: Option<String>,
	/// Request headers.
	#[serde(default)]
	headers: HashMap<String, String>,
	/// Highest price accepted for this request, in TESS; the server's maximum by default.
	max_price: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TaskId {
	/// Identifier of the task.
	id: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct FindTasksParams {
	/// Only tasks of this capability, such as "translation".
	capability: Option<String>,
	/// Maximum number of tasks returned, 20 by default.
	limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateTaskParams {
	/// What to do: a URI or a short text, at most 256 bytes.
	spec: String,
	/// Capability tag of the work, such as "translation", so that workers find it.
	capability: Option<String>,
	/// Reward in TESS, held in escrow until the task ends.
	reward: String,
	/// Address of the only worker allowed to accept, if any.
	invited: Option<String>,
	/// Address of an arbiter for disputes. Without arbiter, delivered results are paid when you
	/// approve them or automatically after the review period.
	arbiter: Option<String>,
	/// Fee of the arbiter in TESS, paid only if it decides a dispute.
	arbiter_fee: Option<String>,
	/// Bond in TESS the worker holds, lost if it does not deliver in time; 0 by default.
	bond: Option<String>,
	/// Minutes the worker has to deliver after accepting, at least 3.
	delivery_minutes: u32,
	/// Minutes you have to approve or dispute after the delivery, at least 3.
	review_minutes: u32,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeliverParams {
	/// Identifier of the task.
	id: u64,
	/// The result: a URI or a short text, at most 256 bytes.
	result: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ResolveParams {
	/// Identifier of the disputed task.
	id: u64,
	/// Share of the reward paid to the worker, from 0 to 100; the rest returns to the client.
	worker_percent: u8,
}

#[derive(Serialize)]
struct Created {
	id: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct ChannelParams {
	/// Identifier of the channel.
	channel: u64,
}

#[derive(Serialize)]
struct FetchResult {
	status: u16,
	content_type: Option<String>,
	body: String,
	truncated: bool,
	payment: Option<Payment>,
}

#[derive(Serialize)]
struct Payment {
	payee: String,
	channel: u64,
	price: Amount,
	/// Cumulative amount paid on the channel.
	total: Amount,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddressParams {
	/// Tesserax address, starting with "te".
	address: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TransferParams {
	/// Recipient address, starting with "te".
	to: String,
	/// Amount of TESS, as a decimal string such as "1.5".
	amount: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct PayAgentParams {
	/// Address of the registered agent to pay.
	agent: String,
	/// Amount of TESS, as a decimal string such as "0.25".
	amount: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RateParams {
	/// Address of an agent you paid.
	agent: String,
	/// Whether the service was good.
	positive: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct FindParams {
	/// Only agents offering this capability, such as "translation".
	capability: Option<String>,
	/// Also list inactive agents.
	#[serde(default)]
	include_inactive: bool,
	/// Maximum number of agents returned, 20 by default.
	limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DelegationParams {
	/// Address of the owner.
	owner: String,
	/// Address of the agent.
	agent: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ProfileParams {
	/// Display name, at most 64 bytes.
	name: String,
	/// What the agent does, at most 512 bytes.
	description: String,
	/// Where its service is reached, such as an HTTPS or MCP URL.
	endpoint: String,
	/// Capabilities as lowercase tags such as "translation" or "code-review", at most 16.
	capabilities: Vec<String>,
	/// Hexadecimal 32-byte hash of an off-chain description, if any.
	metadata: Option<String>,
	/// Address of the owner of the agent, who must confirm. Only used when registering.
	owner: Option<String>,
}

impl From<&ProfileParams> for Profile {
	fn from(p: &ProfileParams) -> Self {
		Profile {
			name: p.name.clone(),
			description: p.description.clone(),
			endpoint: p.endpoint.clone(),
			capabilities: p.capabilities.clone(),
			metadata: p.metadata.clone(),
		}
	}
}

#[derive(Serialize)]
struct Wallet {
	address: String,
	balance: tesserax_sdk::Balance,
	mode: &'static str,
	owner: Option<String>,
	delegation: Option<tesserax_sdk::Delegation>,
}

type ToolResult = Result<CallToolResult, ErrorData>;

/// Blocks of 18 seconds covering `minutes`.
fn blocks(minutes: u32) -> u32 {
	minutes.saturating_mul(60).div_ceil(18)
}

fn ok(value: impl Serialize) -> ToolResult {
	let text = serde_json::to_string_pretty(&value).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
	Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// A failure the agent should see and can act on.
fn fail(message: impl ToString) -> ToolResult {
	Ok(CallToolResult::error(vec![ContentBlock::text(message.to_string())]))
}

macro_rules! attempt {
	($e:expr) => {
		match $e {
			Ok(value) => value,
			Err(error) => return fail(error),
		}
	};
}

impl Tesserax {
	pub fn new(client: Client, key: Option<Keypair>, owner: Option<AccountId>, payments: Payments) -> Self {
		Tesserax { client, key, owner, payments: Arc::new(payments), tool_router: Self::tool_router() }
	}

	async fn send(&self, p: &FetchParams, voucher: Option<String>) -> Result<reqwest::Response, String> {
		let method = reqwest::Method::from_bytes(p.method.as_deref().unwrap_or("GET").to_uppercase().as_bytes())
			.map_err(|_| "invalid HTTP method".to_string())?;
		let mut request = self.payments.http.request(method, &p.url);
		for (name, value) in &p.headers {
			request = request.header(name, value);
		}
		if let Some(body) = &p.body {
			request = request.body(body.clone());
		}
		if let Some(voucher) = voucher {
			request = request.header(VOUCHER_HEADER, voucher);
		}
		request.send().await.map_err(|e| format!("request failed: {e}"))
	}

	/// The account that pays and gets paid: the owner in delegated mode, the key otherwise.
	fn acting_account(&self) -> Result<AccountId, String> {
		Ok(match &self.owner {
			Some(owner) => owner.clone(),
			None => self.key()?.account_id(),
		})
	}

	fn key(&self) -> Result<&Keypair, String> {
		self.key
			.as_ref()
			.ok_or_else(|| "no key configured: set TESSERAX_KEY to send transactions".to_string())
	}
}

#[tool_router]
impl Tesserax {
	#[tool(description = "Information about the Tesserax chain: name, best block, token and address format.")]
	async fn tesserax_chain_info(&self) -> ToolResult {
		ok(attempt!(self.client.chain_info().await))
	}

	#[tool(description = "Your wallet: address, balance, and whether you spend your own funds or \
		an owner's funds within a delegation, with what is left of your budget.")]
	async fn tesserax_wallet(&self) -> ToolResult {
		let key = attempt!(self.key());
		let account = key.account_id();
		let delegation = match &self.owner {
			Some(owner) => attempt!(self.client.delegation(owner, &account).await),
			None => None,
		};
		ok(Wallet {
			address: key.address(),
			balance: attempt!(self.client.balance(&account).await),
			mode: if self.owner.is_some() { "delegated" } else { "own funds" },
			owner: self.owner.as_ref().map(format_address),
			delegation,
		})
	}

	#[tool(description = "Balance of a Tesserax address, in TESS.")]
	async fn tesserax_balance(&self, Parameters(p): Parameters<AddressParams>) -> ToolResult {
		let account = attempt!(parse_address(&p.address));
		ok(attempt!(self.client.balance(&account).await))
	}

	#[tool(description = "Transfer TESS to an address. When spending for an owner, the chain \
		checks the budget, the limit per payment and the allowed recipients.")]
	async fn tesserax_transfer(&self, Parameters(p): Parameters<TransferParams>) -> ToolResult {
		let key = attempt!(self.key());
		let to = attempt!(parse_address(&p.to));
		let amount = attempt!(Amount::from_tess(&p.amount));
		let outcome = match &self.owner {
			Some(owner) => self.client.agent_transfer(key, owner, &to, amount).await,
			None => self.client.transfer(key, &to, amount).await,
		};
		ok(attempt!(outcome))
	}

	#[tool(description = "Pay a registered agent for its service. The payment counts in its \
		reputation. When spending for an owner, the chain checks the delegation limits.")]
	async fn tesserax_pay_agent(&self, Parameters(p): Parameters<PayAgentParams>) -> ToolResult {
		let key = attempt!(self.key());
		let agent = attempt!(parse_address(&p.agent));
		let amount = attempt!(Amount::from_tess(&p.amount));
		let outcome = match &self.owner {
			Some(owner) => self.client.agent_pay_agent(key, owner, &agent, amount).await,
			None => self.client.pay_agent(key, &agent, amount).await,
		};
		ok(attempt!(outcome))
	}

	#[tool(description = "Rate an agent you paid with your own funds: one rating per payment.")]
	async fn tesserax_rate_agent(&self, Parameters(p): Parameters<RateParams>) -> ToolResult {
		if self.owner.is_some() {
			return fail("payments made for your owner can only be rated by the owner");
		}
		let key = attempt!(self.key());
		let agent = attempt!(parse_address(&p.agent));
		ok(attempt!(self.client.rate_agent(key, &agent, p.positive).await))
	}

	#[tool(description = "Find registered agents, optionally by capability, best rated first, \
		with their endpoint and reputation.")]
	async fn tesserax_find_agents(&self, Parameters(p): Parameters<FindParams>) -> ToolResult {
		let mut agents = attempt!(
			self.client.find_agents(p.capability.as_deref(), !p.include_inactive).await
		);
		agents.truncate(p.limit.unwrap_or(20).min(100));
		ok(agents)
	}

	#[tool(description = "A registered agent: profile, owner, endpoint and reputation.")]
	async fn tesserax_get_agent(&self, Parameters(p): Parameters<AddressParams>) -> ToolResult {
		let account = attempt!(parse_address(&p.address));
		match attempt!(self.client.agent(&account).await) {
			Some(agent) => ok(agent),
			None => fail(format!("{} is not a registered agent", p.address)),
		}
	}

	#[tool(description = "The delegation of an owner to an agent: budget, limits, what was spent \
		and what is left.")]
	async fn tesserax_delegation(&self, Parameters(p): Parameters<DelegationParams>) -> ToolResult {
		let owner = attempt!(parse_address(&p.owner));
		let agent = attempt!(parse_address(&p.agent));
		match attempt!(self.client.delegation(&owner, &agent).await) {
			Some(d) => ok(d),
			None => fail("no delegation from this owner to this agent"),
		}
	}

	#[tool(description = "Make an HTTP request, paying the service in TESS if it asks for it: on a \
		402 answer with a Tesserax-Payment header, pays the price with a voucher on a payment channel \
		to the service (opened or topped up on chain when needed) and repeats the request. Refuses \
		prices above max_price.")]
	async fn tesserax_fetch(&self, Parameters(p): Parameters<FetchParams>) -> ToolResult {
		let max_price = match &p.max_price {
			Some(text) => attempt!(Amount::from_tess(text)).min(self.payments.max_price),
			None => self.payments.max_price,
		};
		let mut response = attempt!(self.send(&p, None).await);
		let mut payment = None;
		if response.status() == reqwest::StatusCode::PAYMENT_REQUIRED {
			let Some(header) = response.headers().get(PAYMENT_HEADER).and_then(|v| v.to_str().ok()) else {
				return fail("the service asks for a payment without a Tesserax-Payment header");
			};
			let requirement = attempt!(PaymentRequired::from_header(header));
			let Some(payer) = &self.payments.payer else {
				return fail("no key configured: set TESSERAX_KEY to pay services");
			};
			let voucher = attempt!(payer.pay(&requirement, max_price).await);
			payment = Some(Payment {
				payee: requirement.payee.clone(),
				channel: voucher.channel,
				price: requirement.price,
				total: voucher.amount,
			});
			response = attempt!(self.send(&p, Some(voucher.to_header())).await);
			if response.status() == reqwest::StatusCode::PAYMENT_REQUIRED {
				let reason = response
					.headers()
					.get(PAYMENT_HEADER)
					.and_then(|v| v.to_str().ok())
					.and_then(|v| PaymentRequired::from_header(v).ok())
					.and_then(|r| r.error)
					.unwrap_or_else(|| "no reason given".into());
				return fail(format!("the service refused the payment: {reason}"));
			}
		}
		let status = response.status().as_u16();
		let content_type =
			response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(String::from);
		let bytes = attempt!(response.bytes().await.map_err(|e| format!("cannot read the response: {e}")));
		let truncated = bytes.len() > MAX_BODY;
		let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY)]).into_owned();
		ok(FetchResult { status, content_type, body, truncated, payment })
	}

	#[tool(description = "Your payment channels: the services you pay, deposits and amounts paid.")]
	async fn tesserax_channels(&self) -> ToolResult {
		let key = attempt!(self.key());
		let payer = self.owner.clone().unwrap_or_else(|| key.account_id());
		let mut channels = attempt!(self.client.channels_of(&payer).await);
		channels.retain(|c| c.signer == key.address());
		ok(channels)
	}

	#[tool(description = "Request to close one of your payment channels: after 6 hours, during \
		which the service can claim what it was paid, the rest of the deposit returns.")]
	async fn tesserax_close_channel(&self, Parameters(p): Parameters<ChannelParams>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.request_close_channel(key, self.owner.as_ref(), p.channel).await))
	}

	#[tool(description = "Open tasks of the task market, best paid first: work you can accept to \
		earn TESS. The reward is held in escrow by the chain.")]
	async fn tesserax_find_tasks(&self, Parameters(p): Parameters<FindTasksParams>) -> ToolResult {
		let mut tasks = attempt!(self.client.open_tasks(p.capability.as_deref()).await);
		tasks.truncate(p.limit.unwrap_or(20).min(100));
		ok(tasks)
	}

	#[tool(description = "A task of the task market: specification, reward, status, worker, result.")]
	async fn tesserax_get_task(&self, Parameters(p): Parameters<TaskId>) -> ToolResult {
		match attempt!(self.client.task(p.id).await) {
			Some(task) => ok(task),
			None => fail(format!("task {} is not in progress", p.id)),
		}
	}

	#[tool(description = "Your tasks in progress: those you ordered and those you work on.")]
	async fn tesserax_my_tasks(&self) -> ToolResult {
		let me = format_address(&attempt!(self.acting_account()));
		let tasks = attempt!(self.client.tasks().await);
		ok(tasks.into_iter().filter(|t| t.client == me || t.worker() == Some(me.as_str())).collect::<Vec<_>>())
	}

	#[tool(description = "Order work: create a task whose reward is held in escrow by the chain. \
		A worker accepts and delivers; you approve, or the result is approved automatically after \
		the review period. With an arbiter you can dispute a result during the review period.")]
	async fn tesserax_create_task(&self, Parameters(p): Parameters<CreateTaskParams>) -> ToolResult {
		let key = attempt!(self.key());
		let reward = attempt!(Amount::from_tess(&p.reward));
		let bond = attempt!(Amount::from_tess(p.bond.as_deref().unwrap_or("0")));
		let arbiter = match (&p.arbiter, &p.arbiter_fee) {
			(Some(arbiter), fee) => Some((arbiter.clone(), attempt!(Amount::from_tess(fee.as_deref().unwrap_or("0"))))),
			(None, Some(_)) => return fail("an arbiter fee needs an arbiter"),
			(None, None) => None,
		};
		let task = tesserax_sdk::NewTask {
			spec: p.spec,
			capability: p.capability,
			reward,
			invited: p.invited,
			arbiter,
			bond,
			delivery_blocks: blocks(p.delivery_minutes),
			review_blocks: blocks(p.review_minutes),
		};
		ok(Created { id: attempt!(self.client.create_task(key, self.owner.as_ref(), &task).await) })
	}

	#[tool(description = "Cancel a task you ordered that no worker accepted: the reward returns.")]
	async fn tesserax_cancel_task(&self, Parameters(p): Parameters<TaskId>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.cancel_task(key, self.owner.as_ref(), p.id).await))
	}

	#[tool(description = "Accept an open task as its worker, holding its bond. Deliver before the \
		deadline or lose the bond.")]
	async fn tesserax_accept_task(&self, Parameters(p): Parameters<TaskId>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.accept_task(key, self.owner.as_ref(), p.id).await))
	}

	#[tool(description = "Deliver the result of a task you accepted: a URI or a short text.")]
	async fn tesserax_deliver_task(&self, Parameters(p): Parameters<DeliverParams>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.deliver_task(key, self.owner.as_ref(), p.id, &p.result).await))
	}

	#[tool(description = "Approve the result of a task you ordered: the worker is paid.")]
	async fn tesserax_approve_task(&self, Parameters(p): Parameters<TaskId>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.approve_task(key, self.owner.as_ref(), p.id).await))
	}

	#[tool(description = "Dispute the result of a task you ordered, during its review period. \
		Only tasks with an arbiter can be disputed; the arbiter decides how the reward is shared.")]
	async fn tesserax_dispute_task(&self, Parameters(p): Parameters<TaskId>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.dispute_task(key, self.owner.as_ref(), p.id).await))
	}

	#[tool(description = "Decide a dispute as the arbiter of a task: the share of the reward paid \
		to the worker, from 0 to 100 percent. You receive the arbiter fee.")]
	async fn tesserax_resolve_task(&self, Parameters(p): Parameters<ResolveParams>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.resolve_task(key, p.id, p.worker_percent).await))
	}

	#[tool(description = "Finalize a task whose time is up: a delivered result past its review \
		period is paid to the worker, an undelivered task past its deadline returns the reward and \
		the worker's bond to the client.")]
	async fn tesserax_finalize_task(&self, Parameters(p): Parameters<TaskId>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.finalize_task(key, p.id).await))
	}

	#[tool(description = "Register yourself in the agent registry so that other agents can find \
		and pay you. A small deposit is held from your key while registered.")]
	async fn tesserax_register_agent(&self, Parameters(p): Parameters<ProfileParams>) -> ToolResult {
		let key = attempt!(self.key());
		let owner = attempt!(p.owner.as_deref().map(parse_address).transpose());
		ok(attempt!(self.client.register_agent(key, &Profile::from(&p), owner.as_ref()).await))
	}

	#[tool(description = "Replace your profile in the agent registry.")]
	async fn tesserax_update_profile(&self, Parameters(p): Parameters<ProfileParams>) -> ToolResult {
		let key = attempt!(self.key());
		ok(attempt!(self.client.update_profile(key, &Profile::from(&p)).await))
	}
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Tesserax {
	fn get_info(&self) -> ServerConfig {
		ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
			.with_server_info(Implementation::new("tesserax-mcp", env!("CARGO_PKG_VERSION")))
			.with_instructions(INSTRUCTIONS)
	}
}
