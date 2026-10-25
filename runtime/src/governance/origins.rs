//! Origins of the referendum tracks other than root: what a referendum of each track may do.

pub use pallet_origins::*;

#[frame_support::pallet]
pub mod pallet_origins {
	use frame_support::pallet_prelude::*;

	#[pallet::config]
	pub trait Config: frame_system::Config {}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[derive(
		PartialEq, Eq, Clone, MaxEncodedLen, Encode, Decode, DecodeWithMemTracking, TypeInfo, Debug,
	)]
	#[pallet::origin]
	pub enum Origin {
		/// A referendum of the urgent track: it dispatches, as root, a call the technical
		/// committee whitelisted.
		WhitelistedCaller,
		/// A referendum of the cancellation track: it cancels or kills another referendum.
		ReferendumCanceller,
	}

	macro_rules! ensure_origin {
		($($name:ident),*) => { $(
			/// Ensures the origin of a referendum of its track.
			pub struct $name;
			impl<O: OriginTrait + From<Origin>> EnsureOrigin<O> for $name
			where
				for<'a> &'a O::PalletsOrigin: TryInto<&'a Origin>,
			{
				type Success = ();
				fn try_origin(o: O) -> Result<Self::Success, O> {
					if let Ok(Origin::$name) = o.caller().try_into() {
						return Ok(());
					}
					Err(o)
				}
				#[cfg(feature = "runtime-benchmarks")]
				fn try_successful_origin() -> Result<O, ()> {
					Ok(O::from(Origin::$name))
				}
			}
		)* };
	}

	ensure_origin!(WhitelistedCaller, ReferendumCanceller);
}
