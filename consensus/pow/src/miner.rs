//! Built-in CPU miner.
//!
//! Each thread hashes nonces against the block template held by the [`MiningHandle`] and submits
//! the first seal that satisfies the difficulty. Threads share one RandomTESS dataset (fast mode)
//! or one cache (light mode) and rebuild it when the key changes.

use codec::Encode;
use parking_lot::Mutex;
use randomtess::{Cache, Dataset, Flags, Vm};
use sc_consensus_pow::MiningHandle;
use sp_api::ProvideRuntimeApi;
use sp_blockchain::{HeaderBackend, HeaderMetadata};
use sp_consensus_pow::DifficultyApi;
use sp_runtime::traits::Block as BlockT;
use std::{
	collections::hash_map::RandomState,
	hash::{BuildHasher, Hasher},
	sync::{
		atomic::{AtomicU64, Ordering},
		Arc,
	},
	thread,
	time::{Duration, Instant},
};

use crate::{
	meets_difficulty, pow_value, seed_hash, work_input, Difficulty, Seal, TesseraxPow, LOG_TARGET,
	NONCE_SIZE,
};

/// Hashes computed between two checks for a new block template.
const BATCH: u64 = 32;

/// Interval between two hashrate reports.
const REPORT_INTERVAL: Duration = Duration::from_secs(60);

/// RandomTESS memory mode used to mine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MiningMode {
	/// Full 2 GiB dataset. About ten times faster than light mode.
	Fast,
	/// 256 MiB cache only.
	Light,
}

#[derive(Clone, Copy, Debug)]
pub struct MinerConfig {
	pub threads: usize,
	pub mode: MiningMode,
}

/// RandomTESS memory shared by the mining threads.
enum Memory {
	Dataset(Arc<Dataset>),
	Cache(Arc<Cache>),
}

impl Memory {
	fn key(&self) -> &[u8] {
		match self {
			Memory::Dataset(dataset) => dataset.key(),
			Memory::Cache(cache) => cache.key(),
		}
	}
}

struct Shared {
	flags: Flags,
	config: MinerConfig,
	memory: Mutex<Option<Memory>>,
	hashes: AtomicU64,
}

impl Shared {
	/// A VM for `key`, building the shared memory if another thread has not done it yet.
	fn vm(&self, key: &[u8]) -> Result<Vm, randomtess::Error> {
		let mut memory = self.memory.lock();
		if memory.as_ref().map_or(true, |memory| memory.key() != key) {
			// Release the previous dataset before allocating the next one.
			*memory = None;
			let started = Instant::now();
			let cache = Cache::new(self.flags, key)?;
			*memory = Some(match self.config.mode {
				MiningMode::Fast => Memory::Dataset(Arc::new(Dataset::new(
					self.flags,
					&cache,
					self.config.threads,
				)?)),
				MiningMode::Light => Memory::Cache(Arc::new(cache)),
			});
			log::info!(
				target: LOG_TARGET,
				"⛏  RandomTESS {} ready in {:.1}s",
				if self.config.mode == MiningMode::Fast { "dataset" } else { "cache" },
				started.elapsed().as_secs_f64(),
			);
		}
		match memory.as_ref().expect("set above; qed") {
			Memory::Dataset(dataset) => Vm::fast(self.flags, dataset.clone()),
			Memory::Cache(cache) => Vm::light(self.flags, cache.clone()),
		}
	}
}

