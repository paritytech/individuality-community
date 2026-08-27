// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Forwards trust-backed assets to a sibling chain over XCM.
//!
//! Any signed account can replicate an existing trust-backed asset onto the destination chain.
//! The pallet reads the asset's minimum balance and sufficiency from local state, so a caller
//! cannot misrepresent them, and sends the destination a program that force-creates the replica
//! with this pallet's sovereign account as owner and team. The metadata is not carried over but it
//! can be queried on the original chain of the asset. The destination authenticates the program by
//! this pallet's location, so its `pallet-assets` instance must set an origin filter (for example
//! `EnsureXcm<Equals<location>>`) that matches `(1, [Parachain(<this chain>), PalletInstance(<this
//! pallet>)])` as `ForceOrigin`.
//!
//! The replica's owner and team resolve to an account nobody controls on the destination, so its
//! issuance there can only change through XCM transfers backed by this chain. The caller pays the
//! XCM delivery fees and a deposit which backs the [`ForwardedAssets`] entry; the deposit is held
//! forever, which is what makes junk forwards costly.
//!
//! Sufficiency and minimum balance can change after an asset was forwarded, so the permissionless
//! [`Pallet::sync_asset_status`] call re-sends the current values for an already forwarded asset.
//! The last-sent values are recorded and a sync that repeats them is rejected, so the destination
//! only executes when there is something to update.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
pub mod weights;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use pallet::*;
pub use weights::WeightInfo;

use alloc::vec;
use codec::{Encode, HasCompact};
use frame_support::{
	pallet_prelude::*,
	traits::fungible::{Inspect as FungibleInspect, Mutate as FungibleMutate, MutateHold},
};
use sp_runtime::{
	traits::{Convert, TryConvert},
	MultiAddress,
};
use xcm::latest::{
	validate_send, ExecuteXcm, InteriorLocation, Junction, Location, MaybeErrorCode, OriginKind,
	Reanchorable, SendXcm, WeightLimit, Xcm, XcmHash,
};
use xcm_executor::traits::{ConvertLocation, FeeManager, FeeReason};

/// Balance type of the currency backing the forward deposit.
pub type NativeBalanceOf<T, I> = <<T as Config<I>>::Currency as FungibleInspect<
	<T as frame_system::Config>::AccountId,
>>::Balance;

/// Record of a forwarded asset.
#[derive(Clone, Debug, Decode, Encode, Eq, MaxEncodedLen, PartialEq, TypeInfo)]
pub struct ForwardInfo<AccountId, Balance, AssetBalance> {
	/// The account that forwarded the asset and pays the deposit.
	pub depositor: AccountId,
	/// The deposit held from the depositor. Recorded here because the configured deposit can
	/// change later.
	pub deposit: Balance,
	/// The asset's minimum balance last sent to the destination.
	pub min_balance: AssetBalance,
	/// The asset's sufficiency last sent to the destination.
	pub is_sufficient: bool,
}

/// Mirror of the destination's `pallet-assets` calls this pallet sends.
///
/// The variant indices are the `pallet-assets` call indices, so the encoding matches the
/// destination without depending on its runtime. The booleans mirror the remote call arguments.
#[derive(Clone, Debug, Encode, Eq, PartialEq)]
pub enum RemoteAssetsCall<AccountId, Balance: HasCompact> {
	#[codec(index = 1)]
	ForceCreate {
		id: Location,
		owner: MultiAddress<AccountId, ()>,
		is_sufficient: bool,
		#[codec(compact)]
		min_balance: Balance,
	},
	#[codec(index = 21)]
	ForceAssetStatus {
		id: Location,
		owner: MultiAddress<AccountId, ()>,
		issuer: MultiAddress<AccountId, ()>,
		admin: MultiAddress<AccountId, ()>,
		freezer: MultiAddress<AccountId, ()>,
		#[codec(compact)]
		min_balance: Balance,
		is_sufficient: bool,
		is_frozen: bool,
	},
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T, I = ()>(_);

