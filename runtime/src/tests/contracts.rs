//! Smart contracts: a Solidity contract compiled to PolkaVM with `resolc`, deployed and called
//! with ML-DSA signed extrinsics.

use super::*;
use alloy_core::{
	primitives::{Address, Bytes, FixedBytes, U256 as SolU256},
	sol,
	sol_types::SolCall,
};
use frame_support::weights::Weight;
use sp_core::crypto::Pair as _;
use pallet_revive::{ExecConfig, TransactionLimits, H160, U256};

/// `runtime/fixtures/Counter.sol` compiled with `resolc` 1.4.0.
const COUNTER: &[u8] = include_bytes!("../../fixtures/Counter.polkavm");

sol! {
	interface ICounter {
		function count() external view returns (uint256);
		function increment() external;
		function verifyMlDsa(bytes publicKey, bytes context, bytes message, bytes signature)
			external view returns (bool);
		function recover(bytes32 hash, uint8 v, bytes32 r, bytes32 s)
			external pure returns (address);
	}
}

const WEIGHT_LIMIT: Weight = Weight::from_parts(1_000_000_000_000, 10 * 1024 * 1024);
const DEPOSIT_LIMIT: Balance = 100 * UNIT;

fn revive_call(call: pallet_revive::Call<Runtime>) -> RuntimeCall {
	RuntimeCall::Revive(call)
}

/// Deploy the counter contract from Alice.
fn instantiate_counter() -> ApplyExtrinsicResult {
	let alice = Keyring::Alice;
	let xt = signed(
		revive_call(pallet_revive::Call::instantiate_with_code {
			value: 0,
			weight_limit: WEIGHT_LIMIT,
			storage_deposit_limit: DEPOSIT_LIMIT,
			code: COUNTER.to_vec(),
			data: Vec::new(),
			salt: None,
		}),
		alice.to_account_id(),
		&alice.pair(),
		System::account_nonce(alice.to_account_id()),
	);
	apply(xt)
}

/// Deploy the counter contract from Alice and return its address.
fn deploy_counter() -> H160 {
	assert_eq!(instantiate_counter(), Ok(Ok(())));
	System::events()
		.into_iter()
		.find_map(|record| match record.event {
			RuntimeEvent::Revive(pallet_revive::Event::Instantiated { contract, .. }) =>
				Some(contract),
			_ => None,
		})
		.expect("the contract is instantiated")
}

/// Call `contract` in a signed extrinsic from `sender`.
fn call_contract(sender: Keyring, contract: H160, data: Vec<u8>) -> ApplyExtrinsicResult {
	let xt = signed(
		revive_call(pallet_revive::Call::call {
			dest: contract,
			value: 0,
			weight_limit: WEIGHT_LIMIT,
			storage_deposit_limit: DEPOSIT_LIMIT,
			data,
		}),
		sender.to_account_id(),
		&sender.pair(),
		System::account_nonce(sender.to_account_id()),
	);
	apply(xt)
}

/// Read-only call returning the raw output.
fn dry_run(contract: H160, data: Vec<u8>) -> Vec<u8> {
	let result = Revive::bare_call(
		RuntimeOrigin::signed(Keyring::Alice.to_account_id()),
		contract,
		U256::zero(),
		TransactionLimits::WeightAndDeposit {
			weight_limit: WEIGHT_LIMIT,
			deposit_limit: DEPOSIT_LIMIT,
		},
		data,
		&ExecConfig::new_substrate_tx(),
	);
	let output = result.result.expect("the call succeeds");
	assert!(!output.did_revert(), "the call reverted");
	output.data
}

fn count(contract: H160) -> SolU256 {
	ICounter::countCall::abi_decode_returns(&dry_run(contract, ICounter::countCall {}.abi_encode()))
		.unwrap()
}

fn verify(contract: H160, public: &[u8], context: &[u8], message: &[u8], signature: &[u8]) -> bool {
	let data = ICounter::verifyMlDsaCall {
		publicKey: Bytes::copy_from_slice(public),
		context: Bytes::copy_from_slice(context),
		message: Bytes::copy_from_slice(message),
		signature: Bytes::copy_from_slice(signature),
	}
	.abi_encode();
	ICounter::verifyMlDsaCall::abi_decode_returns(&dry_run(contract, data)).unwrap()
}

