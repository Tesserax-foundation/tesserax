use codec::Decode;
use frame_support::{
	derive_impl,
	dispatch::DispatchClass,
	parameter_types,
	traits::{
		fungible::{Balanced, Credit},
		ConstBool, ConstU128, ConstU32, ConstU64, ConstU8, Contains, FindAuthor, OnUnbalanced,
		VariantCountOf, WithdrawReasons,
	},
	PalletId,
	weights::{
		constants::{RocksDbWeight, WEIGHT_REF_TIME_PER_SECOND},
		IdentityFee, Weight,
	},
};
use frame_system::limits::{BlockLength, BlockWeights};
use pallet_transaction_payment::{ConstFeeMultiplier, FungibleAdapter, Multiplier};
use sp_core::U256;
use frame_system::EnsureSigned;
use pallet_revive::{AccountId32Mapper, AddressMapper};
use sp_runtime::{
	traits::{ConvertInto, One},
	ConsensusEngineId, FixedU128, Perbill,
};
use sp_version::RuntimeVersion;

use super::weights::{self, block_weights::BlockExecutionWeight, extrinsic_weights::ExtrinsicBaseWeight};
use super::{
	precompiles::MlDsa87Verify, AccountId, Balance, Balances, Block, BlockNumber, Hash, Nonce,
	PalletInfo, Runtime, RuntimeCall, RuntimeEvent, RuntimeFreezeReason, RuntimeHoldReason,
	AgentRegistry, RuntimeOrigin, RuntimeTask, System, Timestamp, Vesting, BLOCK_REWARD, DAYS, EXISTENTIAL_DEPOSIT, HALVING_INTERVAL,
	HOURS, MILLI_SECS_PER_BLOCK, MILLI_UNIT, MINING_SUPPLY, SS58_PREFIX, UNIT, VERSION,
};

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

/// Share of the block weight expected to be used by `on_initialize` hooks.
const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(10);

/// Two seconds of compute per block. Blocks must stay fast to import: every second spent
/// executing a block delays its propagation and raises the orphan rate.
const MAXIMUM_BLOCK_WEIGHT: Weight =
	Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX);

parameter_types! {
	pub const BlockHashCount: BlockNumber = 2400;
	pub const Version: RuntimeVersion = VERSION;

	/// Base weights benchmarked with ML-DSA transaction signatures.
	pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
		.base_block(BlockExecutionWeight::get())
		.for_class(DispatchClass::all(), |weights| {
			weights.base_extrinsic = ExtrinsicBaseWeight::get();
		})
		.for_class(DispatchClass::Normal, |weights| {
			weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
		})
		.for_class(DispatchClass::Operational, |weights| {
			weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
			// Operational transactions have some extra reserved space, so that they
			// are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
			weights.reserved = Some(
				MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT,
			);
		})
		.avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
		.build_or_panic();
	pub RuntimeBlockLength: BlockLength = BlockLength::builder()
		.max_length(5 * 1024 * 1024)
		.modify_max_length_for_class(DispatchClass::Normal, |m| *m = NORMAL_DISPATCH_RATIO * *m)
		.build();
	pub const SS58Prefix: u16 = SS58_PREFIX;
}

#[derive_impl(frame_system::config_preludes::SolochainDefaultConfig)]
impl frame_system::Config for Runtime {
	type Block = Block;
	type BlockWeights = RuntimeBlockWeights;
	type BlockLength = RuntimeBlockLength;
	type AccountId = AccountId;
	type Nonce = Nonce;
	type Hash = Hash;
	type BlockHashCount = BlockHashCount;
	type DbWeight = RocksDbWeight;
	type Version = Version;
	type AccountData = pallet_balances::AccountData<Balance>;
	type SS58Prefix = SS58Prefix;
	type MaxConsumers = ConstU32<16>;
	type BaseCallFilter = NoEthereumTransactions;
	type SystemWeightInfo = weights::frame_system::WeightInfo<Runtime>;
	type ExtensionsWeightInfo = weights::frame_system_extensions::WeightInfo<Runtime>;
	/// Accounts get their contract address mapping on creation.
	type OnNewAccount = pallet_revive::AutoMapper<Runtime>;
	type OnKilledAccount = pallet_revive::AutoMapper<Runtime>;
}

