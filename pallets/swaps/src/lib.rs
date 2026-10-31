//! # Swaps pallet
//!
//! Atomic swaps of TESS against assets of other chains: BTC, and ETH, USDC and USDT on Ethereum,
//! Base and Arbitrum. Nobody holds the funds of another: each side is locked by a hash time-locked
//! contract, on its own chain, and released by the same secret.
//!
//! - A maker sells TESS with an offer: an amount held from its account, a price in the smallest
//!   unit of the asset for one TESS, the chain and asset it wants, and where it is paid.
//! - A taker picks a secret, and locks the payment on the other chain for the maker, under the
//!   SHA-256 hash of the secret, for 24 hours. The lock names the offer and the account of the
//!   taker on Tesserax.
//! - The maker checks the lock, then fills the offer with [`Pallet::fill`]: the TESS are locked
//!   for the taker under the same hash, for [`Config::TradeTimeout`] blocks.
//! - The taker claims the TESS with the secret, [`Pallet::claim`], without signer and without
//!   fee: the secret authorizes it. The secret is now public, and releases the payment to the
//!   maker on the other chain, for anyone who submits it there.
//! - A trade not claimed in time is refunded to the maker, [`Pallet::refund`], and the taker
//!   takes its payment back once its own lock expires.
//!
//! The chain cannot see the other chains: the maker checks the payment before filling. A maker
//! filling without a payment, or a wrong amount, only exposes its own TESS: the taker cannot claim
//! more than the trade, and the payment cannot be claimed without the secret of the taker.
//!
//! Bids go the other way, for those who sell TESS, miners first, without waiting for a buyer:
//!
//! - A buyer bids for TESS with [`Pallet::bid`]: a price in the smallest unit of the asset for one
//!   TESS, the chain and asset it pays in, and how many TESS it wants. Only a deposit is held.
//! - A seller picks a secret, and sells into the bid with [`Pallet::sell`]: its TESS are locked for
//!   the buyer under the SHA-256 hash of the secret, for [`Config::SaleTimeout`] blocks, with the
//!   address or key the seller is paid to.
//! - The buyer checks the sale, locks the payment on the other chain for the seller under the same
//!   hash, for less time than the sale has left, and records the lock with [`Pallet::pay`].
//! - The seller checks the payment and claims it with the secret, which is now public: the buyer,
//!   or anyone, claims the TESS for the buyer with [`Pallet::claim_sale`], without signer and
//!   without fee.
//! - A sale not claimed in time returns to the seller, [`Pallet::refund_sale`].
//!
//! A buyer who never pays only delays the seller: its TESS come back when the sale expires. The
//! seller reveals the secret only by claiming a payment it checked, and the payment expires before
//! the sale, which leaves the buyer the time to claim the TESS.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub use pallet::*;
pub use weights::WeightInfo;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{pallet_prelude::ConstU32, BoundedVec};
use scale_info::TypeInfo;

/// Identifier of an offer.
pub type OfferId = u64;

/// Identifier of a trade.
pub type TradeId = u64;

/// Identifier of a bid.
pub type BidId = u64;

/// Identifier of a sale into a bid.
pub type SaleId = u64;

/// Planck in one TESS: prices are for one TESS.
pub const PLANCK_PER_TESS: u128 = 1_000_000_000_000;

/// The chain a payment is made on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen)]
pub enum Chain {
	Bitcoin,
	Ethereum,
	Base,
	Arbitrum,
}

/// The asset a payment is made in, on its chain.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen)]
pub enum Asset {
	/// BTC on Bitcoin, ETH on the other chains.
	Native,
	Usdc,
	Usdt,
}

/// What an offer is paid in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen)]
pub struct Payment {
	pub chain: Chain,
	pub asset: Asset,
}

impl Payment {
	/// Whether the asset exists on the chain: BTC on Bitcoin only, USDT on Ethereum and Arbitrum.
	pub fn is_supported(&self) -> bool {
		match (self.chain, self.asset) {
			(Chain::Bitcoin, Asset::Native) => true,
			(Chain::Bitcoin, _) => false,
			(Chain::Base, Asset::Usdt) => false,
			_ => true,
		}
	}

	/// Length of the payout of the maker on this chain: the 20-byte address it is paid to on the
	/// EVM chains, the 33-byte compressed public key that signs its claim on Bitcoin.
	pub fn payout_len(&self) -> usize {
		match self.chain {
			Chain::Bitcoin => 33,
			_ => 20,
		}
	}
}

/// Where a maker is paid: an EVM address, or the public key of its claims on Bitcoin.
pub type Payout = BoundedVec<u8, ConstU32<64>>;

