//! RandomTESS, the proof-of-work hash function of Tesserax.
//!
//! RandomTESS is RandomX v2 compiled with a Tesserax specific configuration (see
//! `randomx/src/configuration.h`): a distinct Argon2 salt and rebalanced pairs of functionally
//! equivalent instructions. Memory requirements and security margins are those of RandomX v2:
//! 256 MiB for the light mode cache and a little over 2 GiB for the fast mode dataset.
//!
//! The light mode is meant for verification, the fast mode for mining. Both produce the same
//! hashes.
//!
//! As in RandomX v2, the proof-of-work value is the [`commitment`] of a hash to its input: it is
//! cheap to compute from a claimed hash, so a node can reject an invalid proof of work before it
//! runs the virtual machine.

use std::{
	os::raw::{c_int, c_ulong, c_void},
	ptr::NonNull,
	sync::Arc,
};

mod ffi {
	use super::*;

	#[repr(C)]
	pub struct randomx_cache {
		_private: [u8; 0],
	}
	#[repr(C)]
	pub struct randomx_dataset {
		_private: [u8; 0],
	}
	#[repr(C)]
	pub struct randomx_vm {
		_private: [u8; 0],
	}

	extern "C" {
		pub fn randomx_get_flags() -> c_int;
		pub fn randomx_alloc_cache(flags: c_int) -> *mut randomx_cache;
		pub fn randomx_init_cache(cache: *mut randomx_cache, key: *const c_void, key_size: usize);
		pub fn randomx_release_cache(cache: *mut randomx_cache);
		pub fn randomx_alloc_dataset(flags: c_int) -> *mut randomx_dataset;
		pub fn randomx_dataset_item_count() -> c_ulong;
		pub fn randomx_init_dataset(
			dataset: *mut randomx_dataset,
			cache: *mut randomx_cache,
			start_item: c_ulong,
			item_count: c_ulong,
		);
		pub fn randomx_release_dataset(dataset: *mut randomx_dataset);
		pub fn randomx_create_vm(
			flags: c_int,
			cache: *mut randomx_cache,
			dataset: *mut randomx_dataset,
		) -> *mut randomx_vm;
		pub fn randomx_destroy_vm(vm: *mut randomx_vm);
		pub fn randomx_calculate_hash(
			vm: *mut randomx_vm,
			input: *const c_void,
			input_size: usize,
			output: *mut c_void,
		);
		pub fn randomx_calculate_commitment(
			input: *const c_void,
			input_size: usize,
			hash_in: *const c_void,
			com_out: *mut c_void,
		);
	}
}

/// Size of a RandomTESS hash in bytes.
pub const HASH_SIZE: usize = 32;

/// A RandomTESS hash.
pub type Hash = [u8; HASH_SIZE];

const FLAG_LARGE_PAGES: c_int = 1;
const FLAG_FULL_MEM: c_int = 4;
const FLAG_JIT: c_int = 8;
const FLAG_V2: c_int = 128;

/// Errors returned when allocating RandomTESS structures.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
	#[error("failed to allocate the RandomTESS cache")]
	CacheAllocation,
	#[error("failed to allocate the RandomTESS dataset")]
	DatasetAllocation,
	#[error("failed to create a RandomTESS virtual machine")]
	VmCreation,
}

/// Feature flags used to allocate caches, datasets and virtual machines.
///
/// The RandomX v2 algorithm flag is always set and cannot be removed: it is part of the
/// definition of RandomTESS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flags(c_int);

impl Flags {
	/// The fastest flags supported by the current CPU (JIT, hardware AES, Argon2 SIMD).
	pub fn recommended() -> Self {
		Self(unsafe { ffi::randomx_get_flags() } | FLAG_V2)
	}

	/// Portable flags: interpreted VM and software AES. Slow, but works everywhere.
	pub fn portable() -> Self {
		Self(FLAG_V2)
	}

	/// Allocate memory in large pages. Allocation fails if large pages are not available.
	pub fn with_large_pages(self) -> Self {
		Self(self.0 | FLAG_LARGE_PAGES)
	}

	/// Whether the virtual machine uses a JIT compiler.
	pub fn jit(&self) -> bool {
		self.0 & FLAG_JIT != 0
	}

	fn light(self) -> c_int {
		self.0 & !FLAG_FULL_MEM
	}

	fn full(self) -> c_int {
		self.0 | FLAG_FULL_MEM
	}
}

/// The light mode cache, derived from a key with Argon2d.
///
/// A cache is read-only once initialized and can be shared between threads.
pub struct Cache {
	raw: NonNull<ffi::randomx_cache>,
	key: Vec<u8>,
}

// SAFETY: an initialized cache is never mutated by RandomX; VMs and dataset initialization only
// read from it.
unsafe impl Send for Cache {}
unsafe impl Sync for Cache {}

impl Cache {
	/// Allocate a cache and initialize it with `key`. This takes in the order of a second.
	pub fn new(flags: Flags, key: &[u8]) -> Result<Self, Error> {
		let raw = NonNull::new(unsafe { ffi::randomx_alloc_cache(flags.light()) })
			.ok_or(Error::CacheAllocation)?;
		unsafe { ffi::randomx_init_cache(raw.as_ptr(), key.as_ptr() as *const c_void, key.len()) };
		Ok(Self { raw, key: key.to_vec() })
	}

