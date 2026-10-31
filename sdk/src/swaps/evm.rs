//! The payment side of swaps on Ethereum, Base and Arbitrum: the contract `TesseraxSwap`
//! (`swaps/evm/`), at the same address on every chain.

use super::{Asset, Chain, Payment};
use crate::{AccountId, Error, Result};
use alloy::{
	network::EthereumWallet,
	primitives::{address, Address},
	providers::{DynProvider, Provider, ProviderBuilder},
	rpc::types::Filter,
	sol,
	sol_types::SolEvent,
};

pub use alloy::{
	primitives::{Address as EvmAddress, B256, U256},
	signers::local::PrivateKeySigner,
};

/// Address of `TesseraxSwap` on every chain, deployed with the deterministic CREATE2 deployer.
pub const SWAP_CONTRACT: Address = address!("eA2f4f08D2A0f1972031B98De63f4B0395c256Fc");

sol! {
	#[sol(rpc)]
	contract TesseraxSwap {
		event Locked(
			bytes32 indexed id,
			address indexed sender,
			address indexed recipient,
			address token,
			uint256 amount,
			bytes32 hash,
			uint64 timelock,
			uint64 offer,
			bytes32 taker
		);
		event Claimed(bytes32 indexed id, bytes32 secret);
		event Refunded(bytes32 indexed id);

		function lock(
			address recipient,
			address token,
			uint256 amount,
			bytes32 hash,
			uint64 timelock,
			uint64 offer,
			bytes32 taker
		) external payable returns (bytes32 id);
		function claim(bytes32 id, bytes32 secret) external;
		function refund(bytes32 id) external;
		function locks(bytes32 id) external view returns (
			address sender,
			address recipient,
			address token,
			uint256 amount,
			bytes32 hash,
			uint64 timelock,
			uint8 state
		);
	}

	#[sol(rpc)]
	interface IERC20 {
		function approve(address spender, uint256 amount) external returns (bool);
		function allowance(address owner, address spender) external view returns (uint256);
	}
}

fn evm(error: impl std::fmt::Display) -> Error {
	Error::Evm(error.to_string())
}

/// An EVM chain with its stablecoins: the mainnets for the mainnet of Tesserax, their testnets
/// for its testnet.
#[derive(Clone, Debug)]
pub struct Network {
	pub chain: Chain,
	pub name: &'static str,
	pub chain_id: u64,
	/// A public RPC; users may give their own.
	pub rpc_url: &'static str,
	pub usdc: Address,
	pub usdt: Option<Address>,
	pub explorer: &'static str,
}

impl Network {
	/// The token of `asset`, `Address::ZERO` for ETH, if the chain has it.
	pub fn token(&self, asset: Asset) -> Option<Address> {
		match asset {
			Asset::Native => Some(Address::ZERO),
			Asset::Usdc => Some(self.usdc),
			Asset::Usdt => self.usdt,
		}
	}

	/// The asset of `token`, if it is one of this chain.
	pub fn asset(&self, token: Address) -> Option<Asset> {
		[Asset::Native, Asset::Usdc, Asset::Usdt].into_iter().find(|a| self.token(*a) == Some(token))
	}
}

/// The EVM chains, mainnets or testnets. Token addresses are those of Circle for USDC and of
/// Tether for USDT (USD₮0 on Arbitrum); Base has no native USDT.
pub fn networks(testnet: bool) -> [Network; 3] {
	if testnet {
		[
			Network {
				chain: Chain::Ethereum,
				name: "Ethereum Sepolia",
				chain_id: 11_155_111,
				rpc_url: "https://ethereum-sepolia-rpc.publicnode.com",
				usdc: address!("1c7D4B196Cb0C7B01d743Fbc6116a902379C7238"),
				usdt: None,
				explorer: "https://sepolia.etherscan.io",
			},
			Network {
				chain: Chain::Base,
				name: "Base Sepolia",
				chain_id: 84_532,
				rpc_url: "https://sepolia.base.org",
				usdc: address!("036CbD53842c5426634e7929541eC2318f3dCF7e"),
				usdt: None,
				explorer: "https://sepolia.basescan.org",
			},
			Network {
				chain: Chain::Arbitrum,
				name: "Arbitrum Sepolia",
				chain_id: 421_614,
				rpc_url: "https://sepolia-rollup.arbitrum.io/rpc",
				usdc: address!("75faf114eafb1BDbe2F0316DF893fd58CE46AA4d"),
				usdt: None,
				explorer: "https://sepolia.arbiscan.io",
			},
		]
	} else {
		[
			Network {
				chain: Chain::Ethereum,
				name: "Ethereum",
				chain_id: 1,
				rpc_url: "https://ethereum-rpc.publicnode.com",
				usdc: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
				usdt: Some(address!("dAC17F958D2ee523a2206206994597C13D831ec7")),
				explorer: "https://etherscan.io",
			},
			Network {
				chain: Chain::Base,
				name: "Base",
				chain_id: 8_453,
				rpc_url: "https://mainnet.base.org",
				usdc: address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
				usdt: None,
				explorer: "https://basescan.org",
			},
			Network {
				chain: Chain::Arbitrum,
				name: "Arbitrum",
				chain_id: 42_161,
				rpc_url: "https://arb1.arbitrum.io/rpc",
				usdc: address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
				usdt: Some(address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9")),
				explorer: "https://arbiscan.io",
			},
		]
	}
}

/// State of a payment lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockState {
	Locked,
	Claimed,
	Refunded,
}

