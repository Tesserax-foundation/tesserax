//! The shielded wallet of Tesserax as a WebAssembly module for WASI hosts
//! (`cargo build --target wasm32-wasip1`), such as the Python SDK through wasmtime: one module
//! for every platform, generating the same proofs as the native wallet.
//!
//! The interface is plain C over the linear memory of the module:
//!
//! - the host writes its inputs in buffers of [`alloc`] and releases them with [`dealloc`];
//! - a wallet is a handle from [`wallet_new`] or [`wallet_restore`], released by [`wallet_free`];
//! - functions returning data return 0 and leave it in the result buffer, read with
//!   [`result_ptr`] and [`result_len`]; on failure they return 1 and leave the message there,
//!   UTF-8. The buffer is valid until the next call.
//!
//! Values are planck, accounts 32 bytes, digests the 32 bytes of their SCALE encoding,
//! transactions the SCALE encoding of `pallet_shielded::ShieldedTransaction`. The randomness of
//! the notes comes from WASI (`random_get`).

use codec::{Decode, Encode};
use core::cell::RefCell;
use std::collections::BTreeMap;
use tesserax_shielded_wallet::{self as shielded, Digest, ShieldedTransaction, Transacted, Wallet};

thread_local! {
	static RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
	static WALLETS: RefCell<(u32, BTreeMap<u32, Wallet>)> = const { RefCell::new((0, BTreeMap::new())) };
}

/// Leave `result` in the result buffer: 0 with the data, 1 with the message of the error.
fn finish(result: Result<Vec<u8>, String>) -> u32 {
	let (status, bytes) = match result {
		Ok(bytes) => (0, bytes),
		Err(message) => (1, message.into_bytes()),
	};
	RESULT.with(|r| *r.borrow_mut() = bytes);
	status
}

/// The bytes the host wrote at `ptr`.
///
/// # Safety
///
/// `ptr` and `len` describe a buffer of [`alloc`] the host filled.
unsafe fn input<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
	if len == 0 {
		&[]
	} else {
		// SAFETY: the host passes a buffer of `alloc` of at least `len` bytes.
		unsafe { core::slice::from_raw_parts(ptr, len) }
	}
}

/// # Safety
///
/// `ptr` points to 32 bytes the host wrote.
unsafe fn bytes32(ptr: *const u8) -> [u8; 32] {
	// SAFETY: forwarded to the caller.
	unsafe { input(ptr, 32) }.try_into().expect("32 bytes; qed")
}

fn with_wallet<R>(handle: u32, action: impl FnOnce(&mut Wallet) -> R) -> Result<R, String> {
	WALLETS.with(|w| w.borrow_mut().1.get_mut(&handle).map(action).ok_or_else(|| "no such wallet".to_string()))
}

fn store(wallet: Wallet) -> u32 {
	WALLETS.with(|w| {
		let mut w = w.borrow_mut();
		w.0 = w.0.wrapping_add(1).max(1);
		let handle = w.0;
		w.1.insert(handle, wallet);
		handle
	})
}

fn transaction(bytes: &[u8]) -> Result<ShieldedTransaction, String> {
	ShieldedTransaction::decode(&mut &bytes[..]).map_err(|e| e.to_string())
}

/// A buffer of `len` bytes for the host to write in.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
	let mut buffer = Vec::<u8>::with_capacity(len.max(1));
	let ptr = buffer.as_mut_ptr();
	core::mem::forget(buffer);
	ptr
}

/// Release a buffer of [`alloc`].
///
/// # Safety
///
/// `ptr` comes from `alloc(len)` and is released once.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
	// SAFETY: the buffer was allocated by `alloc` with this capacity.
	drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
}

/// Address of the result buffer.
#[no_mangle]
pub extern "C" fn result_ptr() -> *const u8 {
	RESULT.with(|r| r.borrow().as_ptr())
}

/// Length of the result buffer.
#[no_mangle]
pub extern "C" fn result_len() -> usize {
	RESULT.with(|r| r.borrow().len())
}

/// The shielded account of the ML-DSA key with the 32-byte `seed`.
///
/// # Safety
///
/// `seed` points to 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn wallet_new(seed: *const u8) -> u32 {
	// SAFETY: forwarded to the caller.
	store(Wallet::new(shielded::spending_key(&unsafe { bytes32(seed) })))
}

/// The shielded account of `seed` restored from a state of [`wallet_state`]: a handle, or 0 with
/// the message in the result buffer.
///
/// # Safety
///
/// `seed` points to 32 bytes, `state` to `state_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn wallet_restore(seed: *const u8, state: *const u8, state_len: usize) -> u32 {
	// SAFETY: forwarded to the caller.
	let (seed, state) = unsafe { (bytes32(seed), input(state, state_len)) };
	match Wallet::restore(shielded::spending_key(&seed), state) {
		Ok(wallet) => store(wallet),
		Err(error) => {
			finish(Err(error.to_string()));
			0
		},
	}
}

/// Release a wallet.
#[no_mangle]
pub extern "C" fn wallet_free(handle: u32) {
	WALLETS.with(|w| w.borrow_mut().1.remove(&handle));
}

/// Its shielded address, `zte...`, UTF-8.
#[no_mangle]
pub extern "C" fn wallet_address(handle: u32) -> u32 {
	finish(with_wallet(handle, |w| shielded::encode_address(w.address()).into_bytes()))
}

/// Spendable balance.
#[no_mangle]
pub extern "C" fn wallet_balance(handle: u32) -> u64 {
	with_wallet(handle, |w| w.balance()).unwrap_or_default()
}

/// Balance spent by transactions not yet seen on chain.
#[no_mangle]
pub extern "C" fn wallet_pending(handle: u32) -> u64 {
	with_wallet(handle, |w| w.pending()).unwrap_or_default()
}