/// Rejects the Ethereum transaction calls of `pallet_revive`. Ethereum transactions are signed
/// with ECDSA: contracts are deployed and called with ML-DSA signed extrinsics only.
pub struct NoEthereumTransactions;

impl Contains<RuntimeCall> for NoEthereumTransactions {
	fn contains(call: &RuntimeCall) -> bool {
		!matches!(
			call,
			RuntimeCall::Revive(
				pallet_revive::Call::eth_transact { .. } |
					pallet_revive::Call::eth_call { .. } |
					pallet_revive::Call::eth_instantiate_with_code { .. } |
					pallet_revive::Call::eth_substrate_call { .. }
			)
		)
	}
}

/// The miner of a block, read from the proof-of-work pre-runtime digest.
pub struct PowAuthor;

impl FindAuthor<AccountId> for PowAuthor {
	fn find_author<'a, I>(digests: I) -> Option<AccountId>
	where
		I: 'a + IntoIterator<Item = (ConsensusEngineId, &'a [u8])>,
	{
		digests.into_iter().find_map(|(id, mut data)| {
			(id == sp_consensus_pow::POW_ENGINE_ID).then(|| AccountId::decode(&mut data).ok())?
		})
	}
}

/// Sends value that would otherwise be burned to the miner of the current block: transaction fees
/// and tips, the dust of accounts removed below the existential deposit, the rounding of contract
/// gas, and the deposits governance slashes. Nothing is burned.
pub struct ToAuthor;

impl OnUnbalanced<Credit<AccountId, Balances>> for ToAuthor {
	fn on_nonzero_unbalanced(amount: Credit<AccountId, Balances>) {
		let digest = System::digest();
		let author =
			PowAuthor::find_author(digest.logs.iter().filter_map(|item| item.as_pre_runtime()));
		if let Some(author) = author {
			// The block reward, minted before any transaction, has created the account of the
			// miner: this can only fail for an amount it cannot hold, which is then dropped.
			let _ = <Balances as Balanced<AccountId>>::resolve(&author, amount);
		}
	}
}

/// Deposits slashed by governance, such as the one of a referendum killed, go to the miner too.
impl OnUnbalanced<pallet_balances::NegativeImbalance<Runtime>> for ToAuthor {
	fn on_nonzero_unbalanced(amount: pallet_balances::NegativeImbalance<Runtime>) {
		let digest = System::digest();
		let author =
			PowAuthor::find_author(digest.logs.iter().filter_map(|item| item.as_pre_runtime()));
		if let Some(author) = author {
			<Balances as frame_support::traits::Currency<AccountId>>::resolve_creating(&author, amount);
		}
	}
}

impl pallet_timestamp::Config for Runtime {
	/// Milliseconds since the unix epoch.
	type Moment = u64;
	type OnTimestampSet = ();
	/// Consecutive blocks are at least one second apart.
	type MinimumPeriod = ConstU64<1_000>;
	type WeightInfo = weights::pallet_timestamp::WeightInfo<Runtime>;
}

parameter_types! {
	pub const MinimumDifficulty: U256 = U256([1_000, 0, 0, 0]);
}

impl pallet_difficulty::Config for Runtime {
	type Time = Timestamp;
	type TargetBlockTime = ConstU64<MILLI_SECS_PER_BLOCK>;
	/// 120 blocks, about 36 minutes.
	type Window = ConstU32<120>;
	type MinimumDifficulty = MinimumDifficulty;
	type WeightInfo = pallet_difficulty::weights::SubstrateWeight<Runtime>;
}

impl pallet_balances::Config for Runtime {
	type MaxLocks = ConstU32<50>;
	type MaxReserves = ();
	type ReserveIdentifier = [u8; 8];
	type Balance = Balance;
	type RuntimeEvent = RuntimeEvent;
	type DustRemoval = ToAuthor;
	type ExistentialDeposit = ConstU128<EXISTENTIAL_DEPOSIT>;
	type AccountStore = System;
	type WeightInfo = weights::pallet_balances::WeightInfo<Runtime>;
	type FreezeIdentifier = RuntimeFreezeReason;
	type MaxFreezes = VariantCountOf<RuntimeFreezeReason>;
	type RuntimeHoldReason = RuntimeHoldReason;
	type RuntimeFreezeReason = RuntimeFreezeReason;
	type DoneSlashHandler = ();
}

