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
use std::{
	collections::{BTreeMap, HashMap},
	path::{Path, PathBuf},
};
use tokio::sync::Mutex;

/// Version of the channel state files.
const STATE_VERSION: u32 = 1;

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
	/// Base64 ML-DSA-87 signature.
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

	/// Whether the voucher is signed by the ML-DSA-87 public key `signer`.
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayerChannel {
	pub id: u64,
	pub deposit: Amount,
	/// Cumulative amount of the latest voucher signed.
	pub spent: Amount,
}

/// State file of a [`ChannelPayer`].
#[derive(Serialize, Deserialize)]
struct PayerState {
	version: u32,
	kind: String,
	/// Genesis hash of the chain, hexadecimal.
	genesis: String,
	/// Address of the key signing the vouchers.
	signer: String,
	/// Address of the owner the channels are opened for, in delegated mode.
	owner: Option<String>,
	/// Channels by payee address.
	channels: BTreeMap<String, PayerChannel>,
}

/// Pays services over payment channels, one channel per payee, opened and topped up on demand.
///
/// In delegated mode (`owner` given), channels are opened for the owner through the agent's
/// delegation and the agent's key signs the vouchers.
///
/// With a state file ([`ChannelPayer::with_file`]), every channel opened, every top-up and every
/// voucher is written to disk before the voucher is returned: after a restart the payer resumes
/// its channels instead of opening new ones, and never signs a voucher below one it already gave.
/// A state file must be used by one payer at a time.
pub struct ChannelPayer {
	client: Client,
	key: Keypair,
	owner: Option<AccountId>,
	/// Deposit of a new channel, and amount of a top-up.
	deposit: Amount,
	genesis: [u8; 32],
	channels: Mutex<HashMap<String, PayerChannel>>,
	file: Option<PathBuf>,
}

impl ChannelPayer {
	/// A payer keeping its channels in memory only.
	pub fn new(client: Client, key: Keypair, owner: Option<AccountId>, deposit: Amount) -> Self {
		let genesis = client.genesis_hash();
		ChannelPayer { client, key, owner, deposit, genesis, channels: Mutex::new(HashMap::new()), file: None }
	}

	/// A payer keeping its channels in the state file `path`, created if missing. Refuses a file
	/// written for another chain, key or owner.
	pub fn with_file(
		client: Client,
		key: Keypair,
		owner: Option<AccountId>,
		deposit: Amount,
		path: impl Into<PathBuf>,
	) -> Result<Self> {
		let path = path.into();
		let mut payer = Self::new(client, key, owner, deposit);
		let channels = match crate::store::load::<serde_json::Value>(&path)? {
			None => HashMap::new(),
			// Files written before the versioned format: the channels by payee only.
			Some(value) if value.get("version").is_none() => serde_json::from_value(value)
				.map_err(|e| Error::Storage(format!("{} is not a payer state file: {e}", path.display())))?,
			Some(value) => {
				let state: PayerState = serde_json::from_value(value)
					.map_err(|e| Error::Storage(format!("{} is not a payer state file: {e}", path.display())))?;
				payer.check(&state, &path)?;
				state.channels.into_iter().collect()
			},
		};
		payer.channels = Mutex::new(channels);
		payer.file = Some(path);
		Ok(payer)
	}

	fn check(&self, state: &PayerState, path: &Path) -> Result<()> {
		let mismatch = |what: &str| Err(Error::Storage(format!("{} was written for another {what}", path.display())));
		if state.version != STATE_VERSION || state.kind != "payer" {
			return Err(Error::Storage(format!(
				"{} is not a payer state file of version {STATE_VERSION}",
				path.display()
			)));
		}
		if state.genesis != hex(&self.genesis) {
			return mismatch("chain");
		}
		if state.signer != self.key.address() {
			return mismatch("key");
		}
		if state.owner != self.owner.as_ref().map(format_address) {
			return mismatch("owner");
		}
		Ok(())
	}

	/// Write `channels` to the state file, if any.
	fn persist(&self, channels: &HashMap<String, PayerChannel>) -> Result<()> {
		let Some(path) = &self.file else { return Ok(()) };
		let state = PayerState {
			version: STATE_VERSION,
			kind: "payer".into(),
			genesis: hex(&self.genesis),
			signer: self.key.address(),
			owner: self.owner.as_ref().map(format_address),
			channels: channels.iter().map(|(payee, channel)| (payee.clone(), channel.clone())).collect(),
		};
		crate::store::save(path, &state)
	}

	/// Resume from channels used before, by payee address.
	pub async fn restore(&self, channels: HashMap<String, PayerChannel>) -> Result<()> {
		let mut current = self.channels.lock().await;
		current.extend(channels);
		self.persist(&current)
	}

