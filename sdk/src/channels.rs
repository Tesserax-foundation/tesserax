//! Micropayments: payment channels, vouchers and the HTTP 402 protocol. See
//! `docs/micropayments.md`.

use crate::{
	client::TxOutcome,
	keys::{format_address, AccountId, Keypair},
	runtime::{
		self,
		runtime_types::{
			pallet_channels::pallet::Call as ChannelsCall,
			tesserax_crypto::{Public as RuntimePublic, Signature as RuntimeSignature},
			tesserax_runtime::RuntimeCall,
		},
	},
	Amount, Client, Error, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// FIPS 204 context of voucher signatures.
pub const VOUCHER_CONTEXT: &[u8] = b"tesserax-channel";

/// Response header of a service asking for payment, with status 402.
pub const PAYMENT_HEADER: &str = "Tesserax-Payment";

/// Request header carrying a voucher.
pub const VOUCHER_HEADER: &str = "Tesserax-Voucher";

/// Blocks before a channel closes under which a service stops accepting its vouchers: it must
/// have time to claim (about 30 minutes).
pub const CLOSE_SAFETY_BLOCKS: u32 = 100;

/// The message a voucher signs: SCALE encoding of `(genesis_hash, channel, amount)`.
pub fn voucher_message(genesis: &[u8; 32], channel: u64, amount: u128) -> Vec<u8> {
	let mut message = Vec::with_capacity(56);
	message.extend_from_slice(genesis);
	message.extend_from_slice(&channel.to_le_bytes());
	message.extend_from_slice(&amount.to_le_bytes());
	message
}

/// A voucher: the payer owes `amount` in total on `channel`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Voucher {
	pub channel: u64,
	pub amount: Amount,
	/// Base64 ML-DSA-65 signature.
	#[serde(with = "base64_bytes")]
	pub signature: Vec<u8>,
}

mod base64_bytes {
	use base64::{engine::general_purpose::STANDARD, Engine};
	use serde::{Deserialize, Deserializer, Serializer};

	pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
		s.serialize_str(&STANDARD.encode(bytes))
	}

	pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
		let text = String::deserialize(d)?;
		STANDARD.decode(text.as_bytes()).map_err(serde::de::Error::custom)
	}
}

impl Voucher {
	/// Sign a voucher of `amount` on `channel` with `key`.
	pub fn sign(key: &Keypair, genesis: &[u8; 32], channel: u64, amount: Amount) -> Self {
		let signature = key.sign_with_context(&voucher_message(genesis, channel, amount.0), VOUCHER_CONTEXT);
		Voucher { channel, amount, signature }
	}

	/// Whether the voucher is signed by the ML-DSA-65 public key `signer`.
	pub fn verify(&self, genesis: &[u8; 32], signer: &[u8]) -> bool {
		tesserax_crypto::verify_with_context(
			&self.signature,
			&voucher_message(genesis, self.channel, self.amount.0),
			VOUCHER_CONTEXT,
			signer,
		)
	}

	/// Value of the `Tesserax-Voucher` header.
	pub fn to_header(&self) -> String {
		serde_json::to_string(self).expect("a voucher serializes; qed")
	}

	/// Parse a `Tesserax-Voucher` header.
	pub fn from_header(value: &str) -> Result<Self> {
		serde_json::from_str(value).map_err(|e| Error::InvalidInput(format!("malformed voucher: {e}")))
	}
}

/// What a service asks for, in the `Tesserax-Payment` header of a 402 response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentRequired {
	/// Address of the service.
	pub payee: String,
	/// Price of the request.
	pub price: Amount,
	/// Genesis hash of the chain, hexadecimal.
	pub genesis: String,
	/// Cumulative amount already accepted on the channel of a rejected voucher.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub accepted: Option<Amount>,
	/// Why a voucher was rejected.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

impl PaymentRequired {
	pub fn to_header(&self) -> String {
		serde_json::to_string(self).expect("a requirement serializes; qed")
	}

	pub fn from_header(value: &str) -> Result<Self> {
		serde_json::from_str(value)
			.map_err(|e| Error::InvalidInput(format!("malformed payment requirement: {e}")))
	}
}

/// A payment channel.
#[derive(Clone, Debug, Serialize)]
pub struct ChannelInfo {
	pub id: u64,
	pub payer: String,
	pub payee: String,
	/// Account of the key signing the vouchers.
	pub signer: String,
	#[serde(skip)]
	pub signer_public: Vec<u8>,
	pub deposit: Amount,
	pub claimed: Amount,
	/// Block from which the close can be finalized, if the payer requested it.
	pub closes_at: Option<u32>,
}