#[test]
fn solidity_contract_is_deployed_and_called_with_ml_dsa_transactions() {
	new_test_ext().execute_with(|| {
		let contract = deploy_counter();
		assert_eq!(count(contract), SolU256::ZERO);

		assert_eq!(
			call_contract(Keyring::Bob, contract, ICounter::incrementCall {}.abi_encode()),
			Ok(Ok(()))
		);
		assert_eq!(
			call_contract(Keyring::Alice, contract, ICounter::incrementCall {}.abi_encode()),
			Ok(Ok(()))
		);
		assert_eq!(count(contract), SolU256::from(2));
	});
}

#[test]
fn contracts_verify_ml_dsa_signatures() {
	new_test_ext().execute_with(|| {
		let contract = deploy_counter();
		let pair = Keyring::Charlie.pair();
		let public = pair.public().0;
		let context = b"my-dapp";
		let message = b"transfer 10 tokens to Bob";
		let signature = pair.sign_with_context(message, context).0;

		assert!(verify(contract, &public, context, message, &signature));
		assert!(!verify(contract, &public, context, b"transfer 99 tokens to Bob", &signature));
		assert!(!verify(contract, &public, b"other-dapp", message, &signature));
		assert!(!verify(contract, &Keyring::Dave.pair().public().0, context, message, &signature));
		assert!(!verify(contract, &public[..100], context, message, &signature));
	});
}

#[test]
fn transaction_signatures_are_rejected_by_contracts() {
	new_test_ext().execute_with(|| {
		let contract = deploy_counter();
		let pair = Keyring::Charlie.pair();
		let message = b"any payload";
		let signature = pair.sign_with_context(message, tesserax_crypto::SIGNING_CONTEXT).0;
		assert!(tesserax_crypto::verify_with_context(
			&signature,
			message,
			tesserax_crypto::SIGNING_CONTEXT,
			&pair.public().0
		));
		assert!(!verify(contract, &pair.public().0, tesserax_crypto::SIGNING_CONTEXT, message, &signature));
	});
}

#[test]
fn ecrecover_is_not_available() {
	new_test_ext().execute_with(|| {
		let contract = deploy_counter();
		// A valid secp256k1 signature would recover a non-zero address. Without the precompile,
		// `ecrecover` returns the zero address.
		let data = ICounter::recoverCall {
			hash: FixedBytes::repeat_byte(1),
			v: 27,
			r: FixedBytes::repeat_byte(2),
			s: FixedBytes::repeat_byte(3),
		}
		.abi_encode();
		let recovered = ICounter::recoverCall::abi_decode_returns(&dry_run(contract, data)).unwrap();
		assert_eq!(recovered, Address::ZERO);
	});
}

#[test]
fn evm_bytecode_is_rejected() {
	new_test_ext().execute_with(|| {
		let alice = Keyring::Alice;
		// `PUSH1 0 PUSH1 0 RETURN`: valid EVM bytecode.
		let evm_code = vec![0x60, 0x00, 0x60, 0x00, 0xf3];
		let xt = signed(
			revive_call(pallet_revive::Call::instantiate_with_code {
				value: 0,
				weight_limit: WEIGHT_LIMIT,
				storage_deposit_limit: DEPOSIT_LIMIT,
				code: evm_code,
				data: Vec::new(),
				salt: None,
			}),
			alice.to_account_id(),
			&alice.pair(),
			0,
		);
		assert!(matches!(apply(xt), Ok(Err(_))));
	});
}

#[test]
fn ethereum_transactions_are_rejected() {
	new_test_ext().execute_with(|| {
		let alice = Keyring::Alice;

		// `eth_transact` declares an unbounded weight: it never fits in a block.
		let xt = signed(
			revive_call(pallet_revive::Call::eth_transact { payload: vec![0xc0] }),
			alice.to_account_id(),
			&alice.pair(),
			0,
		);
		assert!(apply(xt).is_err());

		// The other Ethereum entry points are filtered.
		let xt = signed(
			revive_call(pallet_revive::Call::eth_call {
				dest: H160::repeat_byte(1),
				value: U256::zero(),
				weight_limit: Weight::from_parts(1_000_000, 0),
				eth_gas_limit: U256::zero(),
				data: Vec::new(),
				transaction_encoded: Vec::new(),
				effective_gas_price: U256::zero(),
				encoded_len: 0,
			}),
			alice.to_account_id(),
			&alice.pair(),
			0,
		);
		assert_eq!(apply(xt), Ok(Err(frame_system::Error::<Runtime>::CallFiltered.into())));
	});
}

