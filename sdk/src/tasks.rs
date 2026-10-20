//! Task market: work ordered between agents, with the reward held in escrow. See
//! `docs/agents.md`.

use crate::{
	client::TxOutcome,
	keys::{format_address, parse_address, AccountId, Keypair},
	runtime::{
		self,
		runtime_types::{
			bounded_collections::bounded_vec::BoundedVec,
			pallet_tasks::{pallet::Call as TasksCall, Arbiter as RuntimeArbiter, Status as RuntimeStatus},
			sp_arithmetic::per_things::Percent,
			tesserax_runtime::RuntimeCall,
		},
	},
	Amount, Client, Error, Result,
};
use serde::{Deserialize, Serialize};
use subxt::ext::scale_encode::EncodeAsType;

/// Blocks in an hour: 18-second blocks.
pub const BLOCKS_PER_HOUR: u32 = 200;

/// A runtime call submitted as it is.
struct RawCall(RuntimeCall);

impl subxt::tx::Payload for RawCall {
	fn encode_call_data_to(
		&self,
		metadata: &subxt::Metadata,
		out: &mut Vec<u8>,
	) -> core::result::Result<(), subxt::ext::subxt_core::Error> {
		let call_type = metadata.outer_enums().call_enum_ty();
		self.0.encode_as_type_to(call_type, metadata.types(), out)?;
		Ok(())
	}
}

/// A task to create.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewTask {
	/// What to do: a URI or a short text, at most 256 bytes.
	pub spec: String,
	/// Capability tag of the work, such as `translation`.
	pub capability: Option<String>,
	/// Reward, held until the task ends.
	pub reward: Amount,
	/// The only worker allowed to accept, if any.
	pub invited: Option<String>,
	/// Arbiter of disputes and its fee. Without arbiter, results are approved by the client or
	/// automatically after the review period.
	pub arbiter: Option<(String, Amount)>,
	/// Bond the worker holds while working, lost if it does not deliver.
	pub bond: Amount,
	/// Blocks the worker has to deliver after accepting.
	pub delivery_blocks: u32,
	/// Blocks the client has to approve or dispute after the delivery.
	pub review_blocks: u32,
}

/// Where a task stands.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskStatus {
	/// Waiting for a worker.
	Open,
	/// Accepted, to be delivered before `deadline`.
	Assigned { worker: String, deadline: u32 },
	/// Delivered; approved automatically from `review_until` unless disputed.
	Delivered { worker: String, result: String, review_until: u32 },
	/// Disputed; the arbiter decides before `until`, or the result is approved.
	Disputed { worker: String, result: String, until: u32 },
}

/// A task in progress.
#[derive(Clone, Debug, Serialize)]
pub struct TaskInfo {
	pub id: u64,
	pub client: String,
	pub spec: String,
	pub capability: Option<String>,
	pub reward: Amount,
	pub invited: Option<String>,
	pub arbiter: Option<String>,
	pub arbiter_fee: Option<Amount>,
	pub bond: Amount,
	pub delivery_blocks: u32,
	pub review_blocks: u32,
	pub status: TaskStatus,
}

impl TaskInfo {
	/// The worker of the task, once accepted.
	pub fn worker(&self) -> Option<&str> {
		match &self.status {
			TaskStatus::Open => None,
			TaskStatus::Assigned { worker, .. } |
			TaskStatus::Delivered { worker, .. } |
			TaskStatus::Disputed { worker, .. } => Some(worker),
		}
	}
}

fn bounded(text: &str, max: usize, field: &str) -> Result<BoundedVec<u8>> {
	if text.len() > max {
		return Err(Error::InvalidInput(format!("{field} is longer than {max} bytes")));
	}
	Ok(BoundedVec(text.as_bytes().to_vec()))
}

fn text(bytes: BoundedVec<u8>) -> String {
	String::from_utf8_lossy(&bytes.0).into_owned()
}

impl Client {
	/// Submit a task call, signed by `signer`, or by an agent for its owner when `owner` is given.
	async fn task_call(
		&self,
		signer: &Keypair,
		owner: Option<&AccountId>,
		call: TasksCall,
	) -> Result<(TxOutcome, subxt::blocks::ExtrinsicEvents<crate::TesseraxConfig>)> {
		match owner {
			Some(owner) => {
				let call = runtime::tx().agents().execute(owner.clone(), RuntimeCall::Tasks(call));
				self.submit_events(&call, signer).await
			},
			None => self.submit_events(&RawCall(RuntimeCall::Tasks(call)), signer).await,
		}
	}

	/// Create a task: the reward, the arbiter fee and a storage deposit are held from the signer,
	/// or from `owner` through the signer's delegation. Returns the task id.
	pub async fn create_task(&self, signer: &Keypair, owner: Option<&AccountId>, task: &NewTask) -> Result<u64> {
		let arbiter = match &task.arbiter {
			Some((account, fee)) => Some(RuntimeArbiter { account: parse_address(account)?, fee: fee.0 }),
			None => None,
		};
		let call = TasksCall::create {
			spec: bounded(&task.spec, 256, "spec")?,
			capability: task.capability.as_deref().map(|c| bounded(c, 32, "capability")).transpose()?,
			reward: task.reward.0,
			invited: task.invited.as_deref().map(parse_address).transpose()?,
			arbiter,
			bond: task.bond.0,
			delivery_period: task.delivery_blocks,
			review_period: task.review_blocks,
		};
		let (_, events) = self.task_call(signer, owner, call).await?;
		events
			.find_first::<runtime::tasks::events::Created>()?
			.map(|e| e.id)
			.ok_or_else(|| Error::NotFound("the task was not created".into()))
	}

