//! Smart contracts: Solidity compiled to PolkaVM with `resolc`, deployed and called with ML-DSA
//! signed transactions, their arguments, results and events encoded with the Solidity ABI.
//!
//! ```no_run
//! # async fn example() -> Result<(), tesserax_sdk::Error> {
//! use tesserax_sdk::{contracts::{h160, DynSolValue, JsonAbi, U256}, Client, Keypair};
//!
//! let client = Client::connect("ws://127.0.0.1:9944").await?;
//! let alice = Keypair::from_uri("//Alice")?;
//! let code = std::fs::read("Token.polkavm").unwrap();
//! let abi: JsonAbi = serde_json::from_str(&std::fs::read_to_string("Token.abi.json").unwrap()).unwrap();
//!
//! let args = [DynSolValue::String("Tesserax Coin".into()), DynSolValue::Uint(U256::from(1_000_000u64), 256)];
//! let (token, _) = client.deploy_contract(&alice, &code, abi, &args, Default::default()).await?;
//! let bob = Keypair::from_uri("//Bob")?;
//! token.write(&alice, "transfer", &[DynSolValue::Address(h160(&bob.account_id())), DynSolValue::Uint(U256::from(10u64), 256)]).await?;
//! let balance = token.read("balanceOf", &[DynSolValue::Address(h160(&bob.account_id()))]).await?;
//! # Ok(()) }
//! ```
//!
//! Reads are dry runs: the node executes the call on the state of the best block and returns its
//! result, without a transaction or a fee. Writes are dry run first, which sizes their weight and
//! storage deposit and stops a call that would revert before it costs a fee.

use crate::{
	client::TxOutcome,
	keys::{AccountId, Keypair},
	runtime::{
		self,
		revive::events::{ContractEmitted, Instantiated},
		runtime_types::{
			pallet_revive::primitives::{Code, StorageDeposit},
			sp_runtime::DispatchError as RuntimeDispatchError,
			sp_weights::weight_v2::Weight,
		},
	},
	Amount, Client, Error, Result, TesseraxConfig,
};
use alloy_core::{dyn_abi::DynSolType, primitives::keccak256};
pub use alloy_core::{
	dyn_abi::{DynSolValue, EventExt, FunctionExt, JsonAbiExt},
	json_abi::JsonAbi,
	primitives::{Address, Bytes, B256, I256, U256},
};
use std::sync::Arc;
use subxt::utils::H160;

/// Weight and storage deposit of a transaction are those of its dry run, plus this margin in
/// percent: the state may change between the dry run and the block that includes it.
const MARGIN_PERCENT: u128 = 10;

/// Blocks [`Contract::events`] reads at most at once.
pub const MAX_EVENT_RANGE: u32 = 10_000;

/// Address of an account in contracts: its `msg.sender`, and what `address` arguments name. The
/// last 20 bytes of the Keccak-256 hash of the account id, as the runtime maps it.
pub fn h160(account: &AccountId) -> Address {
	let bytes: &[u8; 32] = account.as_ref();
	// Accounts derived from an Ethereum address end with twelve 0xEE bytes: they map back to it.
	if bytes[20..].iter().all(|b| *b == 0xEE) {
		return Address::from_slice(&bytes[..20]);
	}
	Address::from_slice(&keccak256(bytes)[12..])
}

/// A contract address from its hexadecimal form, `0x` and 40 digits.
pub fn parse_h160(text: &str) -> Result<Address> {
	text.parse().map_err(|_| Error::InvalidAddress(format!("{text} is not a contract address")))
}

/// Options of a deployment.
#[derive(Clone, Debug, Default)]
pub struct Deploy {
	/// Sent to the contract with its constructor, which must then be `payable`.
	pub value: Amount,
	/// With a salt, the address of the contract depends only on the deployer, the code, the
	/// constructor arguments and the salt (CREATE2); without, on the deployer and its nonce.
	pub salt: Option<[u8; 32]>,
}

/// Result of a dry run.
#[derive(Clone, Debug)]
pub struct DryRun {
	/// Data returned, or the revert data when `reverted`.
	pub output: Vec<u8>,
	pub reverted: bool,
	/// Weight the call needs: time (picoseconds) and proof size (bytes).
	pub weight: (u64, u64),
	/// Most storage deposit the call charges at once.
	pub storage_deposit: Amount,
	/// Address of the contract, for a deployment.
	pub contract: Option<Address>,
}