/// A lock as `Evm::lock_status` reads it.
#[derive(Clone, Copy, Debug)]
pub struct LockStatus {
	pub state: LockState,
	pub sender: Address,
	pub recipient: Address,
	pub token: Address,
	pub amount: U256,
	pub hash: [u8; 32],
	/// Unix time from which the sender may take it back.
	pub timelock: u64,
}

/// A payment locked for a maker.
#[derive(Clone, Debug)]
pub struct Lock {
	pub id: B256,
	pub sender: Address,
	pub recipient: Address,
	pub token: Address,
	pub amount: U256,
	pub hash: [u8; 32],
	/// Unix time from which the sender may take it back.
	pub timelock: u64,
	/// The offer it pays, and the Tesserax account of its taker, from its event.
	pub offer: u64,
	pub taker: AccountId,
	pub state: LockState,
	/// Block of its event.
	pub block: u64,
}

impl Lock {
	/// The payment of `payment` this lock holds, on `network`, in the smallest unit of the asset,
	/// if it holds that asset.
	pub fn amount_of(&self, network: &Network, payment: Payment) -> Option<u128> {
		(network.chain == payment.chain && network.asset(self.token) == Some(payment.asset))
			.then(|| u128::try_from(self.amount).ok())
			.flatten()
	}
}

/// An EVM chain, read and signed for with an optional key.
pub struct Evm {
	pub network: Network,
	provider: DynProvider,
	signer: Option<Address>,
}

impl Evm {
	/// Connect to `network` through `rpc_url`, its public RPC by default, signing with `key` if
	/// given. The chain id must be the one of the network.
	pub async fn connect(network: Network, rpc_url: Option<&str>, key: Option<PrivateKeySigner>) -> Result<Self> {
		let url = rpc_url.unwrap_or(network.rpc_url).parse().map_err(evm)?;
		let signer = key.as_ref().map(|k| k.address());
		let provider = match key {
			Some(key) => ProviderBuilder::new().wallet(EthereumWallet::from(key)).connect_http(url).erased(),
			None => ProviderBuilder::new().connect_http(url).erased(),
		};
		let chain_id = provider.get_chain_id().await.map_err(evm)?;
		if chain_id != network.chain_id {
			return Err(Error::Evm(format!("the RPC is on chain {chain_id}, not {} ({})", network.name, network.chain_id)));
		}
		Ok(Evm { network, provider, signer })
	}

	/// The address signing, if any.
	pub fn address(&self) -> Option<Address> {
		self.signer
	}

	fn contract(&self) -> TesseraxSwap::TesseraxSwapInstance<&DynProvider> {
		TesseraxSwap::new(SWAP_CONTRACT, &self.provider)
	}

	/// The latest block, and its unix time.
	pub async fn head(&self) -> Result<(u64, u64)> {
		let number = self.provider.get_block_number().await.map_err(evm)?;
		let block = self
			.provider
			.get_block_by_number(number.into())
			.await
			.map_err(evm)?
			.ok_or_else(|| Error::Evm("no latest block".into()))?;
		Ok((number, block.header.timestamp))
	}

	/// The current state of lock `id`.
	pub async fn lock_state(&self, id: B256) -> Result<Option<LockState>> {
		Ok(self.lock_status(id).await?.map(|s| s.state))
	}

	/// The current state of lock `id`, its hash, and when its sender may take it back.
	pub async fn lock_status(&self, id: B256) -> Result<Option<LockStatus>> {
		let l = self.contract().locks(id).call().await.map_err(evm)?;
		let state = match l.state {
			1 => LockState::Locked,
			2 => LockState::Claimed,
			3 => LockState::Refunded,
			_ => return Ok(None),
		};
		Ok(Some(LockStatus {
			state,
			sender: l.sender,
			recipient: l.recipient,
			token: l.token,
			amount: l.amount,
			hash: l.hash.0,
			timelock: l.timelock,
		}))
	}