/// Start the mining threads. They run until the process exits.
pub fn start_miner<B, C, L>(
	handle: MiningHandle<B, TesseraxPow<B, C>, L>,
	client: Arc<C>,
	config: MinerConfig,
) where
	B: BlockT,
	C: ProvideRuntimeApi<B>
		+ HeaderBackend<B>
		+ HeaderMetadata<B, Error = sp_blockchain::Error>
		+ Send
		+ Sync
		+ 'static,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B> + 'static,
{
	let threads = config.threads.max(1);
	let shared = Arc::new(Shared {
		flags: Flags::recommended(),
		config: MinerConfig { threads, ..config },
		memory: Mutex::new(None),
		hashes: AtomicU64::new(0),
	});

	log::info!(
		target: LOG_TARGET,
		"⛏  Starting {threads} mining thread(s) in {} mode (JIT: {})",
		if config.mode == MiningMode::Fast { "fast" } else { "light" },
		shared.flags.jit(),
	);

	for index in 0..threads {
		let (handle, client, shared) = (handle.clone(), client.clone(), shared.clone());
		thread::Builder::new()
			.name(format!("tesserax-miner-{index}"))
			.spawn(move || mine(handle, client, shared))
			.expect("spawning a mining thread failed");
	}

	thread::Builder::new()
		.name("tesserax-hashrate".into())
		.spawn(move || report(handle, shared))
		.expect("spawning the hashrate reporter failed");
}

fn mine<B, C, L>(handle: MiningHandle<B, TesseraxPow<B, C>, L>, client: Arc<C>, shared: Arc<Shared>)
where
	B: BlockT,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B>,
{
	let mut vm: Option<Vm> = None;
	// Random leading bytes and a counter in the last 8 bytes of the nonce.
	let mut nonce = [0u8; NONCE_SIZE];
	let prefix = RandomState::new().build_hasher().finish().to_le_bytes();
	nonce[..NONCE_SIZE - 8].copy_from_slice(&prefix[..NONCE_SIZE - 8]);
	let mut counter = RandomState::new().build_hasher().finish();

	loop {
		let version = handle.version();
		let Some(metadata) = handle.metadata() else {
			thread::sleep(Duration::from_millis(100));
			continue;
		};

		let key = match seed_hash::<B, _>(client.as_ref(), metadata.best_hash) {
			Ok(key) => key,
			Err(e) => {
				log::warn!(target: LOG_TARGET, "Failed to compute the RandomTESS key: {e}");
				thread::sleep(Duration::from_secs(1));
				continue;
			},
		};
		if vm.as_ref().map_or(true, |vm| vm.key() != key.as_ref()) {
			vm = None;
			match shared.vm(key.as_ref()) {
				Ok(new) => vm = Some(new),
				Err(e) => {
					log::error!(target: LOG_TARGET, "Failed to set up RandomTESS: {e}");
					thread::sleep(Duration::from_secs(5));
					continue;
				},
			}
		}
		let vm = vm.as_mut().expect("set above; qed");

		'template: while handle.version() == version {
			for _ in 0..BATCH {
				counter = counter.wrapping_add(1);
				nonce[NONCE_SIZE - 8..].copy_from_slice(&counter.to_le_bytes());
				let input = work_input(metadata.pre_hash.as_ref(), &nonce);
				let hash = vm.hash(&input);
				if meets_difficulty(&pow_value(&input, &hash), metadata.difficulty) {
					futures::executor::block_on(handle.submit(Seal { nonce, hash }.encode()));
					break 'template;
				}
			}
			shared.hashes.fetch_add(BATCH, Ordering::Relaxed);
		}
	}
}

fn report<B, C, L>(handle: MiningHandle<B, TesseraxPow<B, C>, L>, shared: Arc<Shared>)
where
	B: BlockT,
	C: ProvideRuntimeApi<B> + HeaderBackend<B> + HeaderMetadata<B, Error = sp_blockchain::Error>,
	C::Api: DifficultyApi<B, Difficulty>,
	L: sc_consensus::JustificationSyncLink<B>,
{
	let mut last = Instant::now();
	loop {
		thread::sleep(REPORT_INTERVAL);
		let hashes = shared.hashes.swap(0, Ordering::Relaxed);
		let elapsed = last.elapsed().as_secs_f64();
		last = Instant::now();
		if let Some(metadata) = handle.metadata() {
			log::info!(
				target: LOG_TARGET,
				"⛏  Hashrate {:.1} H/s, difficulty {}",
				hashes as f64 / elapsed,
				metadata.difficulty,
			);
		}
	}
}