parameter_types! {
	pub FeeMultiplier: Multiplier = Multiplier::one();
}

impl pallet_transaction_payment::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type OnChargeTransaction = FungibleAdapter<Balances, ToAuthor>;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightToFee = IdentityFee<Balance>;
	type LengthToFee = IdentityFee<Balance>;
	type FeeMultiplierUpdate = ConstFeeMultiplier<FeeMultiplier>;
	type WeightInfo = weights::pallet_transaction_payment::WeightInfo<Runtime>;
}

/// Fees go to the miner of the block: the benchmark names one, as a mined block does, so that it
/// measures the payment of the fee to the miner.
#[cfg(feature = "runtime-benchmarks")]
impl pallet_transaction_payment::BenchmarkConfig for Runtime {
	fn setup_benchmark_environment() {
		let miner: AccountId = frame_benchmarking::account("miner", 0, 0);
		System::deposit_log(sp_runtime::DigestItem::PreRuntime(
			sp_consensus_pow::POW_ENGINE_ID,
			codec::Encode::encode(&miner),
		));
	}
}

parameter_types! {
	pub const MinVestedTransfer: Balance = UNIT;
	/// Vested funds cannot be transferred or reserved before they are released.
	pub UnvestedFundsAllowedWithdrawReasons: WithdrawReasons =
		WithdrawReasons::except(WithdrawReasons::TRANSFER | WithdrawReasons::RESERVE);
}

impl pallet_vesting::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type BlockNumberToBalance = ConvertInto;
	type MinVestedTransfer = MinVestedTransfer;
	type WeightInfo = weights::pallet_vesting::WeightInfo<Runtime>;
	type UnvestedFundsAllowedWithdrawReasons = UnvestedFundsAllowedWithdrawReasons;
	type BlockNumberProvider = System;
	const MAX_VESTING_SCHEDULES: u32 = 28;
}

parameter_types! {
	pub const MultisigDepositBase: Balance = deposit(1, 88);
	pub const MultisigDepositFactor: Balance = deposit(0, 32);
}

/// Multisig accounts, such as the foundation's.
impl pallet_multisig::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type DepositBase = MultisigDepositBase;
	type DepositFactor = MultisigDepositFactor;
	type MaxSignatories = ConstU32<100>;
	type WeightInfo = weights::pallet_multisig::WeightInfo<Runtime>;
	type BlockNumberProvider = System;
}

/// Team and foundation allocations, created at genesis and released by vesting schedules.
impl pallet_allocations::Config for Runtime {
	type Currency = Balances;
	type Vesting = Vesting;
}

/// What agents may do for their owners: transfers, payments to registered agents, payment
/// channels, tasks and calls of contracts. Every other call is refused, including the management of delegations, multisig,
/// vesting and governance: an agent never votes with the TESS of its owner.
pub struct AgentCalls;

impl pallet_agents::ClassifyCall<RuntimeCall, AccountId> for AgentCalls {
	fn classify(call: &RuntimeCall) -> pallet_agents::CallKind<AccountId> {
		use pallet_agents::CallKind;
		use sp_runtime::traits::StaticLookup;
		match call {
			RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest, .. }) |
			RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death { dest, .. }) =>
				match <Runtime as frame_system::Config>::Lookup::lookup(dest.clone()) {
					Ok(to) => CallKind::Transfer { to },
					Err(_) => CallKind::Forbidden,
				},
			RuntimeCall::AgentRegistry(pallet_agent_registry::Call::pay { agent, .. }) =>
				CallKind::Transfer { to: agent.clone() },
			RuntimeCall::Revive(pallet_revive::Call::call { dest, .. }) => CallKind::Contract {
				to: <Runtime as pallet_revive::Config>::AddressMapper::to_account_id(dest),
			},
			// Opening and funding a channel pay its payee; the deposit counts in the budget.
			RuntimeCall::Channels(pallet_channels::Call::open { payee, .. }) =>
				CallKind::Transfer { to: payee.clone() },
			RuntimeCall::Channels(
				pallet_channels::Call::top_up { id, .. } |
				pallet_channels::Call::request_close { id } |
				pallet_channels::Call::finalize_close { id },
			) => match pallet_channels::Channels::<Runtime>::get(id) {
				Some(channel) => CallKind::Transfer { to: channel.payee },
				None => CallKind::Forbidden,
			},
			// Claiming and closing as the payee only bring funds to the owner.
			RuntimeCall::Channels(
				pallet_channels::Call::claim { id, .. } | pallet_channels::Call::close { id },
			) => match pallet_channels::Channels::<Runtime>::get(id) {
				Some(channel) => CallKind::Transfer { to: channel.payer },
				None => CallKind::Forbidden,
			},
			// The task market stands for every task call: an owner allows tasks by listing its
			// account in the targets, and the budget bounds rewards and bonds.
			RuntimeCall::Tasks(_) => CallKind::Transfer { to: pallet_tasks::Pallet::<Runtime>::account_id() },
			_ => CallKind::Forbidden,
		}
	}
}

