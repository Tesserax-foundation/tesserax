//! Service and ServiceFactory implementation. Specialized wrapper over substrate service.

use futures::FutureExt;
use sc_client_api::Backend;
use sc_service::{error::Error as ServiceError, Configuration, TaskManager};
use sc_telemetry::{Telemetry, TelemetryWorker};
use sc_transaction_pool_api::OffchainTransactionPoolFactory;
use sp_core::Encode;
use std::{sync::Arc, time::Duration};
use tesserax_pow::{MinerConfig, TesseraxPow, CONFIRMATION_DEPTH};
use tesserax_runtime::{apis::RuntimeApi, opaque::Block, AccountId};

pub(crate) type FullClient = sc_service::TFullClient<
	Block,
	RuntimeApi,
	sc_executor::WasmExecutor<sp_io::SubstrateHostFunctions>,
>;
type FullBackend = sc_service::TFullBackend<Block>;
type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;
type Algorithm = TesseraxPow<Block, FullClient>;
type BlockImport = sc_consensus_pow::PowBlockImport<
	Block,
	Arc<FullClient>,
	FullClient,
	FullSelectChain,
	Algorithm,
	InherentDataProviders,
>;

/// Inherent data providers used to build and check blocks.
type InherentDataProviders = fn(
	<Block as sp_runtime::traits::Block>::Hash,
	(),
) -> std::future::Ready<
	Result<sp_timestamp::InherentDataProvider, Box<dyn std::error::Error + Send + Sync>>,
>;

fn inherent_data_providers(
	_parent: <Block as sp_runtime::traits::Block>::Hash,
	_: (),
) -> std::future::Ready<
	Result<sp_timestamp::InherentDataProvider, Box<dyn std::error::Error + Send + Sync>>,
> {
	std::future::ready(Ok(sp_timestamp::InherentDataProvider::from_system_time()))
}

/// Interval at which the block template is rebuilt to include new transactions and refresh its
/// timestamp.
const TEMPLATE_REFRESH: Duration = Duration::from_secs(10);

/// Time allowed to build a block template.
const BLOCK_BUILD_TIME: Duration = Duration::from_secs(2);

pub type Service = sc_service::PartialComponents<
	FullClient,
	FullBackend,
	FullSelectChain,
	sc_consensus::DefaultImportQueue<Block>,
	sc_transaction_pool::TransactionPoolHandle<Block, FullClient>,
	(BlockImport, Algorithm, Option<Telemetry>),
>;

/// Built-in miner settings.
pub struct Mining {
	pub author: AccountId,
	pub miner: MinerConfig,
}

pub fn new_partial(config: &Configuration) -> Result<Service, ServiceError> {
	let telemetry = config
		.telemetry_endpoints
		.clone()
		.filter(|x| !x.is_empty())
		.map(|endpoints| -> Result<_, sc_telemetry::Error> {
			let worker = TelemetryWorker::new(16)?;
			let telemetry = worker.handle().new_telemetry(endpoints);
			Ok((worker, telemetry))
		})
		.transpose()?;

	let executor = sc_service::new_wasm_executor::<sp_io::SubstrateHostFunctions>(&config.executor);

	let (client, backend, keystore_container, task_manager) =
		sc_service::new_full_parts::<Block, RuntimeApi, _>(
			config,
			telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
			executor,
			Vec::new(),
		)?;
	let client = Arc::new(client);

	let telemetry = telemetry.map(|(worker, telemetry)| {
		task_manager.spawn_handle().spawn("telemetry", None, worker.run());
		telemetry
	});

	let select_chain = sc_consensus::LongestChain::new(backend.clone());

	let transaction_pool = Arc::from(
		sc_transaction_pool::Builder::new(
			task_manager.spawn_essential_handle(),
			client.clone(),
			config.role.is_authority().into(),
		)
		.with_options(config.transaction_pool.clone())
		.with_prometheus(config.prometheus_registry())
		.build(),
	);

	let algorithm = TesseraxPow::new(client.clone());

	let block_import = sc_consensus_pow::PowBlockImport::new(
		client.clone(),
		client.clone(),
		algorithm.clone(),
		0,
		select_chain.clone(),
		inherent_data_providers as InherentDataProviders,
	);

	let import_queue = sc_consensus_pow::import_queue(
		Box::new(block_import.clone()),
		None,
		algorithm.clone(),
		&task_manager.spawn_essential_handle(),
		config.prometheus_registry(),
	)?;

	Ok(sc_service::PartialComponents {
		client,
		backend,
		task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		other: (block_import, algorithm, telemetry),
	})
}

