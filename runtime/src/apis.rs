use alloc::vec::Vec;
use frame_support::{
	genesis_builder_helper::{build_state, get_preset},
	weights::Weight,
};
use sp_api::impl_runtime_apis;
use sp_core::{crypto::KeyTypeId, OpaqueMetadata, U256};
use sp_runtime::{
	traits::Block as BlockT,
	transaction_validity::{TransactionSource, TransactionValidity},
	ApplyExtrinsicResult,
};
use sp_session::OpaqueGeneratedSessionKeys;
use sp_version::RuntimeVersion;

use pallet_revive::{evm, AddressMapper, EthTransactError};

use super::{
	AccountId, Balance, Block, BlockNumber, Difficulty, Executive, InherentDataExt, Nonce, Revive,
	Runtime, RuntimeCall, RuntimeGenesisConfig, RuntimeOrigin, System, TransactionPayment, VERSION,
};

impl_runtime_apis! {
	impl sp_api::Core<Block> for Runtime {
		fn version() -> RuntimeVersion {
			VERSION
		}

		fn execute_block(block: <Block as BlockT>::LazyBlock) {
			Executive::execute_block(block);
		}

		fn initialize_block(header: &<Block as BlockT>::Header) -> sp_runtime::ExtrinsicInclusionMode {
			Executive::initialize_block(header)
		}
	}

	impl sp_api::Metadata<Block> for Runtime {
		fn metadata() -> OpaqueMetadata {
			OpaqueMetadata::new(Runtime::metadata().into())
		}

		fn metadata_at_version(version: u32) -> Option<OpaqueMetadata> {
			Runtime::metadata_at_version(version)
		}

		fn metadata_versions() -> Vec<u32> {
			Runtime::metadata_versions()
		}
	}

	impl frame_support::view_functions::runtime_api::RuntimeViewFunction<Block> for Runtime {
		fn execute_view_function(id: frame_support::view_functions::ViewFunctionId, input: Vec<u8>) -> Result<Vec<u8>, frame_support::view_functions::ViewFunctionDispatchError> {
			Runtime::execute_view_function(id, input)
		}
	}

	impl sp_block_builder::BlockBuilder<Block> for Runtime {
		fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
			Executive::apply_extrinsic(extrinsic)
		}

		fn finalize_block() -> <Block as BlockT>::Header {
			Executive::finalize_block()
		}

		fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
			data.create_extrinsics()
		}

		fn check_inherents(
			block: <Block as BlockT>::LazyBlock,
			data: sp_inherents::InherentData,
		) -> sp_inherents::CheckInherentsResult {
			data.check_extrinsics(&block)
		}
	}

	impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
		fn validate_transaction(
			source: TransactionSource,
			tx: <Block as BlockT>::Extrinsic,
			block_hash: <Block as BlockT>::Hash,
		) -> TransactionValidity {
			Executive::validate_transaction(source, tx, block_hash)
		}
	}

	impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
		fn offchain_worker(header: &<Block as BlockT>::Header) {
			Executive::offchain_worker(header)
		}
	}

	impl sp_consensus_pow::DifficultyApi<Block, U256> for Runtime {
		fn difficulty() -> U256 {
			Difficulty::difficulty()
		}
	}

	// Proof of work has no session keys. The API is still required by the node RPC.
	impl sp_session::SessionKeys<Block> for Runtime {
		fn generate_session_keys(_owner: Vec<u8>, _seed: Option<Vec<u8>>) -> OpaqueGeneratedSessionKeys {
			OpaqueGeneratedSessionKeys::default()
		}

		fn decode_session_keys(_encoded: Vec<u8>) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
			None
		}
	}

	impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Nonce> for Runtime {
		fn account_nonce(account: AccountId) -> Nonce {
			System::account_nonce(account)
		}
	}

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
		fn query_info(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_info(uxt, len)
		}
		fn query_fee_details(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment::FeeDetails<Balance> {
			TransactionPayment::query_fee_details(uxt, len)
		}
		fn query_weight_to_fee(weight: Weight) -> Balance {
			TransactionPayment::weight_to_fee(weight)
		}
		fn query_length_to_fee(length: u32) -> Balance {
			TransactionPayment::length_to_fee(length)
		}
	}

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentCallApi<Block, Balance, RuntimeCall>
		for Runtime
	{
		fn query_call_info(
			call: RuntimeCall,
			len: u32,
		) -> pallet_transaction_payment::RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_call_info(call, len)
		}
		fn query_call_fee_details(
			call: RuntimeCall,
			len: u32,
		) -> pallet_transaction_payment::FeeDetails<Balance> {
			TransactionPayment::query_call_fee_details(call, len)
		}
		fn query_weight_to_fee(weight: Weight) -> Balance {
			TransactionPayment::weight_to_fee(weight)
		}
		fn query_length_to_fee(length: u32) -> Balance {
			TransactionPayment::length_to_fee(length)
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	impl frame_benchmarking::Benchmark<Block> for Runtime {
		fn benchmark_metadata(extra: bool) -> (
			Vec<frame_benchmarking::BenchmarkList>,
			Vec<frame_support::traits::StorageInfo>,
		) {
			use frame_benchmarking::{baseline, BenchmarkList};
			use frame_support::traits::StorageInfoTrait;
			use frame_system_benchmarking::Pallet as SystemBench;
			use frame_system_benchmarking::extensions::Pallet as SystemExtensionsBench;
			use baseline::Pallet as BaselineBench;
			use super::*;

			let mut list = Vec::<BenchmarkList>::new();
			list_benchmarks!(list, extra);

			let storage_info = AllPalletsWithSystem::storage_info();

			(list, storage_info)
		}

		#[allow(non_local_definitions)]
		fn dispatch_benchmark(
			config: frame_benchmarking::BenchmarkConfig
		) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, alloc::string::String> {
			use frame_benchmarking::{baseline, BenchmarkBatch};
			use sp_storage::TrackedStorageKey;
			use frame_system_benchmarking::Pallet as SystemBench;
			use frame_system_benchmarking::extensions::Pallet as SystemExtensionsBench;
			use baseline::Pallet as BaselineBench;
			use super::*;

			impl frame_system_benchmarking::Config for Runtime {}
			impl baseline::Config for Runtime {}

			use frame_support::traits::WhitelistedStorageKeys;
			let whitelist: Vec<TrackedStorageKey> = AllPalletsWithSystem::whitelisted_storage_keys();

			let mut batches = Vec::<BenchmarkBatch>::new();
			let params = (&config, &whitelist);
			add_benchmarks!(params, batches);

			Ok(batches)
		}
	}

	#[cfg(feature = "try-runtime")]
	impl frame_try_runtime::TryRuntime<Block> for Runtime {
		fn on_runtime_upgrade(checks: frame_try_runtime::UpgradeCheckSelect) -> (Weight, Weight) {
			// NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
			// have a backtrace here. If any of the pre/post migration checks fail, we shall stop
			// right here and right now.
			let weight = Executive::try_runtime_upgrade(checks).unwrap();
			(weight, super::configs::RuntimeBlockWeights::get().max_block)
		}

		fn execute_block(
			block: <Block as BlockT>::LazyBlock,
			state_root_check: bool,
			signature_check: bool,
			select: frame_try_runtime::TryStateSelect
		) -> Weight {
			// NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
			// have a backtrace here.
			Executive::try_execute_block(block, state_root_check, signature_check, select).expect("execute-block failed")
		}
	}

	// Contracts are deployed and called with ML-DSA signed extrinsics. The Ethereum transaction
	// entry points of the API are not supported.
	impl pallet_revive::ReviveApi<Block, AccountId, Balance, Nonce, BlockNumber, u64> for Runtime {
		fn eth_block() -> pallet_revive::EthBlock {
			Revive::eth_block()
		}

		fn eth_block_hash(number: pallet_revive::U256) -> Option<pallet_revive::H256> {
			Revive::eth_block_hash_from_number(number)
		}

		fn eth_receipt_data() -> Vec<pallet_revive::ReceiptGasInfo> {
			Revive::eth_receipt_data()
		}

		fn balance(address: pallet_revive::H160) -> pallet_revive::U256 {
			Revive::evm_balance(&address)
		}

		fn block_author() -> pallet_revive::H160 {
			Revive::block_author()
		}

		fn block_gas_limit() -> pallet_revive::U256 {
			Revive::evm_block_gas_limit()
		}

		fn max_extrinsic_weight_in_gas() -> pallet_revive::U256 {
			Revive::evm_max_extrinsic_weight_in_gas()
		}

		fn gas_price() -> pallet_revive::U256 {
			Revive::evm_base_fee()
		}

		fn nonce(address: pallet_revive::H160) -> Nonce {
			let account = <Runtime as pallet_revive::Config>::AddressMapper::to_account_id(&address);
			System::account_nonce(account)
		}

		fn address(account_id: AccountId) -> pallet_revive::H160 {
			<Runtime as pallet_revive::Config>::AddressMapper::to_address(&account_id)
		}

		fn eth_transact(
			_tx: evm::GenericTransaction,
		) -> Result<pallet_revive::EthTransactInfo<Balance>, EthTransactError> {
			Err(ethereum_unsupported())
		}

		fn eth_transact_with_config(
			_tx: evm::GenericTransaction,
			_config: pallet_revive::DryRunConfig<u64>,
		) -> Result<pallet_revive::EthTransactInfo<Balance>, EthTransactError> {
			Err(ethereum_unsupported())
		}

		fn eth_estimate_gas(
			_tx: evm::GenericTransaction,
			_config: pallet_revive::DryRunConfig<u64>,
		) -> Result<pallet_revive::U256, EthTransactError> {
			Err(ethereum_unsupported())
		}

		fn eth_pre_dispatch_weight(_tx: Vec<u8>) -> Result<Weight, EthTransactError> {
			Err(ethereum_unsupported())
		}

		fn call(
			origin: AccountId,
			dest: pallet_revive::H160,
			value: Balance,
			weight_limit: Option<Weight>,
			storage_deposit_limit: Option<Balance>,
			input_data: Vec<u8>,
		) -> pallet_revive::ContractResult<pallet_revive::ExecReturnValue, Balance> {
			let block_weights = <<Runtime as frame_system::Config>::BlockWeights as frame_support::traits::Get<frame_system::limits::BlockWeights>>::get();
			Revive::prepare_dry_run(&origin);
			Revive::bare_call(
				RuntimeOrigin::signed(origin),
				dest,
				Revive::convert_native_to_evm(value),
				pallet_revive::TransactionLimits::WeightAndDeposit {
					weight_limit: weight_limit.unwrap_or(block_weights.max_block),
					deposit_limit: storage_deposit_limit.unwrap_or(u128::MAX),
				},
				input_data,
				&pallet_revive::ExecConfig::new_substrate_tx().with_dry_run(Default::default()),
			)
		}

		fn instantiate(
			origin: AccountId,
			value: Balance,
			weight_limit: Option<Weight>,
			storage_deposit_limit: Option<Balance>,
			code: pallet_revive::Code,
			data: Vec<u8>,
			salt: Option<[u8; 32]>,
		) -> pallet_revive::ContractResult<pallet_revive::InstantiateReturnValue, Balance> {
			let block_weights = <<Runtime as frame_system::Config>::BlockWeights as frame_support::traits::Get<frame_system::limits::BlockWeights>>::get();
			Revive::prepare_dry_run(&origin);
			Revive::bare_instantiate(
				RuntimeOrigin::signed(origin),
				Revive::convert_native_to_evm(value),
				pallet_revive::TransactionLimits::WeightAndDeposit {
					weight_limit: weight_limit.unwrap_or(block_weights.max_block),
					deposit_limit: storage_deposit_limit.unwrap_or(u128::MAX),
				},
				code,
				data,
				salt,
				&pallet_revive::ExecConfig::new_substrate_tx().with_dry_run(Default::default()),
			)
		}

		fn upload_code(
			origin: AccountId,
			code: Vec<u8>,
			storage_deposit_limit: Option<Balance>,
		) -> pallet_revive::CodeUploadResult<Balance> {
			Revive::bare_upload_code(
				RuntimeOrigin::signed(origin),
				code,
				storage_deposit_limit.unwrap_or(u128::MAX),
			)
		}

		fn get_storage_var_key(
			address: pallet_revive::H160,
			key: Vec<u8>,
		) -> pallet_revive::GetStorageResult {
			Revive::get_storage_var_key(address, key)
		}

		fn get_storage(address: pallet_revive::H160, key: [u8; 32]) -> pallet_revive::GetStorageResult {
			Revive::get_storage(address, key)
		}

		fn trace_block(
			block: Block,
			tracer_type: evm::TracerType,
		) -> Vec<(u32, evm::Trace)> {
			use pallet_revive::tracing::trace;

			if matches!(tracer_type, evm::TracerType::ExecutionTracer(_)) &&
				!pallet_revive::DebugSettings::is_execution_tracing_enabled::<Runtime>()
			{
				return Default::default()
			}

			let mut traces = Vec::new();
			let (header, extrinsics) = block.deconstruct();
			Executive::initialize_block(&header);
			for (index, ext) in extrinsics.into_iter().enumerate() {
				let mut tracer = Revive::evm_tracer(tracer_type.clone());
				let t = tracer.as_tracing();
				let _ = trace(t, || Executive::apply_extrinsic(ext));
				if let Some(tx_trace) = tracer.collect_trace() {
					traces.push((index as u32, tx_trace));
				}
			}
			traces
		}

		fn trace_tx(
			block: Block,
			tx_index: u32,
			tracer_type: evm::TracerType,
		) -> Option<evm::Trace> {
			use pallet_revive::tracing::trace;

			if matches!(tracer_type, evm::TracerType::ExecutionTracer(_)) &&
				!pallet_revive::DebugSettings::is_execution_tracing_enabled::<Runtime>()
			{
				return None
			}

			let mut tracer = Revive::evm_tracer(tracer_type);
			let (header, extrinsics) = block.deconstruct();
			Executive::initialize_block(&header);
			for (index, ext) in extrinsics.into_iter().enumerate() {
				if index as u32 == tx_index {
					let t = tracer.as_tracing();
					let _ = trace(t, || Executive::apply_extrinsic(ext));
					break;
				} else {
					let _ = Executive::apply_extrinsic(ext);
				}
			}
			tracer.collect_trace()
		}

		fn trace_call(
			_tx: evm::GenericTransaction,
			_tracer_type: evm::TracerType,
		) -> Result<evm::Trace, EthTransactError> {
			Err(ethereum_unsupported())
		}

		fn trace_call_with_config(
			_tx: evm::GenericTransaction,
			_tracer_type: evm::TracerType,
			_config: evm::TracingConfig,
		) -> Result<evm::Trace, EthTransactError> {
			Err(ethereum_unsupported())
		}

		fn runtime_pallets_address() -> pallet_revive::H160 {
			pallet_revive::RUNTIME_PALLETS_ADDR
		}

		fn code(address: pallet_revive::H160) -> Vec<u8> {
			Revive::code(&address)
		}

		fn account_id(address: pallet_revive::H160) -> AccountId {
			<Runtime as pallet_revive::Config>::AddressMapper::to_account_id(&address)
		}

		fn new_balance_with_dust(
			balance: pallet_revive::U256,
		) -> Result<(Balance, u32), pallet_revive::BalanceConversionError> {
			Revive::new_balance_with_dust(balance)
		}
	}

	impl sp_genesis_builder::GenesisBuilder<Block> for Runtime {
		fn build_state(config: Vec<u8>) -> sp_genesis_builder::Result {
			build_state::<RuntimeGenesisConfig>(config)
		}

		fn get_preset(id: &Option<sp_genesis_builder::PresetId>) -> Option<Vec<u8>> {
			get_preset::<RuntimeGenesisConfig>(id, crate::genesis_config_presets::get_preset)
		}

		fn preset_names() -> Vec<sp_genesis_builder::PresetId> {
			crate::genesis_config_presets::preset_names()
		}
	}
}

fn ethereum_unsupported() -> EthTransactError {
	EthTransactError::Message("Ethereum transactions are not supported".into())
}