fn hex(bytes: &[u8]) -> String {
	format!("0x{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

impl Client {
	/// Genesis hash of the chain, which vouchers sign.
	pub fn genesis_hash(&self) -> [u8; 32] {
		self.api().genesis_hash().0
	}

	/// The channel `id`, if open.
	pub async fn channel(&self, id: u64) -> Result<Option<ChannelInfo>> {
		let query = runtime::storage().channels().channels(id);
		let Some(c) = self.storage().await?.fetch(&query).await? else { return Ok(None) };
		let signer_public = c.signer.0.to_vec();
		Ok(Some(ChannelInfo {
			id,
			payer: format_address(&c.payer),
			payee: format_address(&c.payee),
			signer: format_address(&AccountId::from(<[u8; 32]>::from(tesserax_crypto::Public(c.signer.0).to_account_id()))),
			signer_public,
			deposit: Amount(c.deposit),
			claimed: Amount(c.claimed),
			closes_at: c.closes_at,
		}))
	}

	/// Open channels paid by `payer`.
	pub async fn channels_of(&self, payer: &AccountId) -> Result<Vec<ChannelInfo>> {
		let storage = self.storage().await?;
		let mut entries = storage.iter(runtime::storage().channels().channels_iter()).await?;
		let mut found = Vec::new();
		while let Some(entry) = entries.next().await {
			let entry = entry?;
			if entry.value.payer != *payer {
				continue;
			}
			// The map is hashed with Blake2_128Concat: the channel id ends the key.
			let key = &entry.key_bytes[entry.key_bytes.len().saturating_sub(8)..];
			let id = u64::from_le_bytes(key.try_into().map_err(|_| Error::NotFound("malformed channel key".into()))?);
			found.extend(self.channel(id).await?);
		}
		Ok(found)
	}

	fn open_call(payee: &AccountId, signer: &Keypair, deposit: Amount) -> ChannelsCall {
		ChannelsCall::open {
			payee: payee.clone(),
			signer: RuntimePublic(signer.public_key()),
			deposit: deposit.0,
		}
	}

	/// Open a channel from `payer` to `payee` with `deposit`, signed by the payer, whose vouchers
	/// are signed by the payer's key. Returns the channel id.
	pub async fn open_channel(&self, payer: &Keypair, payee: &AccountId, deposit: Amount) -> Result<u64> {
		let call = runtime::tx().channels().open(payee.clone(), RuntimePublic(payer.public_key()), deposit.0);
		let events = self.submit_events(&call, payer).await?.1;
		Self::opened(&events)
	}

	/// Open a channel from `owner` to `payee` with `deposit`, through the delegation of `agent`,
	/// whose vouchers are signed by the agent's key. Returns the channel id.
	pub async fn agent_open_channel(
		&self,
		agent: &Keypair,
		owner: &AccountId,
		payee: &AccountId,
		deposit: Amount,
	) -> Result<u64> {
		let call = runtime::tx()
			.agents()
			.execute(owner.clone(), RuntimeCall::Channels(Self::open_call(payee, agent, deposit)));
		let events = self.submit_events(&call, agent).await?.1;
		Self::opened(&events)
	}

	fn opened(events: &subxt::blocks::ExtrinsicEvents<crate::TesseraxConfig>) -> Result<u64> {
		events
			.find_first::<runtime::channels::events::Opened>()?
			.map(|e| e.id)
			.ok_or_else(|| Error::NotFound("the channel was not opened".into()))
	}

	/// Add `amount` to the deposit of channel `id`: signed by the payer, or by an agent for its
	/// owner when `owner` is given.
	pub async fn top_up_channel(
		&self,
		signer: &Keypair,
		owner: Option<&AccountId>,
		id: u64,
		amount: Amount,
	) -> Result<TxOutcome> {
		let call = ChannelsCall::top_up { id, amount: amount.0 };
		match owner {
			Some(owner) => self.agent_execute(signer, owner, RuntimeCall::Channels(call)).await,
			None => self.submit(&runtime::tx().channels().top_up(id, amount.0), signer).await,
		}
	}

	/// Request to close channel `id`, as its payer or its payer's agent.
	pub async fn request_close_channel(
		&self,
		signer: &Keypair,
		owner: Option<&AccountId>,
		id: u64,
	) -> Result<TxOutcome> {
		match owner {
			Some(owner) =>
				self.agent_execute(signer, owner, RuntimeCall::Channels(ChannelsCall::request_close { id })).await,
			None => self.submit(&runtime::tx().channels().request_close(id), signer).await,
		}
	}

	/// Finalize the close of channel `id` once its close period is over.
	pub async fn finalize_close_channel(&self, signer: &Keypair, id: u64) -> Result<TxOutcome> {
		self.submit(&runtime::tx().channels().finalize_close(id), signer).await
	}

	/// Claim `voucher` as the payee of its channel.
	pub async fn claim_channel(&self, payee: &Keypair, voucher: &Voucher) -> Result<TxOutcome> {
		let signature: [u8; tesserax_crypto::SIGNATURE_SIZE] = voucher
			.signature
			.as_slice()
			.try_into()
			.map_err(|_| Error::InvalidInput("malformed voucher signature".into()))?;
		let call = runtime::tx().channels().claim(voucher.channel, voucher.amount.0, RuntimeSignature(signature));
		self.submit(&call, payee).await
	}

	/// Close channel `id` as its payee: what was not claimed returns to the payer.
	pub async fn close_channel(&self, payee: &Keypair, id: u64) -> Result<TxOutcome> {
		self.submit(&runtime::tx().channels().close(id), payee).await
	}
}

/// A channel the payer uses.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayerChannel {
	pub id: u64,
	pub deposit: Amount,
	/// Cumulative amount of the latest voucher signed.
	pub spent: Amount,
}

/// Pays services over payment channels, one channel per payee, opened and topped up on demand.
///
/// In delegated mode (`owner` given), channels are opened for the owner through the agent's
/// delegation and the agent's key signs the vouchers.
pub struct ChannelPayer {
	client: Client,
	key: Keypair,
	owner: Option<AccountId>,
	/// Deposit of a new channel, and amount of a top-up.
	deposit: Amount,
	genesis: [u8; 32],
	channels: Mutex<HashMap<String, PayerChannel>>,
}

impl ChannelPayer {
	pub fn new(client: Client, key: Keypair, owner: Option<AccountId>, deposit: Amount) -> Self {
		let genesis = client.genesis_hash();
		ChannelPayer { client, key, owner, deposit, genesis, channels: Mutex::new(HashMap::new()) }
	}

	/// Resume from channels used before, by payee address.
	pub async fn restore(&self, channels: HashMap<String, PayerChannel>) {
		self.channels.lock().await.extend(channels);
	}

	/// Channels in use, by payee address, to persist across restarts.
	pub async fn channels(&self) -> HashMap<String, PayerChannel> {
		self.channels.lock().await.clone()
	}

	/// A voucher paying `requirement`, opening or topping up the channel to the payee if needed.
	/// Refuses prices above `max_price`, other chains, and services claiming more than what was
	/// paid.
	pub async fn pay(&self, requirement: &PaymentRequired, max_price: Amount) -> Result<Voucher> {
		if requirement.genesis.to_lowercase() != hex(&self.genesis) {
			return Err(Error::InvalidInput("the service is on another chain".into()));
		}
		if requirement.price > max_price {
			return Err(Error::InvalidInput(format!(
				"the service asks {} TESS, more than the maximum of {max_price}",
				requirement.price
			)));
		}
		if requirement.price.0 == 0 {
			return Err(Error::InvalidInput("the service asks a price of zero".into()));
		}
		let payee = crate::parse_address(&requirement.payee)?;
		let mut channels = self.channels.lock().await;

		if let (Some(channel), Some(accepted)) = (channels.get(&requirement.payee), requirement.accepted) {
			if accepted > channel.spent {
				return Err(Error::InvalidInput(format!(
					"the service says {accepted} TESS were paid on channel {}, but only {} were",
					channel.id, channel.spent
				)));
			}
		}

		let deposit = self.deposit.max(requirement.price);
		let channel = match channels.get(&requirement.payee).cloned() {
			// A channel still open on chain.
			Some(channel) if self.client.channel(channel.id).await?.is_some_and(|c| c.closes_at.is_none()) =>
				channel,
			_ => {
				let id = match &self.owner {
					Some(owner) => self.client.agent_open_channel(&self.key, owner, &payee, deposit).await?,
					None => self.client.open_channel(&self.key, &payee, deposit).await?,
				};
				PayerChannel { id, deposit, spent: Amount(0) }
			},
		};
		let mut channel = channel;

		let amount = Amount(channel.spent.0 + requirement.price.0);
		if amount > channel.deposit {
			let top_up = Amount(self.deposit.0.max(amount.0 - channel.deposit.0));
			self.client.top_up_channel(&self.key, self.owner.as_ref(), channel.id, top_up).await?;
			channel.deposit = Amount(channel.deposit.0 + top_up.0);
		}

		let voucher = Voucher::sign(&self.key, &self.genesis, channel.id, amount);
		channel.spent = amount;
		channels.insert(requirement.payee.clone(), channel);
		Ok(voucher)
	}
}

/// Why a service refused a voucher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
	pub reason: String,
	/// Cumulative amount already accepted on the channel, if known.
	pub accepted: Option<Amount>,
}

