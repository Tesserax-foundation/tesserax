//! On-chain governance (`docs/governance.md`): referenda, votes with conviction, and the
//! whitelist of the technical committee.
//!
//! ```no_run
//! # async fn example() -> Result<(), tesserax_sdk::Error> {
//! use tesserax_sdk::{governance::{Conviction, Track}, Amount, Client, Keypair};
//!
//! let client = Client::connect("wss://rpc.tesserax.app:21207").await?;
//! let alice = Keypair::from_uri("<24 words>")?;
//! let code = std::fs::read("tesserax_runtime.compact.compressed.wasm").unwrap();
//! let index = client.propose_runtime_upgrade(&alice, code).await?;
//! client.vote(&alice, index, true, Amount::from_tess("100")?, Conviction::Locked3x).await?;
//! # Ok(()) }
//! ```

use crate::{
	client::TxOutcome,
	runtime::{
		self,
		runtime_types::{
			frame_support::{
				dispatch::RawOrigin,
				traits::{preimages::Bounded, schedule::DispatchTime},
			},
			pallet_conviction_voting::{
				conviction::Conviction as RuntimeConviction,
				vote::{AccountVote, Vote},
			},
			pallet_referenda::types::{Curve, ReferendumInfo},
			tesserax_runtime::{governance::origins::pallet_origins::Origin, OriginCaller, RuntimeCall},
		},
	},
	AccountId, Amount, Client, Error, Keypair, Result,
};
use subxt::{
	config::{substrate::BlakeTwo256, Hasher},
	utils::{MultiAddress, H256},
};

fn blake2_256(bytes: &[u8]) -> [u8; 32] {
	BlakeTwo256.hash(bytes).0
}

/// A call of `pallet` with its arguments already encoded: for the calls that carry another call,
/// which the generated types cannot decode.
struct RawCall {
	pallet: &'static str,
	call: &'static str,
	arguments: Vec<u8>,
}

impl subxt::tx::Payload for RawCall {
	fn encode_call_data_to(
		&self,
		metadata: &subxt::Metadata,
		out: &mut Vec<u8>,
	) -> core::result::Result<(), subxt::ext::subxt_core::Error> {
		use subxt::ext::subxt_core::error::{Error as CoreError, MetadataError};
		let pallet = metadata
			.pallet_by_name(self.pallet)
			.ok_or_else(|| CoreError::Metadata(MetadataError::PalletNameNotFound(self.pallet.into())))?;
		let call = pallet
			.call_variant_by_name(self.call)
			.ok_or_else(|| CoreError::Metadata(MetadataError::CallNameNotFound(self.call.into())))?
			.index;
		out.extend([pallet.index(), call]);
		out.extend(&self.arguments);
		Ok(())
	}
}

/// What a referendum may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
	/// Anything, a runtime upgrade included. 1,000 TESS of decision deposit, 14 days of vote.
	Root,
	/// A call the technical committee whitelisted, dispatched as root. 500 TESS, 2 days.
	Urgent,
	/// Cancel or kill another referendum. 100 TESS, 1 day.
	Cancellation,
}

impl Track {
	/// Its id on chain.
	pub fn id(self) -> u16 {
		match self {
			Track::Root => 0,
			Track::Urgent => 1,
			Track::Cancellation => 2,
		}
	}

	/// The track of `id`.
	pub fn from_id(id: u16) -> Option<Self> {
		[Track::Root, Track::Urgent, Track::Cancellation].into_iter().find(|t| t.id() == id)
	}

	fn origin(self) -> OriginCaller {
		match self {
			Track::Root => OriginCaller::system(RawOrigin::Root),
			Track::Urgent => OriginCaller::Origins(Origin::WhitelistedCaller),
			Track::Cancellation => OriginCaller::Origins(Origin::ReferendumCanceller),
		}
	}
}

/// How long a vote stays locked after its referendum, multiplying it: `Locked1x` a week, each
/// step doubling it, up to `Locked6x`, 32 weeks. `None` counts a tenth and locks nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conviction {
	None = 0,
	Locked1x = 1,
	Locked2x = 2,
	Locked3x = 3,
	Locked4x = 4,
	Locked5x = 5,
	Locked6x = 6,
}