parameter_types! {
	pub const AgentDepositBase: Balance = deposit(1, 160);
	pub const AgentDepositPerTarget: Balance = deposit(0, 32);
}

/// Agent accounts: keys acting for an owner within a budget, targets and an expiry.
impl pallet_agents::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type CallClassifier = AgentCalls;
	type MaxTargets = ConstU32<32>;
	type DepositBase = AgentDepositBase;
	type DepositPerTarget = AgentDepositPerTarget;
	type WeightInfo = pallet_agents::weights::SubstrateWeight<Runtime>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = AgentBenchmarkHelper;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct AgentBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl pallet_agents::BenchmarkHelper<RuntimeCall, AccountId> for AgentBenchmarkHelper {
	fn transfer(to: AccountId) -> RuntimeCall {
		RuntimeCall::Balances(pallet_balances::Call::transfer_keep_alive { dest: to.into(), value: UNIT })
	}
}

parameter_types! {
	pub const AgentRegistrationDeposit: Balance = deposit(1, 100);
	pub const AgentDepositPerByte: Balance = deposit(0, 1);
	pub const MinAgentPayment: Balance = 10 * MILLI_UNIT;
}

/// Registry of agents: identity, owner, endpoint, capabilities and reputation.
impl pallet_agent_registry::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type DepositBase = AgentRegistrationDeposit;
	type DepositPerByte = AgentDepositPerByte;
	type MinPayment = MinAgentPayment;
	type WeightInfo = pallet_agent_registry::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	pub const ChannelClosePeriod: BlockNumber = 6 * HOURS;
	pub const ChannelMinDeposit: Balance = 10 * MILLI_UNIT;
	/// A channel stores the ML-DSA public key that signs its vouchers.
	pub const ChannelStorageDeposit: Balance = deposit(1, 2_900);
}

/// Micropayments: payment channels with vouchers signed off chain.
impl pallet_channels::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type ClosePeriod = ChannelClosePeriod;
	type MinDeposit = ChannelMinDeposit;
	type StorageDeposit = ChannelStorageDeposit;
	type WeightInfo = pallet_channels::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	pub const TasksPalletId: PalletId = PalletId(*b"tess/tsk");
	pub const TaskMinReward: Balance = 10 * MILLI_UNIT;
	pub const TaskStorageDeposit: Balance = deposit(1, 800);
	pub const TaskMinPeriod: BlockNumber = 10;
	pub const TaskMaxPeriod: BlockNumber = 30 * DAYS;
	pub const TaskArbitrationPeriod: BlockNumber = 3 * DAYS;
}

/// Outcomes of tasks go to the reputation of workers in the agent registry.
pub struct TaskReputation;

impl pallet_tasks::ReputationRecorder<AccountId> for TaskReputation {
	fn record_task(worker: &AccountId, completed: bool) {
		<AgentRegistry as pallet_agent_registry::ReputationRecorder<AccountId>>::record_task(worker, completed)
	}
}

/// Task market: work ordered between agents, with the reward held in escrow.
impl pallet_tasks::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type Reputation = TaskReputation;
	type PalletId = TasksPalletId;
	type MinReward = TaskMinReward;
	type StorageDeposit = TaskStorageDeposit;
	type MinPeriod = TaskMinPeriod;
	type MaxPeriod = TaskMaxPeriod;
	type ArbitrationPeriod = TaskArbitrationPeriod;
	type WeightInfo = pallet_tasks::weights::SubstrateWeight<Runtime>;
}