#[test]
fn ml_dsa_accounts_are_mapped_automatically() {
	new_test_ext().execute_with(|| {
		use pallet_revive::AddressMapper;
		type Mapper = <Runtime as pallet_revive::Config>::AddressMapper;

		let alice = Keyring::Alice.to_account_id();
		assert!(Mapper::is_mapped(&alice));
		assert_eq!(Mapper::to_account_id(&Mapper::to_address(&alice)), alice);
	});
}

type Mapper = <Runtime as pallet_revive::Config>::AddressMapper;

/// Raw storage key of the map `Revive.<item>`, whose keys are addresses hashed with `Identity`.
fn revive_key(item: &[u8], address: &H160) -> Vec<u8> {
	let mut key = frame_support::storage::storage_prefix(b"Revive", item).to_vec();
	key.extend_from_slice(address.as_bytes());
	key
}

/// Give the address of `account` to `owner`, as if both had the same 160 bits and `owner` had
/// been mapped first.
fn take_address(account: &AccountId, owner: &AccountId) -> H160 {
	use pallet_revive::AddressMapper;
	let address = Mapper::to_address(account);
	sp_io::storage::set(&revive_key(b"OriginalAccount", &address), &owner.encode());
	address
}

#[test]
fn an_address_acts_only_for_the_account_it_belongs_to() {
	new_test_ext().execute_with(|| {
		use pallet_revive::AddressMapper;
		let contract = deploy_counter();
		let alice = Keyring::Alice.to_account_id();
		let bob = Keyring::Bob.to_account_id();
		let address = take_address(&bob, &alice);

		// Bob has the address of Alice: he must not act with it in contracts.
		assert!(!Mapper::is_mapped(&bob));
		assert!(Mapper::is_mapped(&alice));
		assert_eq!(Mapper::to_account_id(&address), alice);
		let call = call_contract(Keyring::Bob, contract, ICounter::incrementCall {}.abi_encode());
		assert!(matches!(call, Ok(Err(_))), "{call:?}");
		assert_eq!(count(contract), SolU256::ZERO);

		// Removing the mapping of Bob, as when his account is reaped, keeps the one of Alice.
		assert_eq!(Mapper::unmap(&bob), Ok(()));
		assert_eq!(Mapper::to_account_id(&address), alice);
		assert_eq!(Mapper::map_no_deposit_unchecked(&bob), Err(pallet_revive::Error::<Runtime>::AccountAlreadyMapped.into()));
	});
}

#[test]
fn contracts_are_not_created_at_the_address_of_an_account() {
	new_test_ext().execute_with(|| {
		use pallet_revive::AddressMapper;
		let alice = Keyring::Alice.to_account_id();
		let next = pallet_revive::create1(&Mapper::to_address(&alice), u64::from(System::account_nonce(&alice)));
		sp_io::storage::set(&revive_key(b"OriginalAccount", &next), &Keyring::Bob.to_account_id().encode());

		let result = instantiate_counter();
		assert!(matches!(result, Ok(Err(_))), "{result:?}");
		assert!(!System::events()
			.iter()
			.any(|record| matches!(record.event, RuntimeEvent::Revive(pallet_revive::Event::Instantiated { .. }))));
	});
}

#[test]
fn accounts_are_not_mapped_to_the_address_of_a_contract() {
	new_test_ext().execute_with(|| {
		use pallet_revive::AddressMapper;
		let contract = deploy_counter();
		let charlie = Keyring::Charlie.to_account_id();
		// A contract at the address of Charlie, before his account exists.
		let address = Mapper::to_address(&charlie);
		let info = sp_io::storage::get(&revive_key(b"AccountInfoOf", &contract)).expect("the contract is stored");
		sp_io::storage::set(&revive_key(b"AccountInfoOf", &address), &info);

		let alice = Keyring::Alice;
		let xt = signed(transfer_call(&Keyring::Charlie, 10 * UNIT), alice.to_account_id(), &alice.pair(), System::account_nonce(alice.to_account_id()));
		assert_eq!(apply(xt), Ok(Ok(())));
		assert!(!Mapper::is_mapped(&charlie));
		assert_ne!(Mapper::to_account_id(&address), charlie);
	});
}