	/// Channels in use, by payee address.
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
		let mut channel = match channels.get(&requirement.payee).cloned() {
			// A channel still open on chain.
			Some(channel) if self.client.channel(channel.id).await?.is_some_and(|c| c.closes_at.is_none()) =>
				channel,
			_ => {
				let id = match &self.owner {
					Some(owner) => self.client.agent_open_channel(&self.key, owner, &payee, deposit).await?,
					None => self.client.open_channel(&self.key, &payee, deposit).await?,
				};
				let channel = PayerChannel { id, deposit, spent: Amount(0) };
				channels.insert(requirement.payee.clone(), channel.clone());
				self.persist(&channels)?;
				channel
			},
		};

		let amount = Amount(channel.spent.0 + requirement.price.0);
		if amount > channel.deposit {
			let top_up = Amount(self.deposit.0.max(amount.0 - channel.deposit.0));
			self.client.top_up_channel(&self.key, self.owner.as_ref(), channel.id, top_up).await?;
			channel.deposit = Amount(channel.deposit.0 + top_up.0);
			channels.insert(requirement.payee.clone(), channel.clone());
			self.persist(&channels)?;
		}

		// Recorded before the voucher leaves the payer.
		let previous = channels.insert(requirement.payee.clone(), PayerChannel { spent: amount, ..channel.clone() });
		if let Err(error) = self.persist(&channels) {
			match previous {
				Some(previous) => channels.insert(requirement.payee.clone(), previous),
				None => channels.remove(&requirement.payee),
			};
			return Err(error);
		}
		Ok(Voucher::sign(&self.key, &self.genesis, channel.id, amount))
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
	/// State of the channel on chain, read at `fetched`; unknown after a restart.
	info: Option<(ChannelInfo, std::time::Instant)>,
	/// Amount of the latest voucher accepted.
	accepted: Amount,
	/// Latest voucher accepted and not claimed yet.
	unclaimed: Option<Voucher>,
}

/// What a [`ChannelReceiver`] keeps of a channel in its state file.
#[derive(Serialize, Deserialize)]
struct ReceivedState {
	accepted: Amount,
	unclaimed: Option<Voucher>,
}

/// State file of a [`ChannelReceiver`].
#[derive(Serialize, Deserialize)]
struct ReceiverState {
	version: u32,
	kind: String,
	/// Genesis hash of the chain, hexadecimal.
	genesis: String,
	/// Address of the service.
	payee: String,
	/// Channels by id.
	channels: BTreeMap<u64, ReceivedState>,
}

/// Verifies the vouchers paying a service and claims them.
///
/// With a state file ([`ChannelReceiver::with_file`]), the amount accepted on each channel and
/// the latest voucher not claimed yet are written to disk before a voucher is accepted: after a
/// restart the service still claims what it was paid, and refuses vouchers it already accepted.
/// A state file must be used by one receiver at a time.
pub struct ChannelReceiver {
	client: Client,
	key: Keypair,
	genesis: [u8; 32],
	channels: Mutex<HashMap<u64, ReceivedChannel>>,
	file: Option<PathBuf>,
}

/// How long the state of a channel is trusted before reading it again from the chain.
const CHANNEL_REFRESH: std::time::Duration = std::time::Duration::from_secs(60);

impl ChannelReceiver {
	/// A receiver for the service signing with `key`, payee of the channels, keeping its state in
	/// memory only.
	pub fn new(client: Client, key: Keypair) -> Self {
		let genesis = client.genesis_hash();
		ChannelReceiver { client, key, genesis, channels: Mutex::new(HashMap::new()), file: None }
	}

	/// A receiver keeping its state in the file `path`, created if missing. Refuses a file written
	/// for another chain or service.
	pub fn with_file(client: Client, key: Keypair, path: impl Into<PathBuf>) -> Result<Self> {
		let path = path.into();
		let mut receiver = Self::new(client, key);
		if let Some(state) = crate::store::load::<ReceiverState>(&path)? {
			if state.version != STATE_VERSION || state.kind != "receiver" {
				return Err(Error::Storage(format!(
					"{} is not a receiver state file of version {STATE_VERSION}",
					path.display()
				)));
			}
			if state.genesis != hex(&receiver.genesis) {
				return Err(Error::Storage(format!("{} was written for another chain", path.display())));
			}
			if state.payee != receiver.key.address() {
				return Err(Error::Storage(format!("{} was written for another service", path.display())));
			}
			let channels = state
				.channels
				.into_iter()
				.map(|(id, c)| (id, ReceivedChannel { info: None, accepted: c.accepted, unclaimed: c.unclaimed }))
				.collect();
			receiver.channels = Mutex::new(channels);
		}
		receiver.file = Some(path);
		Ok(receiver)
	}

