//! The tracks of referenda: what each may do, its deposit, its periods and the approval and
//! support it needs.
//!
//! Support is measured against the TESS that may vote, those not vesting
//! ([`super::VotableIssuance`]).

use super::origins;
use crate::{Balance, BlockNumber, RuntimeOrigin, UNIT};
use alloc::borrow::Cow;
use pallet_referenda::{Curve, Track, TrackInfo};
use sp_runtime::Perbill;

/// Blocks of 18 seconds. The `fast-runtime` feature shortens every period, for development
/// chains only.
#[cfg(not(feature = "fast-runtime"))]
mod time {
	use crate::BlockNumber;
	pub const TEN_MINUTES: BlockNumber = 33;
	pub const HOUR: BlockNumber = 200;
	pub const DAY: BlockNumber = 4_800;
}
#[cfg(feature = "fast-runtime")]
mod time {
	use crate::BlockNumber;
	pub const TEN_MINUTES: BlockNumber = 1;
	pub const HOUR: BlockNumber = 2;
	pub const DAY: BlockNumber = 10;
}
pub use time::*;

/// Track of root: anything, a runtime upgrade included.
pub const ROOT: u16 = 0;
/// Track of urgent fixes: a call the technical committee whitelisted, dispatched as root.
pub const URGENT: u16 = 1;
/// Track of cancellations: cancel or kill another referendum.
pub const CANCELLATION: u16 = 2;

const fn name(text: &str) -> [u8; 25] {
	let bytes = text.as_bytes();
	let mut out = [0u8; 25];
	let mut i = 0;
	while i < bytes.len() {
		out[i] = bytes[i];
		i += 1;
	}
	out
}

/// Approval from 100% at the start of the decision period down to `floor` at its end.
const fn approval(floor: u32) -> Curve {
	Curve::LinearDecreasing {
		length: Perbill::from_percent(100),
		floor: Perbill::from_percent(floor),
		ceil: Perbill::from_percent(100),
	}
}

/// Support from `ceil` at the start of the decision period down to `floor` at its end, in
/// thousandths of the TESS that may vote.
const fn support(ceil: u32, floor: u32) -> Curve {
	Curve::LinearDecreasing {
		length: Perbill::from_percent(100),
		floor: Perbill::from_perthousand(floor),
		ceil: Perbill::from_perthousand(ceil),
	}
}

static TRACKS: [Track<u16, Balance, BlockNumber>; 3] = [
	Track {
		id: ROOT,
		info: TrackInfo {
			name: name("root"),
			max_deciding: 5,
			decision_deposit: 1_000 * UNIT,
			prepare_period: 2 * HOUR,
			decision_period: 14 * DAY,
			confirm_period: DAY,
			min_enactment_period: DAY,
			min_approval: approval(50),
			min_support: support(200, 10),
		},
	},
	Track {
		id: URGENT,
		info: TrackInfo {
			name: name("urgent"),
			max_deciding: 10,
			decision_deposit: 500 * UNIT,
			prepare_period: 3 * TEN_MINUTES,
			decision_period: 2 * DAY,
			confirm_period: TEN_MINUTES,
			min_enactment_period: TEN_MINUTES,
			min_approval: approval(50),
			min_support: support(50, 5),
		},
	},
	Track {
		id: CANCELLATION,
		info: TrackInfo {
			name: name("cancellation"),
			max_deciding: 10,
			decision_deposit: 100 * UNIT,
			prepare_period: HOUR,
			decision_period: DAY,
			confirm_period: 3 * HOUR,
			min_enactment_period: TEN_MINUTES,
			min_approval: approval(50),
			min_support: support(100, 5),
		},
	},
];

pub struct TracksInfo;

impl pallet_referenda::TracksInfo<Balance, BlockNumber> for TracksInfo {
	type Id = u16;
	type RuntimeOrigin = <RuntimeOrigin as frame_support::traits::OriginTrait>::PalletsOrigin;

	fn tracks() -> impl Iterator<Item = Cow<'static, Track<Self::Id, Balance, BlockNumber>>> {
		TRACKS.iter().map(Cow::Borrowed)
	}

	fn track_for(id: &Self::RuntimeOrigin) -> Result<Self::Id, ()> {
		if let Ok(system) = frame_system::RawOrigin::try_from(id.clone()) {
			match system {
				frame_system::RawOrigin::Root => Ok(ROOT),
				_ => Err(()),
			}
		} else if let Ok(custom) = origins::Origin::try_from(id.clone()) {
			match custom {
				origins::Origin::WhitelistedCaller => Ok(URGENT),
				origins::Origin::ReferendumCanceller => Ok(CANCELLATION),
			}
		} else {
			Err(())
		}
	}
}
