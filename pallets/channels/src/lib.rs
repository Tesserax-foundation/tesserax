//! # Payment channels pallet
//!
//! Micropayments between a payer and a payee without a transaction per payment.
//!
//! The payer opens a channel to a payee with a deposit, held from its account, and names the
//! ML-DSA-65 key that signs vouchers: its own key or the key of an agent spending for it. Each
//! payment is then a voucher signed off chain, carrying the cumulative amount owed on the
//! channel. The payee checks vouchers off chain, serves, and claims the latest voucher whenever
//! it wants: the chain pays it the difference with what it already claimed.
//!
//! A voucher is the ML-DSA-65 signature, with the FIPS 204 context [`VOUCHER_CONTEXT`], of the
//! SCALE encoding of `(genesis_hash, channel_id, amount)`: it is valid on one chain and one
//! channel only, and an older voucher is worth less than a newer one.
//!
//! Closing:
//! - the payee closes the channel when it is done;
//! - the payer requests to close; the payee has `ClosePeriod` blocks to claim its latest
//!   voucher, after which anyone finalizes the close.
//!
//! On close, what was not claimed returns to the payer with the storage deposit.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;
pub use weights::WeightInfo;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

use codec::{Decode, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use tesserax_crypto::Public;

/// FIPS 204 context of voucher signatures: they can never be taken for transaction signatures.
pub const VOUCHER_CONTEXT: &[u8] = b"tesserax-channel";

/// Identifier of a channel.
pub type ChannelId = u64;

/// A payment channel.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct Channel<AccountId, Balance, BlockNumber> {
	pub payer: AccountId,
	pub payee: AccountId,
	/// Key that signs the vouchers.
	pub signer: Public,
	/// Total deposited by the payer.
	pub deposit: Balance,
	/// Cumulative amount paid to the payee.
	pub claimed: Balance,
	/// Block from which the close can be finalized, once the payer requested it.
	pub closes_at: Option<BlockNumber>,
	/// Storage deposit held from the payer.
	pub storage_deposit: Balance,
}