/// Block rewards: 0.85 TESS per block to the miner, halving every 7 000 000 blocks.
impl pallet_emission::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type FindAuthor = PowAuthor;
	type InitialReward = ConstU128<BLOCK_REWARD>;
	type HalvingInterval = ConstU32<HALVING_INTERVAL>;
	type MiningSupply = ConstU128<MINING_SUPPLY>;
	type WeightInfo = pallet_emission::weights::SubstrateWeight<Runtime>;
}

parameter_types! {
	/// Fees of private transactions are paid from the shielded pool, without signer: they must
	/// pay at least what a signed transaction of the same weight and length pays.
	pub ShieldedMinimumFee: u64 = shielded_minimum_fee();
}

/// Encoded length of a typical private transaction: proof, two encrypted notes, public inputs.
const SHIELDED_TRANSACTION_LENGTH: u64 = 190_000;

fn shielded_minimum_fee() -> u64 {
	use frame_support::weights::WeightToFee;
	use pallet_shielded::WeightInfo;
	type Weights = pallet_shielded::weights::SubstrateWeight<Runtime>;
	let weight = ExtrinsicBaseWeight::get()
		.saturating_add(Weights::authorize_transact())
		.saturating_add(Weights::transact());
	let fee = IdentityFee::<Balance>::weight_to_fee(&weight)
		.saturating_add(IdentityFee::<Balance>::weight_to_fee(&Weight::from_parts(
			SHIELDED_TRANSACTION_LENGTH,
			0,
		)));
	fee.try_into().unwrap_or(u64::MAX)
}

/// Private transfers: notes proven with zero-knowledge STARKs. See `docs/privacy.md`.
impl pallet_shielded::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type FindAuthor = PowAuthor;
	/// Anchors stay valid for the last 100 blocks that changed the note tree.
	type RootHistory = ConstU32<100>;
	type MinimumFee = ShieldedMinimumFee;
	type Longevity = ConstU64<64>;
	type WeightInfo = pallet_shielded::weights::SubstrateWeight<Runtime>;
}

/// Storage deposit for `items` storage items using `bytes` bytes.
const fn deposit(items: u32, bytes: u32) -> Balance {
	items as Balance * 20 * MILLI_UNIT + (bytes as Balance) * (MILLI_UNIT / 10)
}

parameter_types! {
	pub const DepositPerItem: Balance = deposit(1, 0);
	pub const DepositPerChildTrieItem: Balance = deposit(1, 0) / 100;
	pub const DepositPerByte: Balance = deposit(0, 1);
	pub const CodeHashLockupDepositPercent: Perbill = Perbill::from_percent(30);
	pub const MaxEthExtrinsicWeight: FixedU128 = FixedU128::from_rational(9, 10);
	pub const GasScale: u32 = 10;
}

/// Smart contracts: PolkaVM programs only, compiled from Solidity with `resolc`.
impl pallet_revive::Config for Runtime {
	type Time = Timestamp;
	type Balance = Balance;
	type Currency = Balances;
	type OnBurn = ToAuthor;
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeHoldReason = RuntimeHoldReason;
	type WeightInfo = pallet_revive::weights::SubstrateWeight<Self>;
	type Precompiles = (MlDsa87Verify<Self>,);
	type FindAuthor = PowAuthor;
	type DepositPerByte = DepositPerByte;
	type DepositPerItem = DepositPerItem;
	type DepositPerChildTrieItem = DepositPerChildTrieItem;
	type CodeHashLockupDepositPercent = CodeHashLockupDepositPercent;
	type AddressMapper = AccountId32Mapper<Self>;
	/// No EVM bytecode: contracts run on PolkaVM.
	type AllowEVMBytecode = ConstBool<false>;
	type UploadOrigin = EnsureSigned<AccountId>;
	type InstantiateOrigin = EnsureSigned<AccountId>;
	type RuntimeMemory = ConstU32<{ 128 * 1024 * 1024 }>;
	type PVFMemory = ConstU32<{ 512 * 1024 * 1024 }>;
	/// Chain id returned to contracts by `block.chainid`.
	type ChainId = ConstU64<2261>;
	/// TESS has 12 decimals, contracts see 18.
	type NativeToEthRatio = ConstU32<1_000_000>;
	type FeeInfo = ();
	type Deposit = ();
	type MaxEthExtrinsicWeight = MaxEthExtrinsicWeight;
	type DebugEnabled = ConstBool<false>;
	type AutoMap = ConstBool<true>;
	type GasScale = GasScale;
}