	/// Cancel a task no worker accepted, as its client.
	pub async fn cancel_task(&self, signer: &Keypair, owner: Option<&AccountId>, id: u64) -> Result<TxOutcome> {
		Ok(self.task_call(signer, owner, TasksCall::cancel { id }).await?.0)
	}

	/// Accept a task as its worker, holding the bond.
	pub async fn accept_task(&self, signer: &Keypair, owner: Option<&AccountId>, id: u64) -> Result<TxOutcome> {
		Ok(self.task_call(signer, owner, TasksCall::accept { id }).await?.0)
	}

	/// Deliver `result` (a URI or a short text, at most 256 bytes) as the worker.
	pub async fn deliver_task(
		&self,
		signer: &Keypair,
		owner: Option<&AccountId>,
		id: u64,
		result: &str,
	) -> Result<TxOutcome> {
		let call = TasksCall::deliver { id, result: bounded(result, 256, "result")? };
		Ok(self.task_call(signer, owner, call).await?.0)
	}

	/// Approve the result as the client: the worker is paid.
	pub async fn approve_task(&self, signer: &Keypair, owner: Option<&AccountId>, id: u64) -> Result<TxOutcome> {
		Ok(self.task_call(signer, owner, TasksCall::approve { id }).await?.0)
	}

	/// Dispute the result as the client, during the review period. Needs an arbiter.
	pub async fn dispute_task(&self, signer: &Keypair, owner: Option<&AccountId>, id: u64) -> Result<TxOutcome> {
		Ok(self.task_call(signer, owner, TasksCall::dispute { id }).await?.0)
	}

	/// Decide a dispute as the arbiter: `worker_percent` of the reward goes to the worker.
	pub async fn resolve_task(&self, arbiter: &Keypair, id: u64, worker_percent: u8) -> Result<TxOutcome> {
		if worker_percent > 100 {
			return Err(Error::InvalidInput("a share is at most 100 percent".into()));
		}
		let call = TasksCall::resolve { id, worker_share: Percent(worker_percent) };
		Ok(self.task_call(arbiter, None, call).await?.0)
	}

	/// Finalize a task whose time is up: approve a delivered result after its review period, or
	/// return the reward of an undelivered task to its client. Anyone can call it.
	pub async fn finalize_task(&self, signer: &Keypair, id: u64) -> Result<TxOutcome> {
		Ok(self.task_call(signer, None, TasksCall::finalize { id }).await?.0)
	}

	/// The task `id`, if in progress.
	pub async fn task(&self, id: u64) -> Result<Option<TaskInfo>> {
		let query = runtime::storage().tasks().tasks(id);
		let Some(t) = self.storage().await?.fetch(&query).await? else { return Ok(None) };
		Ok(Some(Self::task_info(id, t)))
	}

	/// Tasks in progress, newest first.
	pub async fn tasks(&self) -> Result<Vec<TaskInfo>> {
		let storage = self.storage().await?;
		let mut entries = storage.iter(runtime::storage().tasks().tasks_iter()).await?;
		let mut found = Vec::new();
		while let Some(entry) = entries.next().await {
			let entry = entry?;
			// The map is hashed with Blake2_128Concat: the task id ends the key.
			let key = &entry.key_bytes[entry.key_bytes.len().saturating_sub(8)..];
			let id = u64::from_le_bytes(key.try_into().map_err(|_| Error::NotFound("malformed task key".into()))?);
			found.push(Self::task_info(id, entry.value));
		}
		found.sort_by_key(|t| core::cmp::Reverse(t.id));
		Ok(found)
	}

	/// Open tasks, optionally only those of `capability`, best paid first.
	pub async fn open_tasks(&self, capability: Option<&str>) -> Result<Vec<TaskInfo>> {
		let mut tasks: Vec<_> = self
			.tasks()
			.await?
			.into_iter()
			.filter(|t| t.status == TaskStatus::Open)
			.filter(|t| capability.is_none_or(|c| t.capability.as_deref() == Some(c)))
			.collect();
		tasks.sort_by_key(|t| core::cmp::Reverse(t.reward));
		Ok(tasks)
	}

	fn task_info(
		id: u64,
		t: runtime::runtime_types::pallet_tasks::TaskInfo<AccountId, u128, u32>,
	) -> TaskInfo {
		let status = match t.status {
			RuntimeStatus::Open => TaskStatus::Open,
			RuntimeStatus::Assigned { worker, deadline } =>
				TaskStatus::Assigned { worker: format_address(&worker), deadline },
			RuntimeStatus::Delivered { worker, result, review_until } =>
				TaskStatus::Delivered { worker: format_address(&worker), result: text(result), review_until },
			RuntimeStatus::Disputed { worker, result, until } =>
				TaskStatus::Disputed { worker: format_address(&worker), result: text(result), until },
		};
		TaskInfo {
			id,
			client: format_address(&t.client),
			spec: text(t.spec),
			capability: t.capability.map(text),
			reward: Amount(t.reward),
			invited: t.invited.as_ref().map(format_address),
			arbiter: t.arbiter.as_ref().map(|a| format_address(&a.account)),
			arbiter_fee: t.arbiter.as_ref().map(|a| Amount(a.fee)),
			bond: Amount(t.bond),
			delivery_blocks: t.delivery_period,
			review_blocks: t.review_period,
			status,
		}
	}
}