/// The lock of a payment on its chain: the identifier of the lock in the swap contract of an EVM
/// chain, 32 bytes, or the outpoint of the Bitcoin output, 36 bytes.
pub type PaymentLock = BoundedVec<u8, ConstU32<64>>;

/// The payment, in the smallest unit of its asset, of `amount` planck at `price` units per TESS,
/// rounded up.
pub fn payment_due(amount: u128, price: u128) -> Option<u128> {
	let product = amount.checked_mul(price)?;
	Some(product / PLANCK_PER_TESS + u128::from(product % PLANCK_PER_TESS != 0))
}

/// An offer of TESS.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct Offer<AccountId, Balance, BlockNumber> {
	pub maker: AccountId,
	pub payment: Payment,
	/// Smallest units of the asset for one TESS.
	pub price: u128,
	/// TESS still offered, held from the maker.
	pub available: Balance,
	/// Smallest trade, unless less remains.
	pub min_fill: Balance,
	pub payout: Payout,
	pub created: BlockNumber,
	/// Trades filled, and trades claimed by their taker: how reliable the maker is.
	pub fills: u32,
	pub completed: u32,
	/// Deposit for the storage of the offer, held from the maker.
	pub deposit: Balance,
}

/// TESS locked for a taker.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct Trade<AccountId, Balance, BlockNumber> {
	pub offer: OfferId,
	pub maker: AccountId,
	pub taker: AccountId,
	pub amount: Balance,
	pub price: u128,
	pub payment: Payment,
	/// SHA-256 of the secret.
	pub hash: [u8; 32],
	/// The lock of the payment on its chain.
	pub payment_lock: PaymentLock,
	/// Last block the taker may claim at; after it, the maker is refunded.
	pub expires: BlockNumber,
}

/// A bid for TESS, paid in the asset of another chain.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct Bid<AccountId, Balance, BlockNumber> {
	pub buyer: AccountId,
	pub payment: Payment,
	/// Smallest units of the asset for one TESS.
	pub price: u128,
	/// TESS still wanted.
	pub wanted: Balance,
	/// Smallest sale, unless less is wanted.
	pub min_sale: Balance,
	pub created: BlockNumber,
	/// Sales made into the bid, and sales the buyer paid and received: how reliable the buyer is.
	pub sales: u32,
	pub completed: u32,
	/// Deposit for the storage of the bid, held from the buyer.
	pub deposit: Balance,
}

/// TESS a seller locked for the buyer of a bid.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, TypeInfo, MaxEncodedLen)]
pub struct Sale<AccountId, Balance, BlockNumber> {
	pub bid: BidId,
	pub seller: AccountId,
	pub buyer: AccountId,
	pub amount: Balance,
	pub price: u128,
	pub payment: Payment,
	/// SHA-256 of the secret of the seller.
	pub hash: [u8; 32],
	/// Where the seller is paid: an EVM address, or its public key on Bitcoin.
	pub payout: Payout,
	/// The lock of the payment on its chain, empty until the buyer records it.
	pub payment_lock: PaymentLock,
	/// Last block the buyer may claim at; after it, the seller is refunded.
	pub expires: BlockNumber,
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
	use sp_runtime::traits::{Saturating, UniqueSaturatedInto, Zero};

	pub type BalanceOf<T> = <<T as Config>::Currency as Inspect<<T as frame_system::Config>::AccountId>>::Balance;

	pub type OfferOf<T> = Offer<<T as frame_system::Config>::AccountId, BalanceOf<T>, BlockNumberFor<T>>;

	pub type TradeOf<T> = Trade<<T as frame_system::Config>::AccountId, BalanceOf<T>, BlockNumberFor<T>>;

	pub type BidOf<T> = Bid<<T as frame_system::Config>::AccountId, BalanceOf<T>, BlockNumberFor<T>>;

	pub type SaleOf<T> = Sale<<T as frame_system::Config>::AccountId, BalanceOf<T>, BlockNumberFor<T>>;

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The overarching event type.
		#[allow(deprecated)]
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

		/// The overarching hold reason.
		type RuntimeHoldReason: From<HoldReason>;

		/// The native currency.
		type Currency: Mutate<Self::AccountId> + MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;

		/// Blocks a taker has to claim a trade. The lock of its payment must last longer, so that
		/// the maker can claim the payment with the secret the claim reveals.
		#[pallet::constant]
		type TradeTimeout: Get<BlockNumberFor<Self>>;

		/// Blocks a buyer has to claim a sale. The lock of its payment must expire sooner, so that
		/// the buyer can claim the TESS with the secret the claim of the payment reveals.
		#[pallet::constant]
		type SaleTimeout: Get<BlockNumberFor<Self>>;