struct ReceivedChannel {
	info: ChannelInfo,
	fetched: std::time::Instant,
	/// Amount of the latest voucher accepted.
	accepted: Amount,
	/// Latest voucher accepted and not claimed yet.
	unclaimed: Option<Voucher>,
}

/// Verifies the vouchers paying a service and claims them.
pub struct ChannelReceiver {
	client: Client,
	key: Keypair,
	genesis: [u8; 32],
	channels: Mutex<HashMap<u64, ReceivedChannel>>,
}

/// How long the state of a channel is trusted before reading it again from the chain.
const CHANNEL_REFRESH: std::time::Duration = std::time::Duration::from_secs(60);

impl ChannelReceiver {
	/// A receiver for the service signing with `key`, payee of the channels.
	pub fn new(client: Client, key: Keypair) -> Self {
		let genesis = client.genesis_hash();
		ChannelReceiver { client, key, genesis, channels: Mutex::new(HashMap::new()) }
	}

	/// The `Tesserax-Payment` header asking for `price`, after `rejection` if any.
	pub fn payment_required(&self, price: Amount, rejection: Option<&Rejection>) -> String {
		PaymentRequired {
			payee: self.key.address(),
			price,
			genesis: hex(&self.genesis),
			accepted: rejection.and_then(|r| r.accepted),
			error: rejection.map(|r| r.reason.clone()),
		}
		.to_header()
	}