impl Conviction {
	/// The conviction of `n`, 0 to 6.
	pub fn from_number(n: u8) -> Option<Self> {
		use Conviction::*;
		[None, Locked1x, Locked2x, Locked3x, Locked4x, Locked5x, Locked6x].get(usize::from(n)).copied()
	}

	fn to_runtime(self) -> RuntimeConviction {
		match self {
			Conviction::None => RuntimeConviction::None,
			Conviction::Locked1x => RuntimeConviction::Locked1x,
			Conviction::Locked2x => RuntimeConviction::Locked2x,
			Conviction::Locked3x => RuntimeConviction::Locked3x,
			Conviction::Locked4x => RuntimeConviction::Locked4x,
			Conviction::Locked5x => RuntimeConviction::Locked5x,
			Conviction::Locked6x => RuntimeConviction::Locked6x,
		}
	}

	/// The vote byte: the conviction, with the high bit set for aye.
	fn vote(self, aye: bool) -> Vote {
		Vote((if aye { 0x80 } else { 0 }) | self as u8)
	}
}

/// Where a referendum is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
	/// Waiting for its decision deposit or the end of its preparation.
	Preparing,
	/// Being voted on.
	Deciding,
	/// Passing, if it keeps passing until the block `until`.
	Confirming { until: u32 },
	/// Approved at `at`: its call is scheduled.
	Approved { at: u32 },
	Rejected { at: u32 },
	Cancelled { at: u32 },
	TimedOut { at: u32 },
	Killed { at: u32 },
}

/// A referendum.
#[derive(Clone, Debug)]
pub struct Referendum {
	pub index: u32,
	pub status: Status,
	/// Its track, while it is ongoing.
	pub track: Option<Track>,
	/// Hash of its call, while it is ongoing.
	pub proposal: Option<[u8; 32]>,
	/// Votes for and against, conviction included, and support: the TESS voting for it.
	pub ayes: Amount,
	pub nays: Amount,
	pub support: Amount,
	/// Block it was submitted at, and block its decision started at.
	pub submitted: Option<u32>,
	pub deciding_since: Option<u32>,
	pub decision_deposit_placed: bool,
}

fn referendum<H>(
	index: u32,
	info: ReferendumInfo<
		u16,
		OriginCaller,
		u32,
		Bounded<RuntimeCall, H>,
		u128,
		runtime::runtime_types::pallet_conviction_voting::types::Tally<u128>,
		AccountId,
		(u32, u32),
	>,
) -> Referendum {
	let mut r = Referendum {
		index,
		status: Status::Preparing,
		track: None,
		proposal: None,
		ayes: Amount(0),
		nays: Amount(0),
		support: Amount(0),
		submitted: None,
		deciding_since: None,
		decision_deposit_placed: false,
	};
	match info {
		ReferendumInfo::Ongoing(status) => {
			r.track = Track::from_id(status.track);
			r.proposal = match status.proposal {
				Bounded::Legacy { hash } | Bounded::Lookup { hash, .. } => Some(hash.0),
				Bounded::Inline(bytes) => Some(blake2_256(&bytes.0)),
				_ => None,
			};
			r.ayes = Amount(status.tally.ayes);
			r.nays = Amount(status.tally.nays);
			r.support = Amount(status.tally.support);
			r.submitted = Some(status.submitted);
			r.decision_deposit_placed = status.decision_deposit.is_some();
			if let Some(deciding) = status.deciding {
				r.deciding_since = Some(deciding.since);
				r.status = match deciding.confirming {
					Some(until) => Status::Confirming { until },
					None => Status::Deciding,
				};
			}
		},
		ReferendumInfo::Approved(at, ..) => r.status = Status::Approved { at },
		ReferendumInfo::Rejected(at, ..) => r.status = Status::Rejected { at },
		ReferendumInfo::Cancelled(at, ..) => r.status = Status::Cancelled { at },
		ReferendumInfo::TimedOut(at, ..) => r.status = Status::TimedOut { at },
		ReferendumInfo::Killed(at) => r.status = Status::Killed { at },
	}
	r
}

/// What a track needs at `fraction` of its decision period, in billionths: approval and support.
pub fn threshold(curve: &Curve, fraction: u32) -> u32 {
	match curve {
		Curve::LinearDecreasing { length, floor, ceil } => {
			let length = u64::from(length.0.max(1));
			let progress = u64::from(fraction).min(length);
			let drop = u64::from(ceil.0.saturating_sub(floor.0)) * progress / length;
			ceil.0.saturating_sub(drop as u32).max(floor.0)
		},
		_ => 0,
	}
}