	/// The key this cache was initialized with.
	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl Drop for Cache {
	fn drop(&mut self) {
		unsafe { ffi::randomx_release_cache(self.raw.as_ptr()) }
	}
}

/// The fast mode dataset, expanded from a cache.
///
/// A dataset is read-only once initialized and can be shared between threads.
pub struct Dataset {
	raw: NonNull<ffi::randomx_dataset>,
	key: Vec<u8>,
}

// SAFETY: an initialized dataset is never mutated by RandomX; VMs only read from it.
unsafe impl Send for Dataset {}
unsafe impl Sync for Dataset {}

impl Dataset {
	/// Allocate a dataset and expand `cache` into it using `threads` threads.
	pub fn new(flags: Flags, cache: &Cache, threads: usize) -> Result<Self, Error> {
		let raw = NonNull::new(unsafe { ffi::randomx_alloc_dataset(flags.full()) })
			.ok_or(Error::DatasetAllocation)?;

		let items = unsafe { ffi::randomx_dataset_item_count() } as u64;
		let threads = threads.clamp(1, 64) as u64;
		let per_thread = items / threads;

		struct Ptr<T>(*mut T);
		// SAFETY: every thread writes a disjoint range of dataset items and only reads the cache.
		unsafe impl<T> Send for Ptr<T> {}
		unsafe impl<T> Sync for Ptr<T> {}
		let dataset = Ptr(raw.as_ptr());
		let source = Ptr(cache.raw.as_ptr());

		std::thread::scope(|scope| {
			for i in 0..threads {
				let start = i * per_thread;
				let count = if i == threads - 1 { items - start } else { per_thread };
				let (dataset, source) = (&dataset, &source);
				scope.spawn(move || unsafe {
					ffi::randomx_init_dataset(
						dataset.0,
						source.0,
						start as c_ulong,
						count as c_ulong,
					)
				});
			}
		});

		Ok(Self { raw, key: cache.key.clone() })
	}

	/// The key of the cache this dataset was expanded from.
	pub fn key(&self) -> &[u8] {
		&self.key
	}
}

impl Drop for Dataset {
	fn drop(&mut self) {
		unsafe { ffi::randomx_release_dataset(self.raw.as_ptr()) }
	}
}

enum Backing {
	Cache(Arc<Cache>),
	Dataset(Arc<Dataset>),
}

/// A RandomTESS virtual machine.
///
/// A VM owns a 2 MiB scratchpad and must not be used by two threads at the same time. Create
/// one VM per thread; they can all share the same cache or dataset.
pub struct Vm {
	raw: NonNull<ffi::randomx_vm>,
	backing: Backing,
}

// SAFETY: a VM can move between threads; `&mut self` on `hash` prevents concurrent use.
unsafe impl Send for Vm {}

impl Vm {
	/// Create a light mode VM, used to verify hashes.
	pub fn light(flags: Flags, cache: Arc<Cache>) -> Result<Self, Error> {
		let raw = NonNull::new(unsafe {
			ffi::randomx_create_vm(flags.light(), cache.raw.as_ptr(), std::ptr::null_mut())
		})
		.ok_or(Error::VmCreation)?;
		Ok(Self { raw, backing: Backing::Cache(cache) })
	}

	/// Create a fast mode VM, used to mine.
	pub fn fast(flags: Flags, dataset: Arc<Dataset>) -> Result<Self, Error> {
		let raw = NonNull::new(unsafe {
			ffi::randomx_create_vm(flags.full(), std::ptr::null_mut(), dataset.raw.as_ptr())
		})
		.ok_or(Error::VmCreation)?;
		Ok(Self { raw, backing: Backing::Dataset(dataset) })
	}

	/// The key of the cache or dataset backing this VM.
	pub fn key(&self) -> &[u8] {
		match &self.backing {
			Backing::Cache(cache) => cache.key(),
			Backing::Dataset(dataset) => dataset.key(),
		}
	}

	/// Compute the RandomTESS hash of `input`.
	pub fn hash(&mut self, input: &[u8]) -> Hash {
		let mut output = [0u8; HASH_SIZE];
		unsafe {
			ffi::randomx_calculate_hash(
				self.raw.as_ptr(),
				input.as_ptr() as *const c_void,
				input.len(),
				output.as_mut_ptr() as *mut c_void,
			)
		};
		output
	}
}

impl Drop for Vm {
	fn drop(&mut self) {
		unsafe { ffi::randomx_destroy_vm(self.raw.as_ptr()) }
	}
}

/// The RandomX v2 commitment of `hash` to `input`: Blake2b-256 of `input || hash`.
pub fn commitment(input: &[u8], hash: &Hash) -> Hash {
	let mut output = [0u8; HASH_SIZE];
	unsafe {
		ffi::randomx_calculate_commitment(
			input.as_ptr() as *const c_void,
			input.len(),
			hash.as_ptr() as *const c_void,
			output.as_mut_ptr() as *mut c_void,
		)
	};
	output
}

#[cfg(test)]
mod tests;
