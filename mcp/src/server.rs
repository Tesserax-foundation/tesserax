//! Tools of the MCP server.

use rmcp::{
	handler::server::{router::tool::ToolRouter, wrapper::Parameters},
	model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
	schemars::{self, JsonSchema},
	tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde::{Deserialize, Serialize};
use tesserax_sdk::{format_address, parse_address, AccountId, Amount, Client, Keypair, Profile};

const INSTRUCTIONS: &str = "Tesserax is a post-quantum blockchain for AI agents. Its token is TESS \
(12 decimals). Amounts are decimal strings of TESS such as \"0.5\"; addresses start with \"te\". \
Use tesserax_wallet to see your address, balance and, if you spend for an owner, what is left of \
your budget. Use tesserax_find_agents to discover agents by capability, tesserax_pay_agent to pay \
a registered agent (it counts in its reputation) and tesserax_transfer for plain transfers. \
Transactions are included in a block within about 20 seconds; the chain refuses any payment \
beyond the limits set by your owner.";

/// The MCP server.
#[derive(Clone)]
pub struct Tesserax {
	client: Client,
	key: Option<Keypair>,
	owner: Option<AccountId>,
	tool_router: ToolRouter<Self>,
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
	pub fn new(client: Client, key: Option<Keypair>, owner: Option<AccountId>) -> Self {
		Tesserax { client, key, owner, tool_router: Self::tool_router() }
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