/// An event a contract emitted, decoded with its ABI when it declares it.
#[derive(Clone, Debug)]
pub struct ContractEvent {
	pub contract: Address,
	/// Name of the event, when the ABI declares it.
	pub name: Option<String>,
	/// Arguments by name, indexed ones included, in the order the event declares them.
	pub args: Vec<(String, DynSolValue)>,
	pub topics: Vec<B256>,
	pub data: Vec<u8>,
	pub block_number: u32,
	pub block_hash: String,
}

/// Result of a transaction to a contract.
#[derive(Clone, Debug)]
pub struct ContractTx {
	pub outcome: TxOutcome,
	/// Events emitted by the contracts the transaction called.
	pub events: Vec<ContractEvent>,
}

/// A deployed contract and its ABI.
#[derive(Clone)]
pub struct Contract {
	client: Client,
	address: Address,
	abi: Arc<JsonAbi>,
}

fn abi_error(error: impl std::fmt::Display) -> Error {
	Error::InvalidInput(format!("ABI: {error}"))
}

fn h160_bytes(address: Address) -> H160 {
	H160(address.into_array())
}

fn with_margin(value: u128) -> u128 {
	value.saturating_add(value.saturating_mul(MARGIN_PERCENT).div_ceil(100))
}

fn hex(bytes: &[u8]) -> String {
	format!("0x{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The reason a call reverted, from its revert data: `Error(string)`, `Panic(uint256)`, or an
/// error the ABI declares.
pub fn revert_reason(abi: Option<&JsonAbi>, data: &[u8]) -> String {
	if data.len() < 4 {
		return if data.is_empty() { "reverted without a reason".into() } else { format!("reverted: {}", hex(data)) };
	}
	let (selector, args) = data.split_at(4);
	let single = |ty: DynSolType| match DynSolType::Tuple(vec![ty]).abi_decode_sequence(args) {
		Ok(DynSolValue::Tuple(mut values)) if values.len() == 1 => values.pop(),
		_ => None,
	};
	match selector {
		[0x08, 0xc3, 0x79, 0xa0] => {
			if let Some(DynSolValue::String(reason)) = single(DynSolType::String) {
				return reason;
			}
		},
		[0x4e, 0x48, 0x7b, 0x71] => {
			if let Some(DynSolValue::Uint(code, _)) = single(DynSolType::Uint(256)) {
				return format!("panic {code:#x}{}", panic_meaning(code));
			}
		},
		_ => {
			if let Some(error) = abi.and_then(|abi| abi.errors().find(|e| e.selector().as_slice() == selector)) {
				if let Ok(values) = error.abi_decode_input(args) {
					let fields = error
						.inputs
						.iter()
						.zip(values)
						.map(|(param, value)| format!("{}: {}", param.name, display(&value)))
						.collect::<Vec<_>>();
					return format!("{}({})", error.name, fields.join(", "));
				}
			}
		},
	}
	format!("reverted: {}", hex(data))
}

fn panic_meaning(code: U256) -> &'static str {
	match code.saturating_to::<u64>() {
		0x01 => " (assertion failed)",
		0x11 => " (arithmetic overflow or underflow)",
		0x12 => " (division by zero)",
		0x21 => " (invalid enum value)",
		0x22 => " (invalid storage byte array)",
		0x31 => " (pop on an empty array)",
		0x32 => " (index out of bounds)",
		0x41 => " (out of memory)",
		0x51 => " (call to an uninitialized function)",
		_ => "",
	}
}

/// A value as text: numbers in decimal, addresses and bytes in hexadecimal.
pub fn display(value: &DynSolValue) -> String {
	match value {
		DynSolValue::Bool(b) => b.to_string(),
		DynSolValue::Int(i, _) => i.to_string(),
		DynSolValue::Uint(u, _) => u.to_string(),
		DynSolValue::Address(a) => a.to_checksum(None),
		DynSolValue::String(s) => format!("{s:?}"),
		DynSolValue::Bytes(b) => hex(b),
		DynSolValue::FixedBytes(b, size) => hex(&b[..*size]),
		DynSolValue::Array(values) | DynSolValue::FixedArray(values) | DynSolValue::Tuple(values) => {
			format!("[{}]", values.iter().map(display).collect::<Vec<_>>().join(", "))
		},
		other => format!("{other:?}"),
	}
}

impl Client {
	/// The contract at `address`, called through `abi`.
	pub fn contract(&self, address: Address, abi: JsonAbi) -> Contract {
		Contract { client: self.clone(), address, abi: Arc::new(abi) }
	}

	/// Name of a dispatch error, such as `Revive::ContractTrapped`.
	fn dispatch_error(&self, error: RuntimeDispatchError) -> Error {
		if let RuntimeDispatchError::Module(module) = &error {
			let metadata = self.api().metadata();
			let name = metadata.pallet_by_index(module.index).and_then(|pallet| {
				pallet
					.error_variant_by_index(module.error[0])
					.map(|variant| format!("{}::{}", pallet.name(), variant.name))
			});
			if let Some(name) = name {
				return Error::Dispatch(name);
			}
		}
		Error::Dispatch(format!("{error:?}"))
	}

	/// Dry run of a call of `contract` with `data` by `origin`, on the state of the best block.
	pub async fn dry_run_call(
		&self,
		origin: &AccountId,
		contract: Address,
		value: Amount,
		data: Vec<u8>,
	) -> Result<DryRun> {
		let (hash, _) = self.best().await?;
		let call = runtime::apis().revive_api().call(origin.clone(), h160_bytes(contract), value.0, None, None, data);
		let result = self.api().runtime_api().at(hash).call(call).await?;
		let output = result.result.map_err(|e| self.dispatch_error(e))?;
		Ok(DryRun {
			reverted: output.flags.bits & 1 == 1,
			output: output.data,
			weight: (result.weight_required.ref_time, result.weight_required.proof_size),
			storage_deposit: Amount(charge(&result.max_storage_deposit)),
			contract: None,
		})
	}

	/// Dry run of the deployment of `code` with the constructor input `data` by `origin`.
	pub async fn dry_run_deploy(
		&self,
		origin: &AccountId,
		code: &[u8],
		data: Vec<u8>,
		options: &Deploy,
	) -> Result<DryRun> {
		let (hash, _) = self.best().await?;
		let call = runtime::apis().revive_api().instantiate(
			origin.clone(),
			options.value.0,
			None,
			None,
			Code::Upload(code.to_vec()),
			data,
			options.salt,
		);
		let result = self.api().runtime_api().at(hash).call(call).await?;
		let output = result.result.map_err(|e| self.dispatch_error(e))?;
		Ok(DryRun {
			reverted: output.result.flags.bits & 1 == 1,
			output: output.result.data,
			weight: (result.weight_required.ref_time, result.weight_required.proof_size),
			storage_deposit: Amount(charge(&result.max_storage_deposit)),
			contract: Some(Address::from(output.addr.0)),
		})
	}

	/// Deploy `code`, a contract compiled to PolkaVM, with the constructor arguments `args` of
	/// `abi`, signed by `signer`. Returns the contract once a block includes the deployment.
	pub async fn deploy_contract(
		&self,
		signer: &Keypair,
		code: &[u8],
		abi: JsonAbi,
		args: &[DynSolValue],
		options: Deploy,
	) -> Result<(Contract, ContractTx)> {
		let data = match &abi.constructor {
			Some(constructor) => constructor.abi_encode_input(args).map_err(abi_error)?,
			None if args.is_empty() => Vec::new(),
			None => return Err(Error::InvalidInput("the contract has no constructor to take arguments".into())),
		};
		let dry = self.dry_run_deploy(&signer.account_id(), code, data.clone(), &options).await?;
		if dry.reverted {
			return Err(Error::ContractReverted(revert_reason(Some(&abi), &dry.output)));
		}
		let call = runtime::tx().revive().instantiate_with_code(
			options.value.0,
			weight(dry.weight),
			with_margin(dry.storage_deposit.0),
			code.to_vec(),
			data,
			options.salt,
		);
		let (outcome, events) = self.submit_events(&call, signer).await?;
		let address = events
			.find_first::<Instantiated>()?
			.map(|e| Address::from(e.contract.0))
			.or(dry.contract)
			.ok_or_else(|| Error::NotFound("address of the deployed contract".into()))?;
		let contract = self.contract(address, abi);
		let events = contract.emitted(&outcome, &events)?;
		Ok((contract, ContractTx { outcome, events }))
	}

	/// Call `contract` with the encoded input `data`, signed by `signer`, after a dry run that
	/// stops a call that would revert.
	pub async fn call_contract(
		&self,
		signer: &Keypair,
		contract: Address,
		value: Amount,
		data: Vec<u8>,
		abi: Option<&JsonAbi>,
	) -> Result<(TxOutcome, subxt::blocks::ExtrinsicEvents<TesseraxConfig>)> {
		let dry = self.dry_run_call(&signer.account_id(), contract, value, data.clone()).await?;
		if dry.reverted {
			return Err(Error::ContractReverted(revert_reason(abi, &dry.output)));
		}
		let call = runtime::tx().revive().call(
			h160_bytes(contract),
			value.0,
			weight(dry.weight),
			with_margin(dry.storage_deposit.0),
			data,
		);
		self.submit_events(&call, signer).await
	}

	/// Events emitted by `contract`, if given, in the blocks `from` to `to` of the best chain, as
	/// raw events. The node must keep the state of these blocks, as an archive node does.
	pub async fn contract_events(
		&self,
		contract: Option<Address>,
		from: u32,
		to: u32,
	) -> Result<Vec<(u32, String, ContractEmitted)>> {
		if to < from || to - from >= MAX_EVENT_RANGE {
			return Err(Error::InvalidInput(format!("read at most {MAX_EVENT_RANGE} blocks at once, from <= to")));
		}
		let mut found = Vec::new();
		for number in from..=to {
			let Some(hash) = self.block_hash(number).await? else { break };
			for event in self.api().events().at(hash).await?.find::<ContractEmitted>() {
				let event = event?;
				if contract.is_none_or(|c| c.as_slice() == event.contract.0.as_slice()) {
					found.push((number, hex(hash.as_ref()), event));
				}
			}
		}
		Ok(found)
	}
}

fn charge(deposit: &StorageDeposit<u128>) -> u128 {
	match deposit {
		StorageDeposit::Charge(amount) => *amount,
		StorageDeposit::Refund(_) => 0,
	}
}

fn weight((ref_time, proof_size): (u64, u64)) -> Weight {
	let add = |value: u64| u64::try_from(with_margin(value.into())).unwrap_or(u64::MAX);
	Weight { ref_time: add(ref_time), proof_size: add(proof_size) }
}

impl Contract {
	pub fn address(&self) -> Address {
		self.address
	}

	pub fn abi(&self) -> &JsonAbi {
		&self.abi
	}

	/// The function `name` of the ABI: a name, or a signature such as `transfer(address,uint256)`
	/// for an overloaded one; otherwise the overload taking `arguments` arguments.
	fn function(&self, name: &str, arguments: usize) -> Result<&alloy_core::json_abi::Function> {
		if name.contains('(') {
			return self
				.abi
				.functions()
				.find(|f| f.signature() == name)
				.ok_or_else(|| Error::InvalidInput(format!("no function {name} in the ABI")));
		}
		let overloads =
			self.abi.function(name).ok_or_else(|| Error::InvalidInput(format!("no function {name} in the ABI")))?;
		let mut matching = overloads.iter().filter(|f| f.inputs.len() == arguments);
		match (matching.next(), matching.next()) {
			(Some(function), None) => Ok(function),
			(Some(_), Some(_)) => Err(Error::InvalidInput(format!("{name} is overloaded: name it by its signature"))),
			(None, _) => Err(Error::InvalidInput(format!("{name} takes {} arguments", overloads[0].inputs.len()))),
		}
	}

	/// Input of a call of `function` with `args`: its selector and its encoded arguments.
	pub fn encode(&self, function: &str, args: &[DynSolValue]) -> Result<Vec<u8>> {
		self.function(function, args.len())?.abi_encode_input(args).map_err(abi_error)
	}

	/// Values `function` returned in `output`.
	pub fn decode_output(&self, function: &str, arguments: usize, output: &[u8]) -> Result<Vec<DynSolValue>> {
		self.function(function, arguments)?.abi_decode_output(output).map_err(abi_error)
	}

	/// Call `function` with `args` without a transaction, as the zero account, and return what it
	/// returns. For a function that depends on its caller, use [`Contract::read_as`].
	pub async fn read(&self, function: &str, args: &[DynSolValue]) -> Result<Vec<DynSolValue>> {
		self.read_as(&AccountId::from([0u8; 32]), function, args).await
	}

	/// Like [`Contract::read`], with `caller` as `msg.sender`.
	pub async fn read_as(&self, caller: &AccountId, function: &str, args: &[DynSolValue]) -> Result<Vec<DynSolValue>> {
		let data = self.encode(function, args)?;
		let dry = self.client.dry_run_call(caller, self.address, Amount(0), data).await?;
		if dry.reverted {
			return Err(Error::ContractReverted(revert_reason(Some(&self.abi), &dry.output)));
		}
		self.decode_output(function, args.len(), &dry.output)
	}

	/// Call `function` with `args` in a transaction signed by `signer`, and return once a block
	/// includes it, with the events it emitted.
	pub async fn write(&self, signer: &Keypair, function: &str, args: &[DynSolValue]) -> Result<ContractTx> {
		self.write_with_value(signer, function, args, Amount(0)).await
	}

	/// Like [`Contract::write`], sending `value` to a `payable` function.
	pub async fn write_with_value(
		&self,
		signer: &Keypair,
		function: &str,
		args: &[DynSolValue],
		value: Amount,
	) -> Result<ContractTx> {
		let data = self.encode(function, args)?;
		let (outcome, events) = self.client.call_contract(signer, self.address, value, data, Some(&self.abi)).await?;
		let events = self.emitted(&outcome, &events)?;
		Ok(ContractTx { outcome, events })
	}

	/// The event of `topics` and `data`, decoded with the ABI: its name and its arguments.
	pub fn decode_event(&self, topics: &[B256], data: &[u8]) -> Option<(String, Vec<(String, DynSolValue)>)> {
		let first = topics.first()?;
		let event = self.abi.events().find(|e| !e.anonymous && e.selector() == *first)?;
		let decoded = event.decode_log_parts(topics.iter().copied(), data).ok()?;
		let (mut indexed, mut body) = (decoded.indexed.into_iter(), decoded.body.into_iter());
		let args = event
			.inputs
			.iter()
			.map(|param| {
				let value = if param.indexed { indexed.next() } else { body.next() };
				value.map(|value| (param.name.clone(), value))
			})
			.collect::<Option<Vec<_>>>()?;
		Some((event.name.clone(), args))
	}

	fn event(&self, block_number: u32, block_hash: String, raw: ContractEmitted) -> ContractEvent {
		let topics: Vec<B256> = raw.topics.iter().map(|t| B256::from(t.0)).collect();
		let decoded = self.decode_event(&topics, &raw.data);
		let (name, args) = decoded.map_or((None, Vec::new()), |(name, args)| (Some(name), args));
		ContractEvent {
			contract: Address::from(raw.contract.0),
			name,
			args,
			topics,
			data: raw.data,
			block_number,
			block_hash,
		}
	}

	fn emitted(
		&self,
		outcome: &TxOutcome,
		events: &subxt::blocks::ExtrinsicEvents<TesseraxConfig>,
	) -> Result<Vec<ContractEvent>> {
		let number = outcome.block_number;
		events
			.find::<ContractEmitted>()
			.map(|event| Ok(self.event(number, outcome.block_hash.clone(), event?)))
			.collect()
	}

	/// Events this contract emitted in the blocks `from` to `to` of the best chain, at most
	/// [`MAX_EVENT_RANGE`] blocks at once.
	pub async fn events(&self, from: u32, to: u32) -> Result<Vec<ContractEvent>> {
		Ok(self
			.client
			.contract_events(Some(self.address), from, to)
			.await?
			.into_iter()
			.map(|(number, hash, raw)| self.event(number, hash, raw))
			.collect())
	}

	/// Events this contract emits from now on, block after block of the best chain. A block of
	/// the best chain may still leave it, with its events, until it is 60 blocks deep.
	pub async fn watch_events(&self) -> Result<impl futures::Stream<Item = Result<ContractEvent>> + '_> {
		use futures::StreamExt;
		// The subscription starts with the current best block, whose events are past.
		let (_, start) = self.client.best().await?;
		let blocks = self.client.api().blocks().subscribe_best().await?;
		Ok(blocks
			.then(move |block| async move {
				let block = match block {
					Ok(block) => block,
					Err(e) => return vec![Err(Error::from(e))],
				};
				let (number, hash) = (block.number(), hex(block.hash().as_ref()));
				if number <= start {
					return Vec::new();
				}
				match block.events().await {
					Err(e) => vec![Err(Error::from(e))],
					Ok(events) => events
						.find::<ContractEmitted>()
						.filter_map(|event| match event {
							Ok(event) if event.contract.0 == self.address.into_array() => {
								Some(Ok(self.event(number, hash.clone(), event)))
							},
							Ok(_) => None,
							Err(e) => Some(Err(Error::from(e))),
						})
						.collect(),
				}
			})
			.flat_map(futures::stream::iter))
	}
}