	#[pallet::config]
	pub trait Config<I: 'static = ()>: frame_system::Config + pallet_assets::Config<I> {
		/// The overarching hold reason type.
		type RuntimeHoldReason: From<HoldReason<I>>;

		/// Currency the forward deposit is held in.
		type Currency: FungibleMutate<Self::AccountId>
			+ MutateHold<Self::AccountId, Reason = Self::RuntimeHoldReason>;

		/// Deposit held from the caller for each forwarded asset. It backs the
		/// [`ForwardedAssets`] entry and is never released.
		type ForwardDeposit: Get<NativeBalanceOf<Self, I>>;

		/// Location of the destination chain, as seen from this chain.
		type Destination: Get<Location>;

		/// Index of the `pallet-assets` instance in the destination's runtime call enum. The
		/// destination decodes the remote calls under this index, so a mismatch makes every
		/// forward fail on the destination.
		type RemoteAssetsPalletIndex: Get<u8>;

		/// Location of the local `pallet-assets` instance the forwarded assets live in. Asset ids
		/// are appended to it as `GeneralIndex` junctions and reanchored to the destination.
		type AssetsPalletLocation: Get<Location>;

		/// Universal location of this chain, used to reanchor locations to the destination.
		type UniversalLocation: Get<InteriorLocation>;

		/// Converts this pallet's location, as seen from the destination, into the account that
		/// owns the forwarded assets there.
		type DestinationAccountOf: ConvertLocation<Self::AccountId>;

		/// Converts an asset id into the `GeneralIndex` value of its location.
		type AssetIdToIndex: Convert<Self::AssetId, u128>;

		/// Converts the caller's origin into the location charged for XCM delivery fees.
		type OriginToLocation: TryConvert<Self::RuntimeOrigin, Location>;

		/// Router that delivers XCM to the destination.
		type XcmSender: SendXcm;

		/// Executor used to charge XCM delivery fees to the caller.
		type XcmExecutor: ExecuteXcm<Self::RuntimeCall> + FeeManager;

		/// Weight information for this pallet.
		type WeightInfo: WeightInfo;

