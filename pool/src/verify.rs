//! Verification of shares: the RandomTESS hash a rig claims must be the hash of the work input.
//!
//! Verification runs the RandomTESS virtual machine in light mode, about as much work as one hash
//! of a rig, on threads of its own. The light caches of the live keys (two around a key change,
//! 256 MiB each) are shared by the threads.

use parking_lot::Mutex;
use randomtess::{Cache, Flags, Vm};
use std::{
	sync::{mpsc, Arc},
	thread,
};
use tokio::sync::oneshot;

struct Request {
	seed: [u8; 32],
	input: Vec<u8>,
	reply: oneshot::Sender<Result<[u8; 32], String>>,
}

/// Light caches of the live keys, most recently used first.
#[derive(Default)]
struct Caches(Mutex<Vec<Arc<Cache>>>);

impl Caches {
	const CAPACITY: usize = 2;

	fn get(&self, flags: Flags, seed: &[u8; 32]) -> Result<Arc<Cache>, randomtess::Error> {
		let mut caches = self.0.lock();
		if let Some(index) = caches.iter().position(|cache| cache.key() == seed) {
			let cache = caches.remove(index);
			caches.insert(0, cache.clone());
			return Ok(cache);
		}
		// Building a cache takes about a second; shares wait for it once per key.
		tracing::info!("Building the RandomTESS cache of key {}", hex::encode(seed));
		let cache = Arc::new(Cache::new(flags, seed)?);
		caches.insert(0, cache.clone());
		caches.truncate(Self::CAPACITY);
		Ok(cache)
	}
}

/// A pool of verification threads.
#[derive(Clone)]
pub struct Verifier {
	requests: mpsc::Sender<Request>,
}

impl Verifier {
	pub fn start(threads: usize) -> Self {
		let (requests, receiver) = mpsc::channel::<Request>();
		let receiver = Arc::new(Mutex::new(receiver));
		let caches = Arc::new(Caches::default());
		let flags = Flags::recommended();
		for index in 0..threads.max(1) {
			let receiver = receiver.clone();
			let caches = caches.clone();
			thread::Builder::new()
				.name(format!("verify-{index}"))
				.spawn(move || {
					// One machine per key, rebuilt when the key changes.
					let mut vm: Option<Vm> = None;
					loop {
						let Ok(request) = receiver.lock().recv() else { return };
						let result = (|| {
							if vm.as_ref().map(|vm| vm.key() != request.seed).unwrap_or(true) {
								vm = Some(Vm::light(flags, caches.get(flags, &request.seed)?)?);
							}
							Ok::<_, randomtess::Error>(vm.as_mut().expect("set above; qed").hash(&request.input))
						})();
						let _ = request.reply.send(result.map_err(|e| format!("{e:?}")));
					}
				})
				.expect("verification threads start");
		}
		Verifier { requests }
	}

	/// The RandomTESS hash of `input` under the key `seed`.
	pub async fn hash(&self, seed: [u8; 32], input: Vec<u8>) -> Result<[u8; 32], String> {
		let (reply, response) = oneshot::channel();
		self.requests
			.send(Request { seed, input, reply })
			.map_err(|_| "the verification threads stopped".to_string())?;
		response.await.map_err(|_| "the verification threads stopped".to_string())?
	}
}
