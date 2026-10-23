//! Client of a Tesserax node.

use crate::{
	keys::{format_address, AccountId, Keypair},
	runtime::{
		self,
		runtime_types::{
			bounded_collections::bounded_vec::BoundedVec,
			pallet_agent_registry::{pallet::Call as RegistryCall, Profile as RuntimeProfile},
			pallet_agents::{Permissions as RuntimePermissions, Policy as RuntimePolicy},
			pallet_balances::pallet::Call as BalancesCall,
			tesserax_runtime::RuntimeCall,
		},
	},
	Amount, Error, Result, TesseraxConfig,
};
use serde::{Deserialize, Serialize};
use subxt::{
	backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
	config::DefaultExtrinsicParamsBuilder,
	tx::{Payload, TxStatus},
	utils::{MultiAddress, H256},
	OnlineClient,
};

/// Number of blocks a transaction stays valid, from the best block: about 19 minutes.
const MORTALITY: u64 = 64;

/// Information about the chain.
#[derive(Clone, Debug, Serialize)]
pub struct ChainInfo {
	pub chain: String,
	pub best_block: u32,
	pub spec_version: u32,
	pub token: &'static str,
	pub decimals: u32,
	pub ss58_prefix: u16,
}

/// Balance of an account.
#[derive(Clone, Debug, Serialize)]
pub struct Balance {
	/// Free balance, including what is frozen.
	pub free: Amount,
	/// Held as deposits.
	pub reserved: Amount,
	/// Part of the free balance that cannot be moved, such as vesting.
	pub frozen: Amount,
	/// What can be transferred now.
	pub transferable: Amount,
}

/// Result of an included transaction.
#[derive(Clone, Debug, Serialize)]
pub struct TxOutcome {
	/// Block that includes the transaction.
	pub block_hash: String,
	pub block_number: u32,
	/// Hash of the transaction.
	pub extrinsic_hash: String,
}

/// Limits of a delegation from an owner to an agent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Policy {
	/// Maximum spent per period.
	pub budget: Amount,
	/// Length of a period, in blocks of 18 seconds (4 800 is a day).
	pub period_blocks: u32,
	/// Maximum spent by a single call.
	pub max_per_call: Amount,
	/// Block from which the delegation is no longer valid.
	pub expires_at: Option<u32>,
	/// Permissions.
	pub permissions: Permissions,
	/// Recipients and contracts allowed, as addresses; any if empty.
	pub targets: Vec<String>,
}

/// Kinds of calls an agent may make.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Permissions {
	pub transfers: bool,
	pub contracts: bool,
}

/// A delegation and its use.
#[derive(Clone, Debug, Serialize)]
pub struct Delegation {
	pub owner: String,
	pub agent: String,
	pub policy: Policy,
	/// Spent in the current period.
	pub spent: Amount,
	/// First block of the current period.
	pub period_start: u32,
	/// What the agent can still spend in the current period.
	pub remaining: Amount,
	/// Whether the delegation has expired.
	pub expired: bool,
}

/// Public description of an agent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Profile {
	pub name: String,
	pub description: String,
	/// Where the service of the agent is reached, such as an HTTPS or MCP URL.
	pub endpoint: String,
	/// Lowercase tags such as `translation`.
	pub capabilities: Vec<String>,
	/// Hexadecimal hash of an off-chain description, if any.
	pub metadata: Option<String>,
}

/// Reputation of an agent.
#[derive(Clone, Debug, Serialize, Default)]
pub struct Reputation {
	pub payments: u64,
	pub volume: Amount,
	pub payers: u32,
	pub positive: u32,
	pub negative: u32,
	pub tasks_completed: u32,
	pub tasks_failed: u32,
}

/// A registered agent.
#[derive(Clone, Debug, Serialize)]
pub struct AgentRecord {
	pub address: String,
	pub owner: Option<String>,
	pub owner_confirmed: bool,
	pub active: bool,
	pub registered_at: u32,
	pub profile: Profile,
	pub reputation: Reputation,
}

/// A connection to a Tesserax node.
#[derive(Clone)]
pub struct Client {
	api: OnlineClient<TesseraxConfig>,
	rpc: LegacyRpcMethods<TesseraxConfig>,
}

fn text(bytes: BoundedVec<u8>) -> String {
	String::from_utf8_lossy(&bytes.0).into_owned()
}

fn bounded(text: &str, max: usize, field: &str) -> Result<BoundedVec<u8>> {
	if text.len() > max {
		return Err(Error::InvalidInput(format!("{field} is longer than {max} bytes")));
	}
	Ok(BoundedVec(text.as_bytes().to_vec()))
}