		/// Helper for setting up benchmark preconditions only the runtime knows how to create.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: crate::benchmarking::BenchmarkHelper;
	}

	/// Reasons for holding funds.
	#[pallet::composite_enum]
	pub enum HoldReason<I: 'static = ()> {
		/// Deposit backing a [`ForwardedAssets`] entry.
		#[codec(index = 0)]
		ForwardDeposit,
	}

	/// Assets already forwarded to the destination chain.
	#[pallet::storage]
	pub type ForwardedAssets<T: Config<I>, I: 'static = ()> = StorageMap<
		_,
		Blake2_128Concat,
		T::AssetId,
		ForwardInfo<T::AccountId, NativeBalanceOf<T, I>, T::Balance>,
		OptionQuery,
	>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config<I>, I: 'static = ()> {
		/// An asset was forwarded to the destination chain.
		AssetForwarded {
			asset_id: T::AssetId,
			remote_asset_id: Location,
			is_sufficient: bool,
			message_id: XcmHash,
		},
		/// The status of a forwarded asset was re-sent to the destination chain.
		AssetStatusSynced { asset_id: T::AssetId, is_sufficient: bool, message_id: XcmHash },
	}

	#[pallet::error]
	pub enum Error<T, I = ()> {
		/// The asset does not exist locally.
		UnknownAsset,
		/// The asset exists but is not live.
		AssetNotLive,
		/// The asset was already forwarded.
		AlreadyForwarded,
		/// The asset was not forwarded yet.
		NotForwarded,
		/// The asset's status equals what was last sent, so there is nothing to sync.
		StatusUnchanged,
		/// The asset id or the pallet location cannot be expressed from the destination's
		/// perspective.
		InvalidAssetLocation,
		/// A location cannot be converted into an account.
		LocationConversionFailed,
		/// The XCM delivery fees cannot be charged to the caller.
		FeesNotPaid,
		/// The message cannot be delivered to the destination.
		SendFailed,
	}

	#[pallet::call]
	impl<T: Config<I>, I: 'static> Pallet<T, I> {
		/// Forward the asset `id` to the destination chain.
		///
		/// The asset must exist and be live. Its minimum balance and sufficiency are read from
		/// local state and replicated on the destination. The caller pays the XCM delivery fees and
		/// [`Config::ForwardDeposit`], which is held indefinitely.
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config<I>>::WeightInfo::forward_asset())]
		pub fn forward_asset(origin: OriginFor<T>, id: T::AssetIdParameter) -> DispatchResult {
			let who = ensure_signed(origin.clone())?;
			let asset_id: T::AssetId = id.into();
			ensure!(
				!ForwardedAssets::<T, I>::contains_key(&asset_id),
				Error::<T, I>::AlreadyForwarded
			);

			let details =
				pallet_assets::Asset::<T, I>::get(&asset_id).ok_or(Error::<T, I>::UnknownAsset)?;
			ensure!(
				details.status == pallet_assets::AssetStatus::Live,
				Error::<T, I>::AssetNotLive
			);
			let remote_asset_id = Self::remote_asset_id(asset_id.clone())?;
			let owner = Self::remote_owner_account()?;

			let create = RemoteAssetsCall::ForceCreate {
				id: remote_asset_id.clone(),
				owner: MultiAddress::Id(owner),
				is_sufficient: details.is_sufficient,
				min_balance: details.min_balance,
			};

			let deposit = T::ForwardDeposit::get();
			<T as Config<I>>::Currency::hold(
				&HoldReason::<I>::ForwardDeposit.into(),
				&who,
				deposit,
			)?;

			let message = Self::build_remote_xcm(&create);
			let message_id = Self::send_remote_xcm(origin, message)?;

			ForwardedAssets::<T, I>::insert(
				&asset_id,
				ForwardInfo {
					depositor: who,
					deposit,
					min_balance: details.min_balance,
					is_sufficient: details.is_sufficient,
				},
			);
			Self::deposit_event(Event::AssetForwarded {
				asset_id,
				remote_asset_id,
				is_sufficient: details.is_sufficient,
				message_id,
			});
			Ok(())
		}

		/// Re-send the minimum balance and sufficiency of a forwarded asset after they changed.
		///
		/// Permissionless: the values are read from local state, so any caller sends the same
		/// truth. A call that repeats the last-sent values is rejected, which bounds the unpaid
		/// executions the destination performs to actual status changes. The caller pays the XCM
		/// delivery fees.
		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config<I>>::WeightInfo::sync_asset_status())]
		pub fn sync_asset_status(origin: OriginFor<T>, id: T::AssetIdParameter) -> DispatchResult {
			ensure_signed(origin.clone())?;
			let asset_id: T::AssetId = id.into();
			let mut record =
				ForwardedAssets::<T, I>::get(&asset_id).ok_or(Error::<T, I>::NotForwarded)?;

			let details =
				pallet_assets::Asset::<T, I>::get(&asset_id).ok_or(Error::<T, I>::UnknownAsset)?;
			ensure!(
				details.status == pallet_assets::AssetStatus::Live,
				Error::<T, I>::AssetNotLive
			);
			ensure!(
				record.min_balance != details.min_balance ||
					record.is_sufficient != details.is_sufficient,
				Error::<T, I>::StatusUnchanged
			);

			let remote_asset_id = Self::remote_asset_id(asset_id.clone())?;
			let owner = Self::remote_owner_account()?;

			let status = RemoteAssetsCall::ForceAssetStatus {
				id: remote_asset_id,
				owner: MultiAddress::Id(owner.clone()),
				issuer: MultiAddress::Id(owner.clone()),
				admin: MultiAddress::Id(owner.clone()),
				freezer: MultiAddress::Id(owner),
				min_balance: details.min_balance,
				is_sufficient: details.is_sufficient,
				is_frozen: false,
			};

			let message = Self::build_remote_xcm(&status);
			let message_id = Self::send_remote_xcm(origin, message)?;

			record.min_balance = details.min_balance;
			record.is_sufficient = details.is_sufficient;
			ForwardedAssets::<T, I>::insert(&asset_id, record);

			Self::deposit_event(Event::AssetStatusSynced {
				asset_id,
				is_sufficient: details.is_sufficient,
				message_id,
			});
			Ok(())
		}
	}

	impl<T: Config<I>, I: 'static> Pallet<T, I> {
		/// Returns this pallet's interior location, computed from its actual instance index so it
		/// cannot drift from the runtime's `construct_runtime!` ordering. The destination's origin
		/// filter must match this index.
		pub fn pallet_location() -> InteriorLocation {
			let index = <Self as frame_support::traits::PalletInfoAccess>::index() as u8;
			[Junction::PalletInstance(index)].into()
		}

		/// Returns the location of a local asset from the destination's perspective.
		pub fn remote_asset_id(asset_id: T::AssetId) -> Result<Location, Error<T, I>> {
			let index = T::AssetIdToIndex::convert(asset_id);
			T::AssetsPalletLocation::get()
				.appended_with(Junction::GeneralIndex(index))
				.map_err(|_| Error::<T, I>::InvalidAssetLocation)?
				.reanchored(&T::Destination::get(), &T::UniversalLocation::get())
				.map_err(|_| Error::<T, I>::InvalidAssetLocation)
		}

		/// Returns the account that owns the forwarded assets on the destination: the sovereign
		/// account of this pallet's location as seen from there.
		pub fn remote_owner_account() -> Result<T::AccountId, Error<T, I>> {
			let pallet_location: Location = Self::pallet_location().into();
			let reanchored = pallet_location
				.reanchored(&T::Destination::get(), &T::UniversalLocation::get())
				.map_err(|_| Error::<T, I>::InvalidAssetLocation)?;
			T::DestinationAccountOf::convert_location(&reanchored)
				.ok_or(Error::<T, I>::LocationConversionFailed)
		}

		/// Builds the program executed on the destination. Execution is unpaid because the
		/// destination trusts this chain; the origin is descended into this pallet's location so
		/// the destination can authenticate the `Transact`. The status check surfaces a failed
		/// dispatch as a failed program.
		fn build_remote_xcm(call: &RemoteAssetsCall<T::AccountId, T::Balance>) -> Xcm<()> {
			let encoded = (T::RemoteAssetsPalletIndex::get(), call).encode();
			Xcm(vec![
				xcm::latest::Instruction::UnpaidExecution {
					weight_limit: WeightLimit::Unlimited,
					check_origin: None,
				},
				xcm::latest::Instruction::DescendOrigin(Self::pallet_location()),
				xcm::latest::Instruction::Transact {
					origin_kind: OriginKind::Xcm,
					fallback_max_weight: None,
					call: encoded.into(),
				},
				xcm::latest::Instruction::ExpectTransactStatus(MaybeErrorCode::Success),
			])
		}

		/// Delivers `message` to the destination, charging the delivery fees to `origin` unless
		/// the fee manager waives them.
		fn send_remote_xcm(
			origin: T::RuntimeOrigin,
			message: Xcm<()>,
		) -> Result<XcmHash, DispatchError> {
			let fee_payer = T::OriginToLocation::try_convert(origin)
				.map_err(|_| Error::<T, I>::LocationConversionFailed)?;
			let (ticket, price) = validate_send::<T::XcmSender>(T::Destination::get(), message)
				.map_err(|_| Error::<T, I>::SendFailed)?;
			if !<T::XcmExecutor as FeeManager>::is_waived(Some(&fee_payer), FeeReason::ChargeFees) {
				T::XcmExecutor::charge_fees(fee_payer, price)
					.map_err(|_| Error::<T, I>::FeesNotPaid)?;
			}
			let message_id =
				T::XcmSender::deliver(ticket).map_err(|_| Error::<T, I>::SendFailed)?;
			Ok(message_id)
		}
	}
}