/// The message signed by a voucher.
pub fn voucher_message<Hash: Encode, Balance: Encode>(
	genesis_hash: &Hash,
	channel: ChannelId,
	amount: &Balance,
) -> alloc::vec::Vec<u8> {
	(genesis_hash, channel, amount).encode()
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::{
		pallet_prelude::*,
		traits::{
			fungible::{Inspect, Mutate, MutateHold},
			tokens::{Fortitude, Precision, Restriction},
		},
	};
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::{Saturating, Zero};
	use tesserax_crypto::Signature;

	pub type BalanceOf<T> =
		<<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	pub type ChannelOf<T> =
		Channel<<T as frame_system::Config>::AccountId, BalanceOf<T>, BlockNumberFor<T>>;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The overarching hold reason.
		type RuntimeHoldReason: From<HoldReason>;

		/// The native currency.
		type Currency: Mutate<Self::AccountId>
			+ MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;

		/// Blocks the payee has to claim after the payer requested to close.
		#[pallet::constant]
		type ClosePeriod: Get<BlockNumberFor<Self>>;

		/// Minimum deposit of a channel.
		#[pallet::constant]
		type MinDeposit: Get<BalanceOf<Self>>;

		/// Deposit for the storage of a channel.
		#[pallet::constant]
		type StorageDeposit: Get<BalanceOf<Self>>;

		/// Weights of the calls.
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Reasons for funds held by this pallet.
	#[pallet::composite_enum]
	pub enum HoldReason {
		/// Deposit of a payment channel and of its storage.
		Channel,
	}

	/// Identifier of the next channel.
	#[pallet::storage]
	pub type NextChannelId<T: Config> = StorageValue<_, ChannelId, ValueQuery>;

	/// Open channels.
	#[pallet::storage]
	pub type Channels<T: Config> = StorageMap<_, Blake2_128Concat, ChannelId, ChannelOf<T>, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A channel was opened.
		Opened { id: ChannelId, payer: T::AccountId, payee: T::AccountId, deposit: BalanceOf<T> },
		/// The payer added to the deposit.
		ToppedUp { id: ChannelId, deposit: BalanceOf<T> },
		/// The payee claimed a voucher of cumulative amount `claimed`, and received `paid`.
		Claimed { id: ChannelId, claimed: BalanceOf<T>, paid: BalanceOf<T> },
		/// The payer requested to close; the payee can claim until `closes_at`.
		CloseRequested { id: ChannelId, closes_at: BlockNumberFor<T> },
		/// The channel was closed and `refunded` returned to the payer.
		Closed { id: ChannelId, refunded: BalanceOf<T> },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// No channel has this identifier.
		UnknownChannel,
		/// Only the payer can do this.
		NotPayer,
		/// Only the payee can do this.
		NotPayee,
		/// A channel to oneself.
		SelfChannel,
		/// The deposit is below the minimum.
		DepositTooLow,
		/// The voucher signature is invalid.
		InvalidVoucher,
		/// The voucher is not above what was already claimed.
		NothingToClaim,
		/// The voucher exceeds the deposit.
		ExceedsDeposit,
		/// The channel is closing.
		Closing,
		/// The payer has not requested to close, or the close period is not over.
		NotClosable,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Open a channel to `payee` with `deposit`, whose vouchers are signed by `signer`.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::open())]
		pub fn open(
			origin: OriginFor<T>,
			payee: T::AccountId,
			signer: Public,
			deposit: BalanceOf<T>,
		) -> DispatchResult {
			let payer = ensure_signed(origin)?;
			ensure!(payer != payee, Error::<T>::SelfChannel);
			ensure!(deposit >= T::MinDeposit::get(), Error::<T>::DepositTooLow);
			let storage_deposit = T::StorageDeposit::get();
			T::Currency::hold(&HoldReason::Channel.into(), &payer, deposit.saturating_add(storage_deposit))?;

			let id = NextChannelId::<T>::mutate(|next| {
				let id = *next;
				*next = next.saturating_add(1);
				id
			});
			Channels::<T>::insert(
				id,
				Channel {
					payer: payer.clone(),
					payee: payee.clone(),
					signer,
					deposit,
					claimed: Zero::zero(),
					closes_at: None,
					storage_deposit,
				},
			);
			Self::deposit_event(Event::Opened { id, payer, payee, deposit });
			Ok(())
		}

		/// Add `amount` to the deposit of channel `id`, as its payer.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::top_up())]
		pub fn top_up(origin: OriginFor<T>, id: ChannelId, amount: BalanceOf<T>) -> DispatchResult {
			let payer = ensure_signed(origin)?;
			let deposit = Channels::<T>::try_mutate(id, |channel| -> Result<_, DispatchError> {
				let channel = channel.as_mut().ok_or(Error::<T>::UnknownChannel)?;
				ensure!(channel.payer == payer, Error::<T>::NotPayer);
				ensure!(channel.closes_at.is_none(), Error::<T>::Closing);
				T::Currency::hold(&HoldReason::Channel.into(), &payer, amount)?;
				channel.deposit.saturating_accrue(amount);
				Ok(channel.deposit)
			})?;
			Self::deposit_event(Event::ToppedUp { id, deposit });
			Ok(())
		}

		/// Claim the voucher of cumulative `amount` on channel `id`, as its payee.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::claim())]
		pub fn claim(
			origin: OriginFor<T>,
			id: ChannelId,
			amount: BalanceOf<T>,
			signature: Signature,
		) -> DispatchResult {
			let payee = ensure_signed(origin)?;
			let mut channel = Channels::<T>::get(id).ok_or(Error::<T>::UnknownChannel)?;
			ensure!(channel.payee == payee, Error::<T>::NotPayee);
			ensure!(amount > channel.claimed, Error::<T>::NothingToClaim);
			ensure!(amount <= channel.deposit, Error::<T>::ExceedsDeposit);
			ensure!(Self::verify_voucher(&channel, id, &amount, &signature), Error::<T>::InvalidVoucher);

			let paid = amount.saturating_sub(channel.claimed);
			T::Currency::transfer_on_hold(
				&HoldReason::Channel.into(),
				&channel.payer,
				&payee,
				paid,
				Precision::Exact,
				Restriction::Free,
				Fortitude::Polite,
			)?;
			channel.claimed = amount;
			Channels::<T>::insert(id, channel);
			Self::deposit_event(Event::Claimed { id, claimed: amount, paid });
			Ok(())
		}

		/// Close channel `id` as its payee: what was not claimed returns to the payer.
		#[pallet::call_index(3)]
		#[pallet::weight(T::WeightInfo::close())]
		pub fn close(origin: OriginFor<T>, id: ChannelId) -> DispatchResult {
			let payee = ensure_signed(origin)?;
			let channel = Channels::<T>::get(id).ok_or(Error::<T>::UnknownChannel)?;
			ensure!(channel.payee == payee, Error::<T>::NotPayee);
			Self::do_close(id, channel)
		}

		/// Request to close channel `id` as its payer. The payee can still claim during
		/// `ClosePeriod` blocks.
		#[pallet::call_index(4)]
		#[pallet::weight(T::WeightInfo::request_close())]
		pub fn request_close(origin: OriginFor<T>, id: ChannelId) -> DispatchResult {
			let payer = ensure_signed(origin)?;
			let closes_at = frame_system::Pallet::<T>::block_number().saturating_add(T::ClosePeriod::get());
			Channels::<T>::try_mutate(id, |channel| -> DispatchResult {
				let channel = channel.as_mut().ok_or(Error::<T>::UnknownChannel)?;
				ensure!(channel.payer == payer, Error::<T>::NotPayer);
				ensure!(channel.closes_at.is_none(), Error::<T>::Closing);
				channel.closes_at = Some(closes_at);
				Ok(())
			})?;
			Self::deposit_event(Event::CloseRequested { id, closes_at });
			Ok(())
		}

		/// Finalize the close of channel `id` once its close period is over. Anyone can call it.
		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::close())]
		pub fn finalize_close(origin: OriginFor<T>, id: ChannelId) -> DispatchResult {
			ensure_signed(origin)?;
			let channel = Channels::<T>::get(id).ok_or(Error::<T>::UnknownChannel)?;
			let now = frame_system::Pallet::<T>::block_number();
			ensure!(channel.closes_at.is_some_and(|at| now >= at), Error::<T>::NotClosable);
			Self::do_close(id, channel)
		}
	}

	impl<T: Config> Pallet<T> {
		/// Whether `signature` is a voucher of `amount` on channel `id`.
		pub fn verify_voucher(
			channel: &ChannelOf<T>,
			id: ChannelId,
			amount: &BalanceOf<T>,
			signature: &Signature,
		) -> bool {
			let genesis = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
			let message = voucher_message(&genesis, id, amount);
			tesserax_crypto::verify_with_context(
				signature.as_ref(),
				&message,
				VOUCHER_CONTEXT,
				channel.signer.as_ref(),
			)
		}

		fn do_close(id: ChannelId, channel: ChannelOf<T>) -> DispatchResult {
			let refunded = channel.deposit.saturating_sub(channel.claimed);
			T::Currency::release(
				&HoldReason::Channel.into(),
				&channel.payer,
				refunded.saturating_add(channel.storage_deposit),
				Precision::BestEffort,
			)?;
			Channels::<T>::remove(id);
			Self::deposit_event(Event::Closed { id, refunded });
			Ok(())
		}
	}
}