fn hex(bytes: &[u8]) -> String {
	format!("0x{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

impl Profile {
	fn to_runtime(&self) -> Result<RuntimeProfile> {
		if self.capabilities.len() > 16 {
			return Err(Error::InvalidInput("at most 16 capabilities".into()));
		}
		let metadata = match &self.metadata {
			None => None,
			Some(h) => Some(
				h.strip_prefix("0x")
					.and_then(crate::keys::hex_bytes)
					.and_then(|b| <[u8; 32]>::try_from(b).ok())
					.ok_or_else(|| Error::InvalidInput("metadata must be a 32-byte hex hash".into()))?,
			),
		};
		Ok(RuntimeProfile {
			name: bounded(&self.name, 64, "name")?,
			description: bounded(&self.description, 512, "description")?,
			endpoint: bounded(&self.endpoint, 256, "endpoint")?,
			capabilities: BoundedVec(
				self.capabilities.iter().map(|c| bounded(c, 32, "capability")).collect::<Result<_>>()?,
			),
			metadata,
		})
	}

	fn from_runtime(p: RuntimeProfile) -> Self {
		Profile {
			name: text(p.name),
			description: text(p.description),
			endpoint: text(p.endpoint),
			capabilities: p.capabilities.0.into_iter().map(text).collect(),
			metadata: p.metadata.map(|m| hex(&m)),
		}
	}
}

impl Policy {
	fn to_runtime(&self) -> Result<RuntimePolicy<AccountId, u128, u32>> {
		Ok(RuntimePolicy {
			budget: self.budget.0,
			period: self.period_blocks,
			max_per_call: self.max_per_call.0,
			expires_at: self.expires_at,
			permissions: RuntimePermissions {
				transfers: self.permissions.transfers,
				contracts: self.permissions.contracts,
			},
			targets: BoundedVec(self.targets.iter().map(|t| crate::parse_address(t)).collect::<Result<_>>()?),
		})
	}

	fn from_runtime(p: RuntimePolicy<AccountId, u128, u32>) -> Self {
		Policy {
			budget: Amount(p.budget),
			period_blocks: p.period,
			max_per_call: Amount(p.max_per_call),
			expires_at: p.expires_at,
			permissions: Permissions { transfers: p.permissions.transfers, contracts: p.permissions.contracts },
			targets: p.targets.0.iter().map(format_address).collect(),
		}
	}
}

/// TLS of the `wss://` connections: TLS 1.3, ML-KEM-768 in every key exchange.
fn post_quantum_tls() -> Result<rustls::ClientConfig> {
	use rustls::crypto::{aws_lc_rs, CryptoProvider};
	let provider = std::sync::Arc::new(CryptoProvider {
		kx_groups: vec![aws_lc_rs::kx_group::MLKEM768, aws_lc_rs::kx_group::X25519MLKEM768],
		..aws_lc_rs::default_provider()
	});
	let tls = |e: rustls::Error| Error::InvalidInput(format!("TLS: {e}"));
	let verifier = rustls_platform_verifier::Verifier::new(provider.clone()).map_err(tls)?;
	Ok(rustls::ClientConfig::builder_with_provider(provider)
		.with_protocol_versions(&[&rustls::version::TLS13])
		.map_err(tls)?
		.dangerous()
		.with_custom_certificate_verifier(std::sync::Arc::new(verifier))
		.with_no_client_auth())
}

/// A websocket client of `url` over post-quantum TLS, configured as `subxt` configures its own.
async fn post_quantum_client(url: &str) -> Result<jsonrpsee::core::client::Client> {
	use jsonrpsee::client_transport::ws::{Url, WsTransportClientBuilder};
	let node = |e: String| Error::Node(subxt::Error::Other(format!("{url}: {e}")));
	let parsed = Url::parse(url).map_err(|e| node(e.to_string()))?;
	let (sender, receiver) = WsTransportClientBuilder::default()
		.with_custom_cert_store(post_quantum_tls()?)
		.build(parsed)
		.await
		.map_err(|e| node(e.to_string()))?;
	Ok(jsonrpsee::core::client::Client::builder()
		.max_buffer_capacity_per_subscription(4096)
		.build_with_tokio(sender, receiver))
}

impl Client {
	/// Connect to the node at `url`, such as `wss://rpc.tesserax.app:21207`, or `ws://127.0.0.1:9944`.
	///
	/// `wss://` connections use TLS 1.3 with ML-KEM-768, alone or in the hybrid X25519MLKEM768, and
	/// the certificate authorities of the system: a server without ML-KEM is refused. `ws://` is
	/// not encrypted: use it on the machine of the node or a private network only.
	pub async fn connect(url: &str) -> Result<Self> {
		let rpc = if url.starts_with("wss://") {
			RpcClient::new(post_quantum_client(url).await?)
		} else {
			RpcClient::from_insecure_url(url).await.map_err(subxt::Error::from)?
		};
		let api = OnlineClient::<TesseraxConfig>::from_rpc_client(rpc.clone()).await?;
		Ok(Client { api, rpc: LegacyRpcMethods::new(rpc) })
	}

	/// The underlying `subxt` client.
	pub fn api(&self) -> &OnlineClient<TesseraxConfig> {
		&self.api
	}

	/// Information about the chain.
	pub async fn chain_info(&self) -> Result<ChainInfo> {
		let (_, best_block) = self.best().await?;
		let chain = self.rpc.system_chain().await.map_err(subxt::Error::from)?;
		Ok(ChainInfo {
			chain,
			best_block,
			spec_version: self.api.runtime_version().spec_version,
			token: "TESS",
			decimals: crate::amount::DECIMALS,
			ss58_prefix: crate::SS58_PREFIX,
		})
	}

	/// Hash and number of the best block. Proof-of-work blocks are final only 60 blocks deep, so
	/// the client reads the state of the best block and builds transactions on top of it.
	pub(crate) async fn best(&self) -> Result<(H256, u32)> {
		let rpc_error = |e| Error::Node(subxt::Error::from(e));
		let hash = self
			.rpc
			.chain_get_block_hash(None)
			.await
			.map_err(rpc_error)?
			.ok_or_else(|| Error::NotFound("best block".into()))?;
		let header = self
			.rpc
			.chain_get_header(Some(hash))
			.await
			.map_err(rpc_error)?
			.ok_or_else(|| Error::NotFound("best block header".into()))?;
		Ok((hash, header.number))
	}

	/// Hash of the block `number` of the best chain, if the chain has it.
	pub(crate) async fn block_hash(&self, number: u32) -> Result<Option<H256>> {
		self.rpc.chain_get_block_hash(Some(number.into())).await.map_err(|e| Error::Node(subxt::Error::from(e)))
	}

	async fn best_block(&self) -> Result<u32> {
		Ok(self.best().await?.1)
	}

	/// Number of the best block.
	pub async fn best_block_number(&self) -> Result<u32> {
		self.best_block().await
	}

	pub(crate) async fn storage(
		&self,
	) -> Result<subxt::storage::Storage<TesseraxConfig, OnlineClient<TesseraxConfig>>> {
		let (hash, _) = self.best().await?;
		Ok(self.api.storage().at(hash))
	}

	/// Balance of `account`.
	pub async fn balance(&self, account: &AccountId) -> Result<Balance> {
		let query = runtime::storage().system().account(account.clone());
		let info = self.storage().await?.fetch_or_default(&query).await?;
		let data = info.data;
		Ok(Balance {
			free: Amount(data.free),
			reserved: Amount(data.reserved),
			frozen: Amount(data.frozen),
			transferable: Amount(data.free.saturating_sub(data.frozen)),
		})
	}

	/// Sign `call` with `signer`, submit it and wait until a block includes it successfully.
	pub async fn submit<Call: Payload>(&self, call: &Call, signer: &Keypair) -> Result<TxOutcome> {
		Ok(self.submit_events(call, signer).await?.0)
	}

	/// Like [`Client::submit`], also returning the events of the transaction.
	pub async fn submit_events<Call: Payload>(
		&self,
		call: &Call,
		signer: &Keypair,
	) -> Result<(TxOutcome, subxt::blocks::ExtrinsicEvents<TesseraxConfig>)> {
		let nonce = self.rpc.system_account_next_index(&signer.account_id()).await.map_err(subxt::Error::from)?;
		let (hash, number) = self.best().await?;
		let params = DefaultExtrinsicParamsBuilder::<TesseraxConfig>::new()
			.nonce(nonce)
			.mortal_from_unchecked(MORTALITY, number.into(), hash)
			.build();
		let progress = self.api.tx().sign_and_submit_then_watch(call, signer, params).await?;
		self.included(progress).await
	}

	/// Wait until a block includes the transaction of `progress` successfully.
	pub(crate) async fn included(
		&self,
		mut progress: subxt::tx::TxProgress<TesseraxConfig, OnlineClient<TesseraxConfig>>,
	) -> Result<(TxOutcome, subxt::blocks::ExtrinsicEvents<TesseraxConfig>)> {
		let in_block = loop {
			match progress.next().await {
				Some(Ok(TxStatus::InBestBlock(tx) | TxStatus::InFinalizedBlock(tx))) => break tx,
				Some(Ok(
					TxStatus::Error { message } | TxStatus::Invalid { message } | TxStatus::Dropped { message },
				)) => return Err(Error::Dispatch(message)),
				Some(Ok(_)) => continue,
				Some(Err(error)) => return Err(error.into()),
				None => return Err(Error::Dispatch("the node stopped following the transaction".into())),
			}
		};
		let block_hash = in_block.block_hash();
		let extrinsic_hash = in_block.extrinsic_hash();
		let events = in_block.wait_for_success().await.map_err(|e| match e {
			subxt::Error::Runtime(error) => Error::Dispatch(error.to_string()),
			other => Error::Node(other),
		})?;
		let block_number = self
			.rpc
			.chain_get_header(Some(block_hash))
			.await
			.map_err(|e| Error::Node(subxt::Error::from(e)))?
			.map_or(0, |header| header.number);
		let outcome = TxOutcome {
			block_hash: hex(block_hash.as_ref()),
			block_number,
			extrinsic_hash: hex(extrinsic_hash.as_ref()),
		};
		Ok((outcome, events))
	}

	/// Transfer `amount` from `signer` to `to`, keeping the signer's account alive.
	pub async fn transfer(&self, signer: &Keypair, to: &AccountId, amount: Amount) -> Result<TxOutcome> {
		let call = runtime::tx().balances().transfer_keep_alive(MultiAddress::Id(to.clone()), amount.0);
		self.submit(&call, signer).await
	}

	// Delegations.

	/// Delegate to `agent` within `policy`, signed by the owner.
	pub async fn authorize_agent(&self, owner: &Keypair, agent: &AccountId, policy: &Policy) -> Result<TxOutcome> {
		let call = runtime::tx().agents().authorize(agent.clone(), policy.to_runtime()?);
		self.submit(&call, owner).await
	}

	/// Revoke the delegation to `agent`, signed by the owner.
	pub async fn revoke_agent(&self, owner: &Keypair, agent: &AccountId) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agents().revoke(agent.clone()), owner).await
	}

	/// Give up the delegation of `owner`, signed by the agent.
	pub async fn renounce_delegation(&self, agent: &Keypair, owner: &AccountId) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agents().renounce(owner.clone()), agent).await
	}

	/// The delegation of `owner` to `agent`, if any.
	pub async fn delegation(&self, owner: &AccountId, agent: &AccountId) -> Result<Option<Delegation>> {
		let query = runtime::storage().agents().delegations(owner.clone(), agent.clone());
		let storage = self.storage().await?;
		let Some(d) = storage.fetch(&query).await? else { return Ok(None) };
		let now = self.best_block().await?;
		let policy = Policy::from_runtime(d.policy);
		let expired = policy.expires_at.is_some_and(|e| now >= e);
		let remaining = if expired {
			0
		} else if now >= d.period_start.saturating_add(policy.period_blocks) {
			policy.budget.0
		} else {
			policy.budget.0.saturating_sub(d.spent)
		};
		Ok(Some(Delegation {
			owner: format_address(owner),
			agent: format_address(agent),
			policy,
			spent: Amount(d.spent),
			period_start: d.period_start,
			remaining: Amount(remaining),
			expired,
		}))
	}

	/// Make `call` for `owner`, signed by the agent, within its delegation.
	pub async fn agent_execute(&self, agent: &Keypair, owner: &AccountId, call: RuntimeCall) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agents().execute(owner.clone(), call), agent).await
	}

	/// Transfer `amount` from `owner` to `to`, signed by the agent, within its delegation.
	pub async fn agent_transfer(
		&self,
		agent: &Keypair,
		owner: &AccountId,
		to: &AccountId,
		amount: Amount,
	) -> Result<TxOutcome> {
		let call = RuntimeCall::Balances(BalancesCall::transfer_keep_alive {
			dest: MultiAddress::Id(to.clone()),
			value: amount.0,
		});
		self.agent_execute(agent, owner, call).await
	}

	/// Pay the registered agent `payee` from `owner`'s funds, signed by the agent, within its
	/// delegation. The payment counts in the payee's reputation and entitles the owner to rate it.
	pub async fn agent_pay_agent(
		&self,
		agent: &Keypair,
		owner: &AccountId,
		payee: &AccountId,
		amount: Amount,
	) -> Result<TxOutcome> {
		let call = RuntimeCall::AgentRegistry(RegistryCall::pay { agent: payee.clone(), amount: amount.0 });
		self.agent_execute(agent, owner, call).await
	}

	// Registry.

	/// Register `signer` as an agent, optionally naming its owner.
	pub async fn register_agent(
		&self,
		signer: &Keypair,
		profile: &Profile,
		owner: Option<&AccountId>,
	) -> Result<TxOutcome> {
		let call = runtime::tx().agent_registry().register(profile.to_runtime()?, owner.cloned());
		self.submit(&call, signer).await
	}

	/// Replace the profile of `signer`.
	pub async fn update_profile(&self, signer: &Keypair, profile: &Profile) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agent_registry().update_profile(profile.to_runtime()?), signer).await
	}

	/// Confirm owning `agent`, signed by the owner it named.
	pub async fn confirm_owner(&self, owner: &Keypair, agent: &AccountId) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agent_registry().confirm_owner(agent.clone()), owner).await
	}

	/// Activate or deactivate `agent`, signed by the agent or its confirmed owner.
	pub async fn set_agent_active(&self, signer: &Keypair, agent: &AccountId, active: bool) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agent_registry().set_active(agent.clone(), active), signer).await
	}

	/// Pay `amount` to the registered agent `agent`, signed by the payer.
	pub async fn pay_agent(&self, payer: &Keypair, agent: &AccountId, amount: Amount) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agent_registry().pay(agent.clone(), amount.0), payer).await
	}

	/// Rate `agent` once for a payment made to it.
	pub async fn rate_agent(&self, payer: &Keypair, agent: &AccountId, positive: bool) -> Result<TxOutcome> {
		self.submit(&runtime::tx().agent_registry().rate(agent.clone(), positive), payer).await
	}

	async fn reputation(&self, agent: &AccountId) -> Result<Reputation> {
		let query = runtime::storage().agent_registry().reputations(agent.clone());
		let r = self.storage().await?.fetch_or_default(&query).await?;
		Ok(Reputation {
			payments: r.payments,
			volume: Amount(r.volume),
			payers: r.payers,
			positive: r.positive,
			negative: r.negative,
			tasks_completed: r.tasks_completed,
			tasks_failed: r.tasks_failed,
		})
	}

	/// The registered agent `agent`, if any, with its reputation.
	pub async fn agent(&self, agent: &AccountId) -> Result<Option<AgentRecord>> {
		let query = runtime::storage().agent_registry().agents(agent.clone());
		let Some(info) = self.storage().await?.fetch(&query).await? else {
			return Ok(None);
		};
		Ok(Some(AgentRecord {
			address: format_address(agent),
			owner: info.owner.as_ref().map(format_address),
			owner_confirmed: info.owner_confirmed,
			active: info.active,
			registered_at: info.registered_at,
			profile: Profile::from_runtime(info.profile),
			reputation: self.reputation(agent).await?,
		}))
	}

	/// Registered agents, optionally only the active ones offering `capability`, best rated first.
	pub async fn find_agents(&self, capability: Option<&str>, active_only: bool) -> Result<Vec<AgentRecord>> {
		let storage = self.storage().await?;
		let mut entries = storage.iter(runtime::storage().agent_registry().agents_iter()).await?;
		let mut found = Vec::new();
		while let Some(entry) = entries.next().await {
			let entry = entry?;
			let info = entry.value;
			if active_only && !info.active {
				continue;
			}
			let profile = Profile::from_runtime(info.profile);
			if capability.is_some_and(|c| !profile.capabilities.iter().any(|p| p == c)) {
				continue;
			}
			// The map is hashed with Blake2_128Concat: the account ends the key.
			let key = &entry.key_bytes[entry.key_bytes.len().saturating_sub(32)..];
			let account =
				AccountId::from(<[u8; 32]>::try_from(key).map_err(|_| Error::NotFound("malformed agent key".into()))?);
			found.push(AgentRecord {
				address: format_address(&account),
				owner: info.owner.as_ref().map(format_address),
				owner_confirmed: info.owner_confirmed,
				active: info.active,
				registered_at: info.registered_at,
				profile,
				reputation: self.reputation(&account).await?,
			});
		}
		found.sort_by_key(|a| {
			let r = &a.reputation;
			core::cmp::Reverse((r.positive as i64 - r.negative as i64, r.payers, r.volume))
		});
		Ok(found)
	}
}