	/// Locks created in blocks `from` to `to` for `recipient`, if given, with their current state.
	pub async fn locks(&self, from: u64, to: u64, recipient: Option<Address>) -> Result<Vec<Lock>> {
		let mut filter = Filter::new()
			.address(SWAP_CONTRACT)
			.event_signature(TesseraxSwap::Locked::SIGNATURE_HASH)
			.from_block(from)
			.to_block(to);
		if let Some(recipient) = recipient {
			filter = filter.topic3(recipient.into_word());
		}
		let logs = self.provider.get_logs(&filter).await.map_err(evm)?;
		let mut found = Vec::new();
		for log in logs {
			let block = log.block_number.unwrap_or_default();
			let event = TesseraxSwap::Locked::decode_log(&log.inner).map_err(evm)?.data;
			let state = self.lock_state(event.id).await?.unwrap_or(LockState::Locked);
			found.push(Lock {
				id: event.id,
				sender: event.sender,
				recipient: event.recipient,
				token: event.token,
				amount: event.amount,
				hash: event.hash.0,
				timelock: event.timelock,
				offer: event.offer,
				taker: AccountId::from(event.taker.0),
				state,
				block,
			});
		}
		Ok(found)
	}

	/// Lock `amount` of `asset` for `recipient` until `timelock`, under `hash`, for `offer` and the
	/// Tesserax account `taker`, approving the token first if needed. Returns the id of the lock.
	#[allow(clippy::too_many_arguments)]
	pub async fn lock_payment(
		&self,
		asset: Asset,
		recipient: Address,
		amount: U256,
		hash: [u8; 32],
		timelock: u64,
		offer: u64,
		taker: &AccountId,
	) -> Result<B256> {
		let owner = self.signer.ok_or_else(|| Error::Evm("no key to sign with".into()))?;
		// A payment sent to an address without the contract would be lost.
		if self.provider.get_code_at(SWAP_CONTRACT).await.map_err(evm)?.is_empty() {
			return Err(Error::Evm(format!("the swap contract is not deployed on {}", self.network.name)));
		}
		let token = self.network.token(asset).ok_or_else(|| Error::Evm(format!("{asset:?} is not on {}", self.network.name)))?;
		if token != Address::ZERO {
			let erc20 = IERC20::new(token, &self.provider);
			let allowance = erc20.allowance(owner, SWAP_CONTRACT).call().await.map_err(evm)?;
			if allowance < amount {
				erc20.approve(SWAP_CONTRACT, amount).send().await.map_err(evm)?.get_receipt().await.map_err(evm)?;
			}
		}
		let taker = B256::from(taker.0);
		let contract = self.contract();
		let call = contract.lock(recipient, token, amount, B256::from(hash), timelock, offer, taker);
		let call = if token == Address::ZERO { call.value(amount) } else { call };
		let receipt = call.send().await.map_err(evm)?.get_receipt().await.map_err(evm)?;
		if !receipt.status() {
			return Err(Error::Evm("the lock reverted".into()));
		}
		receipt
			.decoded_log::<TesseraxSwap::Locked>()
			.map(|log| log.data.id)
			.ok_or_else(|| Error::Evm("no lock in the receipt".into()))
	}

	/// Pay lock `id` to its recipient with `secret`. Returns the transaction hash.
	pub async fn claim(&self, id: B256, secret: [u8; 32]) -> Result<B256> {
		let receipt = self
			.contract()
			.claim(id, B256::from(secret))
			.send()
			.await
			.map_err(evm)?
			.get_receipt()
			.await
			.map_err(evm)?;
		receipt.status().then_some(receipt.transaction_hash).ok_or_else(|| Error::Evm("the claim reverted".into()))
	}

	/// The secret that claimed lock `id`, from its `Claimed` event in blocks `from` to the latest,
	/// if it was claimed.
	pub async fn secret_of(&self, id: B256, from: u64) -> Result<Option<[u8; 32]>> {
		let filter = Filter::new()
			.address(SWAP_CONTRACT)
			.event_signature(TesseraxSwap::Claimed::SIGNATURE_HASH)
			.topic1(id)
			.from_block(from);
		for log in self.provider.get_logs(&filter).await.map_err(evm)? {
			let event = TesseraxSwap::Claimed::decode_log(&log.inner).map_err(evm)?.data;
			if event.id == id {
				return Ok(Some(event.secret.0));
			}
		}
		Ok(None)
	}

	/// Return lock `id` to its sender once its timelock passed. Returns the transaction hash.
	pub async fn refund(&self, id: B256) -> Result<B256> {
		let receipt =
			self.contract().refund(id).send().await.map_err(evm)?.get_receipt().await.map_err(evm)?;
		receipt.status().then_some(receipt.transaction_hash).ok_or_else(|| Error::Evm("the refund reverted".into()))
	}
}
