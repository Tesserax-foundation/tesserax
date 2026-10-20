//! Precompiles available to smart contracts in addition to the pallet-revive builtins.

use alloc::vec::Vec;
use alloy_core::{sol, sol_types::SolValue};
use core::{marker::PhantomData, num::NonZero};
use frame_support::weights::Weight;
use pallet_revive::precompiles::{AddressMatcher, Error, Ext, Precompile};

sol! {
	/// ML-DSA-65 (FIPS 204) signature verification.
	interface IMlDsa65 {
		/// Whether `signature` is a valid ML-DSA-65 signature of `message` under `publicKey`
		/// with the FIPS 204 context string `context`.
		///
		/// Returns `false` for malformed inputs. The context `"tesserax"` is reserved for
		/// transaction signatures and always fails, so that a transaction signature can never be
		/// accepted as a contract level signature.
		function verify(
			bytes calldata publicKey,
			bytes calldata context,
			bytes calldata message,
			bytes calldata signature
		) external view returns (bool);
	}
}

/// Address of the ML-DSA-65 verification precompile: `0x00000000000000000000000000000000d5a00000`.
pub const ML_DSA_65_PRECOMPILE: u16 = 0xD5A0;

/// Base cost of one ML-DSA-65 verification, about twice the measured verification time.
const VERIFY_BASE_WEIGHT: u64 = 1_000_000_000;

/// Cost per message byte (SHAKE256 absorption).
const VERIFY_WEIGHT_PER_BYTE: u64 = 10_000;

/// ML-DSA-65 signature verification for contracts.
pub struct MlDsa65Verify<T>(PhantomData<T>);

impl<T: pallet_revive::Config> Precompile for MlDsa65Verify<T> {
	type T = T;
	type Interface = IMlDsa65::IMlDsa65Calls;
	const MATCHER: AddressMatcher =
		AddressMatcher::Fixed(NonZero::new(ML_DSA_65_PRECOMPILE).unwrap());
	const HAS_CONTRACT_INFO: bool = false;

	fn call(
		_address: &[u8; 20],
		input: &Self::Interface,
		env: &mut impl Ext<T = Self::T>,
	) -> Result<Vec<u8>, Error> {
		let IMlDsa65::IMlDsa65Calls::verify(call) = input;
		let message_len = call.message.len() as u64;
		env.charge(Weight::from_parts(
			VERIFY_BASE_WEIGHT.saturating_add(VERIFY_WEIGHT_PER_BYTE.saturating_mul(message_len)),
			0,
		))?;

		let valid = call.context.as_ref() != tesserax_crypto::SIGNING_CONTEXT &&
			tesserax_crypto::verify_with_context(
				&call.signature,
				&call.message,
				&call.context,
				&call.publicKey,
			);
		Ok(valid.abi_encode())
	}
}