impl Client {
	/// The referendum `index`, if any.
	pub async fn referendum(&self, index: u32) -> Result<Option<Referendum>> {
		let query = runtime::storage().referenda().referendum_info_for(index);
		Ok(self.storage().await?.fetch(&query).await?.map(|info| referendum(index, info)))
	}

	/// Every referendum, the latest first.
	pub async fn referenda(&self) -> Result<Vec<Referendum>> {
		let count = self.storage().await?.fetch_or_default(&runtime::storage().referenda().referendum_count()).await?;
		let mut found = Vec::new();
		for index in (0..count).rev() {
			if let Some(r) = self.referendum(index).await? {
				found.push(r);
			}
		}
		Ok(found)
	}

	/// Fail unless `call` starts with the index of a pallet of the runtime and of one of its calls.
	fn check_call(&self, call: &[u8]) -> Result<()> {
		let metadata = self.api().metadata();
		let known = call.len() >= 2 &&
			metadata
				.pallets()
				.any(|p| p.index() == call[0] && p.call_variants().is_some_and(|c| c.iter().any(|v| v.index == call[1])));
		if known {
			Ok(())
		} else {
			Err(Error::InvalidInput("not a call of the runtime".into()))
		}
	}

	/// Store the encoded call `call` on chain, for a referendum, unless it is already there.
	/// Returns its hash.
	pub async fn note_preimage(&self, signer: &Keypair, call: Vec<u8>) -> Result<[u8; 32]> {
		let hash = blake2_256(&call);
		let status = runtime::storage().preimage().request_status_for(H256(hash));
		if self.storage().await?.fetch(&status).await?.is_none() {
			self.submit(&runtime::tx().preimage().note_preimage(call), signer).await?;
		}
		Ok(hash)
	}

	/// Propose the encoded call `call` on `track`: its preimage, the referendum and its decision
	/// deposit, signed by `signer`. Returns the index of the referendum.
	pub async fn propose(&self, signer: &Keypair, track: Track, call: Vec<u8>) -> Result<u32> {
		self.check_call(&call)?;
		let len = call.len() as u32;
		let hash = self.note_preimage(signer, call).await?;
		let submit = runtime::tx().referenda().submit(
			track.origin(),
			Bounded::Lookup { hash: H256(hash), len },
			DispatchTime::After(0),
		);
		let (_, events) = self.submit_events(&submit, signer).await?;
		let index = events
			.find_first::<runtime::referenda::events::Submitted>()?
			.ok_or_else(|| Error::NotFound("the referendum submitted".into()))?
			.index;
		self.submit(&runtime::tx().referenda().place_decision_deposit(index), signer).await?;
		Ok(index)
	}

	/// Propose to replace the runtime by `code`, on the root track.
	pub async fn propose_runtime_upgrade(&self, signer: &Keypair, code: Vec<u8>) -> Result<u32> {
		let call = self.api().tx().call_data(&runtime::tx().system().set_code(code))?;
		self.propose(signer, Track::Root, call).await
	}

	/// Propose to dispatch, on the urgent track, the encoded call `call` the technical committee
	/// whitelisted.
	pub async fn propose_whitelisted(&self, signer: &Keypair, call: Vec<u8>) -> Result<u32> {
		self.check_call(&call)?;
		let dispatch = RawCall { pallet: "Whitelist", call: "dispatch_whitelisted_call_with_preimage", arguments: call };
		let call = self.api().tx().call_data(&dispatch)?;
		self.propose(signer, Track::Urgent, call).await
	}

	/// Propose to cancel the referendum `index`, refunding its deposits, or to kill it, slashing
	/// them, on the cancellation track.
	pub async fn propose_cancellation(&self, signer: &Keypair, index: u32, kill: bool) -> Result<u32> {
		let call = if kill {
			self.api().tx().call_data(&runtime::tx().referenda().kill(index))?
		} else {
			self.api().tx().call_data(&runtime::tx().referenda().cancel(index))?
		};
		self.propose(signer, Track::Cancellation, call).await
	}