		/// Smallest offer, and smallest bid.
		#[pallet::constant]
		type MinOffer: Get<BalanceOf<Self>>;

		/// Deposit for the storage of an offer or a bid, returned when it is cancelled.
		#[pallet::constant]
		type OfferDeposit: Get<BalanceOf<Self>>;

		/// Longevity of a claim in the transaction pool.
		#[pallet::constant]
		type ClaimLongevity: Get<u64>;

		/// Weights of the calls.
		type WeightInfo: WeightInfo;
	}

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	/// Reasons for funds held by this pallet.
	#[pallet::composite_enum]
	pub enum HoldReason {
		/// TESS of an offer, locked by a trade or a sale, and the deposit of an offer or a bid.
		Swap,
	}

	#[pallet::storage]
	pub type NextOfferId<T: Config> = StorageValue<_, OfferId, ValueQuery>;

	#[pallet::storage]
	pub type NextTradeId<T: Config> = StorageValue<_, TradeId, ValueQuery>;

	/// Open offers.
	#[pallet::storage]
	pub type Offers<T: Config> = StorageMap<_, Blake2_128Concat, OfferId, OfferOf<T>, OptionQuery>;

	/// Trades waiting for their claim or their refund.
	#[pallet::storage]
	pub type Trades<T: Config> = StorageMap<_, Blake2_128Concat, TradeId, TradeOf<T>, OptionQuery>;

	/// Payment locks already filled: a payment fills one trade.
	#[pallet::storage]
	pub type FilledLocks<T: Config> =
		StorageDoubleMap<_, Blake2_128Concat, Chain, Blake2_128Concat, PaymentLock, TradeId, OptionQuery>;

	#[pallet::storage]
	pub type NextBidId<T: Config> = StorageValue<_, BidId, ValueQuery>;

	#[pallet::storage]
	pub type NextSaleId<T: Config> = StorageValue<_, SaleId, ValueQuery>;

	/// Open bids.
	#[pallet::storage]
	pub type Bids<T: Config> = StorageMap<_, Blake2_128Concat, BidId, BidOf<T>, OptionQuery>;

	/// Sales waiting for their claim or their refund.
	#[pallet::storage]
	pub type Sales<T: Config> = StorageMap<_, Blake2_128Concat, SaleId, SaleOf<T>, OptionQuery>;

	/// Payment locks recorded for a sale: a payment pays one sale.
	#[pallet::storage]
	pub type PaidLocks<T: Config> =
		StorageDoubleMap<_, Blake2_128Concat, Chain, Blake2_128Concat, PaymentLock, SaleId, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A maker offered `amount` TESS at `price` units of `payment` for one TESS.
		Offered { offer: OfferId, maker: T::AccountId, payment: Payment, price: u128, amount: BalanceOf<T> },
		/// The maker changed its offer.
		OfferUpdated { offer: OfferId, price: u128, available: BalanceOf<T>, min_fill: BalanceOf<T> },
		/// The maker withdrew its offer, and got back what remained.
		OfferCancelled { offer: OfferId, returned: BalanceOf<T> },
		/// The maker locked `amount` TESS for `taker`, released by the secret of `hash` until
		/// `expires`, for the payment locked by `payment_lock`.
		Filled {
			trade: TradeId,
			offer: OfferId,
			maker: T::AccountId,
			taker: T::AccountId,
			amount: BalanceOf<T>,
			price: u128,
			payment: Payment,
			hash: [u8; 32],
			payment_lock: PaymentLock,
			expires: BlockNumberFor<T>,
		},
		/// The taker claimed the trade with `secret`, which now releases the payment.
		Claimed { trade: TradeId, taker: T::AccountId, amount: BalanceOf<T>, secret: [u8; 32] },
		/// The trade expired unclaimed and returned to the maker.
		Refunded { trade: TradeId, maker: T::AccountId, amount: BalanceOf<T> },
		/// A buyer bid for `amount` TESS at `price` units of `payment` for one TESS.
		BidPlaced { bid: BidId, buyer: T::AccountId, payment: Payment, price: u128, amount: BalanceOf<T> },
		/// The buyer changed its bid.
		BidUpdated { bid: BidId, price: u128, wanted: BalanceOf<T>, min_sale: BalanceOf<T> },
		/// The buyer withdrew its bid.
		BidCancelled { bid: BidId },
		/// `seller` locked `amount` TESS for the buyer of bid `bid`, released by the secret of `hash`
		/// until `expires`, to be paid to `payout`.
		Sold {
			sale: SaleId,
			bid: BidId,
			seller: T::AccountId,
			buyer: T::AccountId,
			amount: BalanceOf<T>,
			price: u128,
			payment: Payment,
			hash: [u8; 32],
			payout: Payout,
			expires: BlockNumberFor<T>,
		},
		/// The buyer locked the payment of the sale, by `payment_lock`.
		SalePaid { sale: SaleId, payment_lock: PaymentLock },
		/// The buyer received the sale with `secret`, which the seller revealed by claiming its
		/// payment.
		SaleClaimed { sale: SaleId, buyer: T::AccountId, amount: BalanceOf<T>, secret: [u8; 32] },
		/// The sale expired unclaimed and returned to the seller.
		SaleRefunded { sale: SaleId, seller: T::AccountId, amount: BalanceOf<T> },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// No offer has this identifier.
		UnknownOffer,
		/// No trade has this identifier.
		UnknownTrade,
		/// Only the maker of the offer can do this.
		NotMaker,
		/// This asset is not paid on this chain.
		UnsupportedPayment,
		/// The payout is not an address, or a public key on Bitcoin, of the right length.
		InvalidPayout,
		/// The price is zero, or the payment of a trade overflows.
		InvalidPrice,
		/// The offer is below the minimum, or the smallest fill above the offer.
		InvalidAmount,
		/// The trade is smaller than the smallest fill of the offer.
		BelowMinFill,
		/// The offer has less TESS than the trade.
		NotEnoughOffered,
		/// This payment lock already filled a trade.
		LockAlreadyFilled,
		/// The secret does not match the hash of the trade.
		WrongSecret,
		/// The trade can no longer be claimed.
		Expired,
		/// The trade can still be claimed.
		NotExpired,
		/// A trade to the maker itself, or a sale to the seller itself.
		SelfTrade,
		/// No bid has this identifier.
		UnknownBid,
		/// No sale has this identifier.
		UnknownSale,
		/// Only the buyer of the bid can do this.
		NotBuyer,
		/// The bid wants fewer TESS than the sale.
		NotEnoughWanted,
		/// The sale is smaller than the smallest sale of the bid.
		BelowMinSale,
		/// The payment of this sale is already recorded, or this lock pays another sale.
		AlreadyPaid,
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Offer `amount` TESS at `price` units of `payment` for one TESS, paid to `payout`, in
		/// trades of at least `min_fill`. The amount and a storage deposit are held.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::offer())]
		pub fn offer(
			origin: OriginFor<T>,
			payment: Payment,
			price: u128,
			amount: BalanceOf<T>,
			min_fill: BalanceOf<T>,
			payout: Payout,
		) -> DispatchResult {
			let maker = ensure_signed(origin)?;
			ensure!(payment.is_supported(), Error::<T>::UnsupportedPayment);
			ensure!(payout.len() == payment.payout_len(), Error::<T>::InvalidPayout);
			Self::check_price(amount, price)?;
			ensure!(amount >= T::MinOffer::get() && min_fill <= amount, Error::<T>::InvalidAmount);
			let deposit = T::OfferDeposit::get();
			T::Currency::hold(&HoldReason::Swap.into(), &maker, amount.saturating_add(deposit))?;

			let id = NextOfferId::<T>::mutate(|next| {
				let id = *next;
				*next = next.saturating_add(1);
				id
			});
			Offers::<T>::insert(
				id,
				Offer {
					maker: maker.clone(),
					payment,
					price,
					available: amount,
					min_fill,
					payout,
					created: frame_system::Pallet::<T>::block_number(),
					fills: 0,
					completed: 0,
					deposit,
				},
			);
			Self::deposit_event(Event::Offered { offer: id, maker, payment, price, amount });
			Ok(())
		}

		/// Change the price and the smallest fill of offer `id`, and add `add` TESS to it or
		/// withdraw `remove` TESS from it, as its maker.
		#[pallet::call_index(1)]
		#[pallet::weight(T::WeightInfo::update_offer())]
		pub fn update_offer(
			origin: OriginFor<T>,
			id: OfferId,
			price: u128,
			min_fill: BalanceOf<T>,
			add: BalanceOf<T>,
			remove: BalanceOf<T>,
		) -> DispatchResult {
			let maker = ensure_signed(origin)?;
			let offer = Offers::<T>::try_mutate(id, |offer| -> Result<OfferOf<T>, DispatchError> {
				let offer = offer.as_mut().ok_or(Error::<T>::UnknownOffer)?;
				ensure!(offer.maker == maker, Error::<T>::NotMaker);
				ensure!(remove <= offer.available.saturating_add(add), Error::<T>::NotEnoughOffered);
				let available = offer.available.saturating_add(add).saturating_sub(remove);
				Self::check_price(available, price)?;
				ensure!(min_fill <= available || available.is_zero(), Error::<T>::InvalidAmount);
				if !add.is_zero() {
					T::Currency::hold(&HoldReason::Swap.into(), &maker, add)?;
				}
				if !remove.is_zero() {
					T::Currency::release(&HoldReason::Swap.into(), &maker, remove, Precision::Exact)?;
				}
				offer.available = available;
				offer.price = price;
				offer.min_fill = min_fill;
				Ok(offer.clone())
			})?;
			Self::deposit_event(Event::OfferUpdated {
				offer: id,
				price: offer.price,
				available: offer.available,
				min_fill: offer.min_fill,
			});
			Ok(())
		}

		/// Withdraw offer `id`, as its maker: what remains and the deposit are released. Trades
		/// already filled go on.
		#[pallet::call_index(2)]
		#[pallet::weight(T::WeightInfo::cancel_offer())]
		pub fn cancel_offer(origin: OriginFor<T>, id: OfferId) -> DispatchResult {
			let maker = ensure_signed(origin)?;
			let offer = Offers::<T>::get(id).ok_or(Error::<T>::UnknownOffer)?;
			ensure!(offer.maker == maker, Error::<T>::NotMaker);
			T::Currency::release(
				&HoldReason::Swap.into(),
				&maker,
				offer.available.saturating_add(offer.deposit),
				Precision::BestEffort,
			)?;
			Offers::<T>::remove(id);
			Self::deposit_event(Event::OfferCancelled { offer: id, returned: offer.available });
			Ok(())
		}

		/// Lock `amount` TESS of offer `id` for `taker`, released by the secret of `hash`, as the
		/// maker, once it checked the payment locked by `payment_lock` on the chain of the offer.
		#[pallet::call_index(3)]
		#[pallet::weight(T::WeightInfo::fill())]
		pub fn fill(
			origin: OriginFor<T>,
			id: OfferId,
			taker: T::AccountId,
			amount: BalanceOf<T>,
			hash: [u8; 32],
			payment_lock: PaymentLock,
		) -> DispatchResult {
			let maker = ensure_signed(origin)?;
			ensure!(maker != taker, Error::<T>::SelfTrade);
			let offer = Offers::<T>::try_mutate(id, |offer| -> Result<OfferOf<T>, DispatchError> {
				let offer = offer.as_mut().ok_or(Error::<T>::UnknownOffer)?;
				ensure!(offer.maker == maker, Error::<T>::NotMaker);
				ensure!(amount <= offer.available, Error::<T>::NotEnoughOffered);
				ensure!(amount >= offer.min_fill || amount == offer.available, Error::<T>::BelowMinFill);
				ensure!(!amount.is_zero(), Error::<T>::InvalidAmount);
				offer.available.saturating_reduce(amount);
				offer.fills.saturating_inc();
				Ok(offer.clone())
			})?;
			ensure!(!FilledLocks::<T>::contains_key(offer.payment.chain, &payment_lock), Error::<T>::LockAlreadyFilled);

			let trade = NextTradeId::<T>::mutate(|next| {
				let id = *next;
				*next = next.saturating_add(1);
				id
			});
			let expires = frame_system::Pallet::<T>::block_number().saturating_add(T::TradeTimeout::get());
			FilledLocks::<T>::insert(offer.payment.chain, &payment_lock, trade);
			Trades::<T>::insert(
				trade,
				Trade {
					offer: id,
					maker: maker.clone(),
					taker: taker.clone(),
					amount,
					price: offer.price,
					payment: offer.payment,
					hash,
					payment_lock: payment_lock.clone(),
					expires,
				},
			);
			Self::deposit_event(Event::Filled {
				trade,
				offer: id,
				maker,
				taker,
				amount,
				price: offer.price,
				payment: offer.payment,
				hash,
				payment_lock,
				expires,
			});
			Ok(())
		}

		/// Claim trade `id` for its taker with `secret`. Submitted without signer and without fee:
		/// the secret authorizes it, so that a taker without TESS yet can claim.
		#[pallet::call_index(4)]
		#[pallet::weight(T::WeightInfo::claim())]
		#[pallet::authorize(Pallet::<T>::authorize_claim)]
		#[pallet::weight_of_authorize(T::WeightInfo::authorize_claim())]
		pub fn claim(origin: OriginFor<T>, id: TradeId, secret: [u8; 32]) -> DispatchResult {
			ensure_authorized(origin)?;
			let trade = Self::claimable(id, &secret)?;
			T::Currency::transfer_on_hold(
				&HoldReason::Swap.into(),
				&trade.maker,
				&trade.taker,
				trade.amount,
				Precision::Exact,
				Restriction::Free,
				Fortitude::Polite,
			)?;
			Trades::<T>::remove(id);
			Offers::<T>::mutate(trade.offer, |offer| {
				if let Some(offer) = offer {
					offer.completed.saturating_inc();
				}
			});
			Self::deposit_event(Event::Claimed { trade: id, taker: trade.taker, amount: trade.amount, secret });
			Ok(())
		}

		/// Return trade `id` to its maker once it expired unclaimed: to its offer if it is still
		/// open, else to its account. Anyone can call it.
		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::refund())]
		pub fn refund(origin: OriginFor<T>, id: TradeId) -> DispatchResult {
			ensure_signed(origin)?;
			let trade = Trades::<T>::get(id).ok_or(Error::<T>::UnknownTrade)?;
			ensure!(frame_system::Pallet::<T>::block_number() > trade.expires, Error::<T>::NotExpired);
			let back_to_offer = Offers::<T>::mutate(trade.offer, |offer| match offer {
				Some(offer) => {
					offer.available.saturating_accrue(trade.amount);
					true
				},
				None => false,
			});
			if !back_to_offer {
				T::Currency::release(&HoldReason::Swap.into(), &trade.maker, trade.amount, Precision::BestEffort)?;
			}
			Trades::<T>::remove(id);
			Self::deposit_event(Event::Refunded { trade: id, maker: trade.maker, amount: trade.amount });
			Ok(())
		}

		/// Bid for `amount` TESS at `price` units of `payment` for one TESS, in sales of at least
		/// `min_sale`. Only a storage deposit is held: the payment is locked sale by sale.
		#[pallet::call_index(6)]
		#[pallet::weight(T::WeightInfo::bid())]
		pub fn bid(
			origin: OriginFor<T>,
			payment: Payment,
			price: u128,
			amount: BalanceOf<T>,
			min_sale: BalanceOf<T>,
		) -> DispatchResult {
			let buyer = ensure_signed(origin)?;
			ensure!(payment.is_supported(), Error::<T>::UnsupportedPayment);
			Self::check_price(amount, price)?;
			ensure!(amount >= T::MinOffer::get() && min_sale <= amount, Error::<T>::InvalidAmount);
			let deposit = T::OfferDeposit::get();
			T::Currency::hold(&HoldReason::Swap.into(), &buyer, deposit)?;

			let id = NextBidId::<T>::mutate(|next| {
				let id = *next;
				*next = next.saturating_add(1);
				id
			});
			Bids::<T>::insert(
				id,
				Bid {
					buyer: buyer.clone(),
					payment,
					price,
					wanted: amount,
					min_sale,
					created: frame_system::Pallet::<T>::block_number(),
					sales: 0,
					completed: 0,
					deposit,
				},
			);
			Self::deposit_event(Event::BidPlaced { bid: id, buyer, payment, price, amount });
			Ok(())
		}

		/// Change the price of bid `id`, the TESS it still wants and its smallest sale, as its
		/// buyer. Sales already made go on.
		#[pallet::call_index(7)]
		#[pallet::weight(T::WeightInfo::update_bid())]
		pub fn update_bid(
			origin: OriginFor<T>,
			id: BidId,
			price: u128,
			wanted: BalanceOf<T>,
			min_sale: BalanceOf<T>,
		) -> DispatchResult {
			let buyer = ensure_signed(origin)?;
			Self::check_price(wanted, price)?;
			ensure!(min_sale <= wanted || wanted.is_zero(), Error::<T>::InvalidAmount);
			Bids::<T>::try_mutate(id, |bid| -> DispatchResult {
				let bid = bid.as_mut().ok_or(Error::<T>::UnknownBid)?;
				ensure!(bid.buyer == buyer, Error::<T>::NotBuyer);
				bid.price = price;
				bid.wanted = wanted;
				bid.min_sale = min_sale;
				Ok(())
			})?;
			Self::deposit_event(Event::BidUpdated { bid: id, price, wanted, min_sale });
			Ok(())
		}

		/// Withdraw bid `id`, as its buyer: the deposit is released. Sales already made go on.
		#[pallet::call_index(8)]
		#[pallet::weight(T::WeightInfo::cancel_bid())]
		pub fn cancel_bid(origin: OriginFor<T>, id: BidId) -> DispatchResult {
			let buyer = ensure_signed(origin)?;
			let bid = Bids::<T>::get(id).ok_or(Error::<T>::UnknownBid)?;
			ensure!(bid.buyer == buyer, Error::<T>::NotBuyer);
			T::Currency::release(&HoldReason::Swap.into(), &buyer, bid.deposit, Precision::BestEffort)?;
			Bids::<T>::remove(id);
			Self::deposit_event(Event::BidCancelled { bid: id });
			Ok(())
		}

		/// Sell `amount` TESS into bid `id`: they are locked for its buyer, released by the secret
		/// of `hash`, and the buyer pays `payout`, an EVM address or a public key on Bitcoin.
		#[pallet::call_index(9)]
		#[pallet::weight(T::WeightInfo::sell())]
		pub fn sell(
			origin: OriginFor<T>,
			id: BidId,
			amount: BalanceOf<T>,
			hash: [u8; 32],
			payout: Payout,
		) -> DispatchResult {
			let seller = ensure_signed(origin)?;
			let bid = Bids::<T>::try_mutate(id, |bid| -> Result<BidOf<T>, DispatchError> {
				let bid = bid.as_mut().ok_or(Error::<T>::UnknownBid)?;
				ensure!(bid.buyer != seller, Error::<T>::SelfTrade);
				ensure!(payout.len() == bid.payment.payout_len(), Error::<T>::InvalidPayout);
				ensure!(!amount.is_zero(), Error::<T>::InvalidAmount);
				ensure!(amount <= bid.wanted, Error::<T>::NotEnoughWanted);
				ensure!(amount >= bid.min_sale || amount == bid.wanted, Error::<T>::BelowMinSale);
				bid.wanted.saturating_reduce(amount);
				bid.sales.saturating_inc();
				Ok(bid.clone())
			})?;
			T::Currency::hold(&HoldReason::Swap.into(), &seller, amount)?;

			let sale = NextSaleId::<T>::mutate(|next| {
				let id = *next;
				*next = next.saturating_add(1);
				id
			});
			let expires = frame_system::Pallet::<T>::block_number().saturating_add(T::SaleTimeout::get());
			Sales::<T>::insert(
				sale,
				Sale {
					bid: id,
					seller: seller.clone(),
					buyer: bid.buyer.clone(),
					amount,
					price: bid.price,
					payment: bid.payment,
					hash,
					payout: payout.clone(),
					payment_lock: Default::default(),
					expires,
				},
			);
			Self::deposit_event(Event::Sold {
				sale,
				bid: id,
				seller,
				buyer: bid.buyer,
				amount,
				price: bid.price,
				payment: bid.payment,
				hash,
				payout,
				expires,
			});
			Ok(())
		}

		/// Record `payment_lock`, the lock of the payment of sale `id` on its chain, as its buyer, so
		/// that the seller finds it.
		#[pallet::call_index(10)]
		#[pallet::weight(T::WeightInfo::pay())]
		pub fn pay(origin: OriginFor<T>, id: SaleId, payment_lock: PaymentLock) -> DispatchResult {
			let buyer = ensure_signed(origin)?;
			ensure!(!payment_lock.is_empty(), Error::<T>::InvalidAmount);
			let sale = Sales::<T>::try_mutate(id, |sale| -> Result<SaleOf<T>, DispatchError> {
				let sale = sale.as_mut().ok_or(Error::<T>::UnknownSale)?;
				ensure!(sale.buyer == buyer, Error::<T>::NotBuyer);
				ensure!(sale.payment_lock.is_empty(), Error::<T>::AlreadyPaid);
				ensure!(frame_system::Pallet::<T>::block_number() <= sale.expires, Error::<T>::Expired);
				sale.payment_lock = payment_lock.clone();
				Ok(sale.clone())
			})?;
			ensure!(!PaidLocks::<T>::contains_key(sale.payment.chain, &payment_lock), Error::<T>::AlreadyPaid);
			PaidLocks::<T>::insert(sale.payment.chain, &payment_lock, id);
			Self::deposit_event(Event::SalePaid { sale: id, payment_lock });
			Ok(())
		}

		/// Claim sale `id` for its buyer with `secret`. Submitted without signer and without fee:
		/// the secret authorizes it, so that a buyer without TESS yet can claim.
		#[pallet::call_index(11)]
		#[pallet::weight(T::WeightInfo::claim_sale())]
		#[pallet::authorize(Pallet::<T>::authorize_claim_sale)]
		#[pallet::weight_of_authorize(T::WeightInfo::authorize_claim_sale())]
		pub fn claim_sale(origin: OriginFor<T>, id: SaleId, secret: [u8; 32]) -> DispatchResult {
			ensure_authorized(origin)?;
			let sale = Self::claimable_sale(id, &secret)?;
			T::Currency::transfer_on_hold(
				&HoldReason::Swap.into(),
				&sale.seller,
				&sale.buyer,
				sale.amount,
				Precision::Exact,
				Restriction::Free,
				Fortitude::Polite,
			)?;
			Sales::<T>::remove(id);
			Bids::<T>::mutate(sale.bid, |bid| {
				if let Some(bid) = bid {
					bid.completed.saturating_inc();
				}
			});
			Self::deposit_event(Event::SaleClaimed { sale: id, buyer: sale.buyer, amount: sale.amount, secret });
			Ok(())
		}

		/// Return sale `id` to its seller once it expired unclaimed, and what it took to its bid if
		/// the bid is still open. Anyone can call it.
		#[pallet::call_index(12)]
		#[pallet::weight(T::WeightInfo::refund_sale())]
		pub fn refund_sale(origin: OriginFor<T>, id: SaleId) -> DispatchResult {
			ensure_signed(origin)?;
			let sale = Sales::<T>::get(id).ok_or(Error::<T>::UnknownSale)?;
			ensure!(frame_system::Pallet::<T>::block_number() > sale.expires, Error::<T>::NotExpired);
			T::Currency::release(&HoldReason::Swap.into(), &sale.seller, sale.amount, Precision::BestEffort)?;
			Bids::<T>::mutate(sale.bid, |bid| {
				if let Some(bid) = bid {
					bid.wanted.saturating_accrue(sale.amount);
				}
			});
			Sales::<T>::remove(id);
			Self::deposit_event(Event::SaleRefunded { sale: id, seller: sale.seller, amount: sale.amount });
			Ok(())
		}
	}

	impl<T: Config> Pallet<T> {
		/// The price must be positive, and the payment of `amount` must not overflow.
		fn check_price(amount: BalanceOf<T>, price: u128) -> DispatchResult {
			let amount: u128 = amount.unique_saturated_into();
			ensure!(price > 0 && payment_due(amount, price).is_some(), Error::<T>::InvalidPrice);
			Ok(())
		}

		/// Trade `id`, if `secret` claims it now.
		pub fn claimable(id: TradeId, secret: &[u8; 32]) -> Result<TradeOf<T>, Error<T>> {
			let trade = Trades::<T>::get(id).ok_or(Error::<T>::UnknownTrade)?;
			ensure!(sp_io::hashing::sha2_256(secret) == trade.hash, Error::<T>::WrongSecret);
			ensure!(frame_system::Pallet::<T>::block_number() <= trade.expires, Error::<T>::Expired);
			Ok(trade)
		}

		/// Sale `id`, if `secret` claims it now.
		pub fn claimable_sale(id: SaleId, secret: &[u8; 32]) -> Result<SaleOf<T>, Error<T>> {
			let sale = Sales::<T>::get(id).ok_or(Error::<T>::UnknownSale)?;
			ensure!(sp_io::hashing::sha2_256(secret) == sale.hash, Error::<T>::WrongSecret);
			ensure!(frame_system::Pallet::<T>::block_number() <= sale.expires, Error::<T>::Expired);
			Ok(sale)
		}

		/// Authorization of [`Pallet::claim_sale`]: the secret of an open sale.
		pub fn authorize_claim_sale(
			_source: TransactionSource,
			id: &SaleId,
			secret: &[u8; 32],
		) -> TransactionValidityWithRefund {
			Self::claimable_sale(*id, secret).map_err(|error| match error {
				Error::<T>::UnknownSale => InvalidTransaction::Stale,
				Error::<T>::Expired => InvalidTransaction::Stale,
				_ => InvalidTransaction::BadProof,
			})?;
			let validity = ValidTransaction::with_tag_prefix("Swaps")
				.and_provides(("claim_sale", *id))
				.longevity(T::ClaimLongevity::get())
				.propagate(true)
				.build()?;
			Ok((validity, Weight::zero()))
		}

		/// Authorization of [`Pallet::claim`]: the secret of an open trade.
		pub fn authorize_claim(
			_source: TransactionSource,
			id: &TradeId,
			secret: &[u8; 32],
		) -> TransactionValidityWithRefund {
			Self::claimable(*id, secret).map_err(|error| match error {
				Error::<T>::UnknownTrade => InvalidTransaction::Stale,
				Error::<T>::Expired => InvalidTransaction::Stale,
				_ => InvalidTransaction::BadProof,
			})?;
			let validity = ValidTransaction::with_tag_prefix("Swaps")
				.and_provides(("claim", *id))
				.longevity(T::ClaimLongevity::get())
				.propagate(true)
				.build()?;
			Ok((validity, Weight::zero()))
		}
	}
}