	/// Write `channels` to the state file, if any.
	fn persist(&self, channels: &HashMap<u64, ReceivedChannel>) -> Result<()> {
		let Some(path) = &self.file else { return Ok(()) };
		let state = ReceiverState {
			version: STATE_VERSION,
			kind: "receiver".into(),
			genesis: hex(&self.genesis),
			payee: self.key.address(),
			channels: channels
				.iter()
				.map(|(id, c)| (*id, ReceivedState { accepted: c.accepted, unclaimed: c.unclaimed.clone() }))
				.collect(),
		};
		crate::store::save(path, &state)
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

		let stale = channels.get(&voucher.channel).is_none_or(|c| match &c.info {
			Some((info, fetched)) => fetched.elapsed() > CHANNEL_REFRESH || voucher.amount > info.deposit,
			None => true,
		});
		if stale {
			let info = self
				.client
				.channel(voucher.channel)
				.await
				.map_err(|e| reject(&format!("cannot read the channel: {e}"), None))?
				.ok_or_else(|| reject("unknown channel", None))?;
			let entry = channels.entry(voucher.channel).or_insert_with(|| ReceivedChannel {
				info: None,
				accepted: info.claimed,
				unclaimed: None,
			});
			entry.accepted = entry.accepted.max(info.claimed);
			entry.info = Some((info, std::time::Instant::now()));
		}
		let channel = channels.get_mut(&voucher.channel).expect("inserted above; qed");
		let info = &channel.info.as_ref().expect("fetched above; qed").0;

		if info.payee != self.key.address() {
			return Err(reject("the channel pays another payee", None));
		}
		if let Some(closes_at) = info.closes_at {
			let now = self.client.best_block_number().await.map_err(|e| reject(&e.to_string(), None))?;
			if closes_at.saturating_sub(now) < CLOSE_SAFETY_BLOCKS {
				return Err(reject("the channel is closing", Some(channel.accepted)));
			}
		}
		if voucher.amount > info.deposit {
			return Err(reject("the voucher exceeds the deposit", Some(channel.accepted)));
		}
		if voucher.amount.0 < channel.accepted.0.saturating_add(price.0) {
			return Err(reject("the voucher does not pay the price", Some(channel.accepted)));
		}
		if !voucher.verify(&self.genesis, &info.signer_public) {
			return Err(reject("invalid voucher signature", Some(channel.accepted)));
		}

		// Recorded before the request is served.
		let (id, amount) = (voucher.channel, voucher.amount);
		let previous = (channel.accepted, channel.unclaimed.replace(voucher));
		channel.accepted = amount;
		if self.persist(&channels).is_err() {
			let channel = channels.get_mut(&id).expect("present above; qed");
			(channel.accepted, channel.unclaimed) = previous;
			return Err(reject("the service cannot record the voucher", None));
		}
		Ok(amount)
	}

	/// Claim the latest voucher of every channel. A voucher stays recorded until its claim
	/// succeeds; vouchers of channels closed in the meantime, or already claimed, are dropped.
	/// Returns the outcomes of the claims.
	pub async fn claim_all(&self) -> Result<Vec<TxOutcome>> {
		let pending: Vec<Voucher> =
			self.channels.lock().await.values().filter_map(|c| c.unclaimed.clone()).collect();
		let mut outcomes = Vec::new();
		for voucher in pending {
			let info = self.client.channel(voucher.channel).await?;
			let claimable = info.as_ref().is_some_and(|info| info.claimed < voucher.amount);
			if claimable {
				outcomes.push(self.client.claim_channel(&self.key, &voucher).await?);
			}
			let mut channels = self.channels.lock().await;
			if info.is_none() {
				// Closed: nothing can be claimed any more, and its vouchers are refused.
				channels.remove(&voucher.channel);
			} else if let Some(channel) = channels.get_mut(&voucher.channel) {
				// A newer voucher may have arrived during the claim.
				if channel.unclaimed.as_ref().is_some_and(|v| v.amount <= voucher.amount) {
					channel.unclaimed = None;
				}
				if let Some((info, _)) = &mut channel.info {
					info.claimed = info.claimed.max(voucher.amount);
				}
			}
			self.persist(&channels)?;
		}
		Ok(outcomes)
	}

	/// Amount accepted and not claimed yet, over all channels.
	pub async fn unclaimed(&self) -> Amount {
		let channels = self.channels.lock().await;
		Amount(
			channels
				.values()
				.filter_map(|c| {
					let claimed = c.info.as_ref().map_or(0, |(info, _)| info.claimed.0);
					c.unclaimed.as_ref().map(|v| v.amount.0.saturating_sub(claimed))
				})
				.sum(),
		)
	}
}