	/// Vote on the referendum `index`, for or against, with `amount` and `conviction`. TESS still
	/// vesting do not vote.
	pub async fn vote(&self, signer: &Keypair, index: u32, aye: bool, amount: Amount, conviction: Conviction) -> Result<TxOutcome> {
		let vote = AccountVote::Standard { vote: conviction.vote(aye), balance: amount.0 };
		self.submit(&runtime::tx().conviction_voting().vote(index, vote), signer).await
	}

	/// Remove the vote of `signer` on the referendum `index`. Its lock remains if the referendum
	/// ended in its favour, until its conviction period is over.
	pub async fn remove_vote(&self, signer: &Keypair, index: u32) -> Result<TxOutcome> {
		let call = runtime::tx().conviction_voting().remove_vote(None, index);
		self.submit(&call, signer).await
	}

	/// Release the TESS of `signer` whose vote locks have expired, on every track.
	pub async fn unlock_votes(&self, signer: &Keypair) -> Result<()> {
		for track in [Track::Root, Track::Urgent, Track::Cancellation] {
			let call = runtime::tx().conviction_voting().unlock(track.id(), MultiAddress::Id(signer.account_id()));
			self.submit(&call, signer).await?;
		}
		Ok(())
	}

	/// Delegate the votes of `signer` on `track` to `to`, with `amount` and `conviction`.
	pub async fn delegate_votes(
		&self,
		signer: &Keypair,
		track: Track,
		to: &AccountId,
		amount: Amount,
		conviction: Conviction,
	) -> Result<TxOutcome> {
		let call = runtime::tx().conviction_voting().delegate(
			track.id(),
			MultiAddress::Id(to.clone()),
			conviction.to_runtime(),
			amount.0,
		);
		self.submit(&call, signer).await
	}

	/// Stop delegating the votes of `signer` on `track`.
	pub async fn undelegate_votes(&self, signer: &Keypair, track: Track) -> Result<TxOutcome> {
		self.submit(&runtime::tx().conviction_voting().undelegate(track.id()), signer).await
	}

	/// The members of the technical committee.
	pub async fn committee(&self) -> Result<Vec<AccountId>> {
		let members = self.storage().await?.fetch_or_default(&runtime::storage().technical_committee().members()).await?;
		Ok(members)
	}

	/// Approve, as a member of the technical committee, the whitelisting of the call of hash
	/// `call_hash`: the first member proposes it, the second one's approval whitelists it.
	/// Returns whether the call is whitelisted.
	pub async fn whitelist(&self, member: &Keypair, call_hash: [u8; 32]) -> Result<bool> {
		let proposal = runtime::tx().whitelist().whitelist_call(H256(call_hash));
		let call = self.api().tx().call_data(&proposal)?;
		let length = call.len() as u32;
		let motion = blake2_256(&call);
		let voting = runtime::storage().technical_committee().voting(H256(motion));
		let index = match self.storage().await?.fetch(&voting).await? {
			Some(votes) => votes.index,
			None => {
				use subxt::ext::codec::{Compact, Encode};
				let mut arguments = Compact(2u32).encode();
				arguments.extend(&call);
				arguments.extend(Compact(length).encode());
				let propose = RawCall { pallet: "TechnicalCommittee", call: "propose", arguments };
				let (_, events) = self.submit_events(&propose, member).await?;
				events
					.find_first::<runtime::technical_committee::events::Proposed>()?
					.ok_or_else(|| Error::NotFound("the motion proposed".into()))?
					.proposal_index
			},
		};
		let votes = self.storage().await?.fetch(&voting).await?;
		if votes.as_ref().is_some_and(|v| !v.ayes.contains(&member.account_id())) {
			self.submit(&runtime::tx().technical_committee().vote(H256(motion), index, true), member).await?;
		}
		let votes = self.storage().await?.fetch(&voting).await?;
		if votes.is_some_and(|v| v.ayes.len() as u32 >= v.threshold) {
			let weight = runtime::runtime_types::sp_weights::weight_v2::Weight { ref_time: u64::MAX / 4, proof_size: u64::MAX / 4 };
			self.submit(&runtime::tx().technical_committee().close(H256(motion), index, weight, length), member).await?;
		}
		let whitelisted = runtime::storage().whitelist().whitelisted_call(H256(call_hash));
		Ok(self.storage().await?.fetch(&whitelisted).await?.is_some())
	}
}