/// Commitments ingested: the position of the next one.
#[no_mangle]
pub extern "C" fn wallet_position(handle: u32) -> u64 {
	with_wallet(handle, |w| w.position()).unwrap_or_default()
}

/// Root of the note tree, 32 bytes.
#[no_mangle]
pub extern "C" fn wallet_root(handle: u32) -> u32 {
	finish(with_wallet(handle, |w| w.root().encode()))
}

/// Ingest the next `Shielded.Transacted` event: the value it pays to this account, 8 bytes
/// little-endian.
///
/// # Safety
///
/// `nullifiers` and `commitments` point to 64 bytes, the notes to their lengths.
#[no_mangle]
pub unsafe extern "C" fn wallet_ingest(
	handle: u32,
	position: u64,
	nullifiers: *const u8,
	commitments: *const u8,
	note_0: *const u8,
	note_0_len: usize,
	note_1: *const u8,
	note_1_len: usize,
) -> u32 {
	// SAFETY: forwarded to the caller.
	let (nullifiers, commitments, note_0, note_1) =
		unsafe { (input(nullifiers, 64), input(commitments, 64), input(note_0, note_0_len), input(note_1, note_1_len)) };
	let result = (|| {
		let event = Transacted {
			position,
			nullifiers: <[Digest; 2]>::decode(&mut &nullifiers[..]).map_err(|e| e.to_string())?,
			commitments: <[Digest; 2]>::decode(&mut &commitments[..]).map_err(|e| e.to_string())?,
			notes: [note_0.to_vec(), note_1.to_vec()],
		};
		let received = with_wallet(handle, |w| w.ingest(&event))?.map_err(|e| e.to_string())?;
		Ok(received.iter().map(|n| n.value).sum::<u64>().to_le_bytes().to_vec())
	})();
	finish(result)
}

/// Shield `value` of the transparent account `source`: the transaction.
///
/// # Safety
///
/// `source` points to 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn wallet_shield(handle: u32, source: *const u8, value: u64) -> u32 {
	// SAFETY: forwarded to the caller.
	let source = unsafe { bytes32(source) };
	finish(with_wallet(handle, |w| w.shield(source, value).map(|t| t.encode()).map_err(|e| e.to_string())).and_then(|r| r))
}

/// Send `value` to the shielded address `to`, UTF-8, paying `fee` from the pool: the transaction.
///
/// # Safety
///
/// `to` points to `to_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn wallet_send(handle: u32, to: *const u8, to_len: usize, value: u64, fee: u64) -> u32 {
	// SAFETY: forwarded to the caller.
	let to = unsafe { input(to, to_len) };
	let result = core::str::from_utf8(to)
		.map_err(|e| e.to_string())
		.and_then(|to| shielded::decode_address(to.trim()).map_err(|e| e.to_string()))
		.and_then(|to| with_wallet(handle, |w| w.send(&to, value, fee).map(|t| t.encode()).map_err(|e| e.to_string()))?);
	finish(result)
}

/// Send `value` from the pool to the transparent account `recipient`, paying `fee`: the
/// transaction.
///
/// # Safety
///
/// `recipient` points to 32 bytes.
#[no_mangle]
pub unsafe extern "C" fn wallet_unshield(handle: u32, recipient: *const u8, value: u64, fee: u64) -> u32 {
	// SAFETY: forwarded to the caller.
	let recipient = unsafe { bytes32(recipient) };
	finish(with_wallet(handle, |w| w.unshield(recipient, value, fee).map(|t| t.encode()).map_err(|e| e.to_string())).and_then(|r| r))
}

/// Mark the notes of a submitted transaction as spent until the chain shows it.
///
/// # Safety
///
/// `tx` points to `tx_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn wallet_mark_pending(handle: u32, tx: *const u8, tx_len: usize) -> u32 {
	// SAFETY: forwarded to the caller.
	let tx = unsafe { input(tx, tx_len) };
	finish(transaction(tx).and_then(|t| with_wallet(handle, |w| w.mark_pending(&t))).map(|()| Vec::new()))
}

/// Make spendable again the notes of transactions that were not included.
#[no_mangle]
pub extern "C" fn wallet_release_pending(handle: u32) {
	let _ = with_wallet(handle, |w| w.release_pending());
}

/// The tree and the notes, to store.
#[no_mangle]
pub extern "C" fn wallet_state(handle: u32) -> u32 {
	finish(with_wallet(handle, |w| w.state()))
}

/// Whether the UTF-8 text at `text` is a shielded address: 1 or 0.
///
/// # Safety
///
/// `text` points to `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn is_address(text: *const u8, len: usize) -> u32 {
	// SAFETY: forwarded to the caller.
	let text = unsafe { input(text, len) };
	core::str::from_utf8(text).is_ok_and(|t| shielded::decode_address(t.trim()).is_ok()) as u32
}

/// A `Shielded.transact` extrinsic without signer.
///
/// # Safety
///
/// `tx` points to `tx_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn transact_extrinsic(pallet: u8, call: u8, tx: *const u8, tx_len: usize) -> u32 {
	// SAFETY: forwarded to the caller.
	let tx = unsafe { input(tx, tx_len) };
	finish(transaction(tx).map(|t| shielded::transact_extrinsic(pallet, call, &t)))
}

/// Call data of `Shielded.<call>(transaction)`.
///
/// # Safety
///
/// `tx` points to `tx_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn call_data(pallet: u8, call: u8, tx: *const u8, tx_len: usize) -> u32 {
	// SAFETY: forwarded to the caller.
	let tx = unsafe { input(tx, tx_len) };
	finish(transaction(tx).map(|t| shielded::call_data(pallet, call, &t)))
}