/// Builds a new service for a full client.
pub fn new_full<
	N: sc_network::NetworkBackend<Block, <Block as sp_runtime::traits::Block>::Hash>,
>(
	config: Configuration,
	mining: Option<Mining>,
) -> Result<TaskManager, ServiceError> {
	let sc_service::PartialComponents {
		client,
		backend,
		mut task_manager,
		import_queue,
		keystore_container,
		select_chain,
		transaction_pool,
		other: (block_import, algorithm, mut telemetry),
	} = new_partial(&config)?;

	let net_config = sc_network::config::FullNetworkConfiguration::<
		Block,
		<Block as sp_runtime::traits::Block>::Hash,
		N,
	>::new(&config.network, config.prometheus_registry().cloned());
	let metrics = N::register_notification_metrics(config.prometheus_registry());

	let (network, system_rpc_tx, tx_handler_controller, sync_service) =
		sc_service::build_network(sc_service::BuildNetworkParams {
			config: &config,
			net_config,
			client: client.clone(),
			transaction_pool: transaction_pool.clone(),
			spawn_handle: task_manager.spawn_handle(),
			spawn_essential_handle: task_manager.spawn_essential_handle(),
			import_queue,
			block_announce_validator_builder: None,
			warp_sync_config: None,
			block_relay: None,
			metrics,
		})?;

	if config.offchain_worker.enabled {
		let offchain_workers =
			sc_offchain::OffchainWorkers::new(sc_offchain::OffchainWorkerOptions {
				runtime_api_provider: client.clone(),
				is_validator: config.role.is_authority(),
				keystore: Some(keystore_container.keystore()),
				offchain_db: backend.offchain_storage(),
				transaction_pool: Some(OffchainTransactionPoolFactory::new(
					transaction_pool.clone(),
				)),
				network_provider: Arc::new(network.clone()),
				enable_http_requests: true,
				custom_extensions: |_| vec![],
			})?;
		task_manager.spawn_handle().spawn(
			"offchain-workers-runner",
			"offchain-worker",
			offchain_workers.run(client.clone(), task_manager.spawn_handle()).boxed(),
		);
	}

	let prometheus_registry = config.prometheus_registry().cloned();

	let rpc_extensions_builder = {
		let client = client.clone();
		let pool = transaction_pool.clone();

		Box::new(move |_| {
			let deps = crate::rpc::FullDeps { client: client.clone(), pool: pool.clone() };
			crate::rpc::create_full(deps).map_err(Into::into)
		})
	};

	let _rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
		network: Arc::new(network.clone()),
		client: client.clone(),
		keystore: keystore_container.keystore(),
		task_manager: &mut task_manager,
		transaction_pool: transaction_pool.clone(),
		rpc_builder: rpc_extensions_builder,
		backend,
		system_rpc_tx,
		tx_handler_controller,
		sync_service: sync_service.clone(),
		config,
		telemetry: telemetry.as_mut(),
		tracing_execute_block: None,
	})?;

	// Blocks buried deep enough are finalized, which bounds reorganizations.
	task_manager.spawn_essential_handle().spawn(
		"pow-confirmation",
		None,
		tesserax_pow::finalize_at_depth::<Block, FullBackend, _>(
			client.clone(),
			CONFIRMATION_DEPTH,
		),
	);

	if let Some(mining) = mining {
		let proposer_factory = sc_basic_authorship::ProposerFactory::new(
			task_manager.spawn_handle(),
			client.clone(),
			transaction_pool.clone(),
			prometheus_registry.as_ref(),
			telemetry.as_ref().map(|x| x.handle()),
		);

		let (handle, worker) = sc_consensus_pow::start_mining_worker(
			Box::new(block_import),
			client.clone(),
			select_chain,
			algorithm,
			proposer_factory,
			sync_service.clone(),
			sync_service.clone(),
			Some(mining.author.encode()),
			inherent_data_providers as InherentDataProviders,
			TEMPLATE_REFRESH,
			BLOCK_BUILD_TIME,
		);

		task_manager
			.spawn_essential_handle()
			.spawn_blocking("pow-mining-worker", Some("block-authoring"), worker);

		tesserax_pow::start_miner(handle, client, mining.miner);
	}

	Ok(task_manager)
}