	/// Check the `Tesserax-Voucher` header of a request paying `price`. On success the voucher
	/// is recorded for claiming and its amount returned.
	pub async fn verify(&self, header: &str, price: Amount) -> core::result::Result<Amount, Rejection> {
		let reject = |reason: &str, accepted: Option<Amount>| Rejection { reason: reason.into(), accepted };
		let voucher = Voucher::from_header(header).map_err(|e| reject(&e.to_string(), None))?;
		let mut channels = self.channels.lock().await;

		let stale = channels.get(&voucher.channel).is_none_or(|c| {
			c.fetched.elapsed() > CHANNEL_REFRESH || voucher.amount > c.info.deposit
		});
		if stale {
			let info = self
				.client
				.channel(voucher.channel)
				.await
				.map_err(|e| reject(&format!("cannot read the channel: {e}"), None))?
				.ok_or_else(|| reject("unknown channel", None))?;
			let entry = channels.entry(voucher.channel).or_insert_with(|| ReceivedChannel {
				accepted: info.claimed,
				unclaimed: None,
				info: info.clone(),
				fetched: std::time::Instant::now(),
			});
			entry.accepted = entry.accepted.max(info.claimed);
			entry.info = info;
			entry.fetched = std::time::Instant::now();
		}
		let channel = channels.get_mut(&voucher.channel).expect("inserted above; qed");

		if channel.info.payee != self.key.address() {
			return Err(reject("the channel pays another payee", None));
		}
		if let Some(closes_at) = channel.info.closes_at {
			let now = self.client.best_block_number().await.map_err(|e| reject(&e.to_string(), None))?;
			if closes_at.saturating_sub(now) < CLOSE_SAFETY_BLOCKS {
				return Err(reject("the channel is closing", Some(channel.accepted)));
			}
		}
		if voucher.amount > channel.info.deposit {
			return Err(reject("the voucher exceeds the deposit", Some(channel.accepted)));
		}
		if voucher.amount.0 < channel.accepted.0.saturating_add(price.0) {
			return Err(reject("the voucher does not pay the price", Some(channel.accepted)));
		}
		if !voucher.verify(&self.genesis, &channel.info.signer_public) {
			return Err(reject("invalid voucher signature", Some(channel.accepted)));
		}
		channel.accepted = voucher.amount;
		let amount = voucher.amount;
		channel.unclaimed = Some(voucher);
		Ok(amount)
	}

	/// Claim the latest voucher of every channel, and close the channels whose payer requested
	/// it. Returns the outcomes of the claims.
	pub async fn claim_all(&self) -> Result<Vec<TxOutcome>> {
		let pending: Vec<Voucher> = {
			let mut channels = self.channels.lock().await;
			channels.values_mut().filter_map(|c| c.unclaimed.take()).collect()
		};
		let mut outcomes = Vec::new();
		for voucher in pending {
			match self.client.claim_channel(&self.key, &voucher).await {
				Ok(outcome) => outcomes.push(outcome),
				Err(error) => {
					// Keep the voucher for the next attempt.
					let mut channels = self.channels.lock().await;
					if let Some(channel) = channels.get_mut(&voucher.channel) {
						if channel.unclaimed.as_ref().is_none_or(|v| v.amount < voucher.amount) {
							channel.unclaimed = Some(voucher);
						}
					}
					return Err(error);
				},
			}
		}
		Ok(outcomes)
	}

	/// Amount accepted and not claimed yet, over all channels.
	pub async fn unclaimed(&self) -> Amount {
		let channels = self.channels.lock().await;
		Amount(
			channels
				.values()
				.filter_map(|c| c.unclaimed.as_ref().map(|v| v.amount.0.saturating_sub(c.info.claimed.0)))
				.sum(),
		)
	}
}
