// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0

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

//! Multi-asset bounty and child-bounty source types that derive account IDs using distinct
//! sub-account prefixes (`"mbt"` and `"mcb"`) so they do not collide with the legacy
//! single-asset bounties pallet (which uses `"bt"` and `"cb"`).

use frame_support::{traits::Get, PalletId};
use pallet_multi_asset_bounties::BountyIndex;
use sp_runtime::traits::{AccountIdConversion, Convert, TryConvert};

// TODO: remove this module and use the try_convert methods from
// multi-asset-bounties pallet directly in config.

/// Derives a **multi-asset** bounty account ID from the `PalletId` and the `BountyIndex`,
/// then converts it into the corresponding bounty `Beneficiary`.
///
/// Uses the prefix `"mbt"` (multi-asset bounty) so account IDs do not collide with the
/// legacy bounties pallet, which uses `"bt"`.
///
/// # Type Parameters
/// - `Id`: The pallet ID getter
/// - `T`: The pallet configuration
/// - `C`: Converter from `T::AccountId` to `T::Beneficiary`. Use `Identity` when types are the
///   same.
/// - `I`: Instance parameter (default: `()`)
pub struct MultiAssetBountySourceFromPalletId<Id, T, C, I = ()>(
	core::marker::PhantomData<(Id, T, C, I)>,
);

impl<Id, T, C, I> TryConvert<(BountyIndex, T::AssetKind), T::Beneficiary>
	for MultiAssetBountySourceFromPalletId<Id, T, C, I>
where
	Id: Get<PalletId>,
	T: pallet_multi_asset_bounties::Config<I>,
	C: Convert<T::AccountId, T::Beneficiary>,
{
	fn try_convert(
		(parent_bounty_id, _asset_kind): (BountyIndex, T::AssetKind),
	) -> Result<T::Beneficiary, (BountyIndex, T::AssetKind)> {
		let account: T::AccountId =
			Id::get().into_sub_account_truncating(("mbt", parent_bounty_id));
		Ok(C::convert(account))
	}
}

/// Derives a **multi-asset** child-bounty account ID from the `PalletId`, the parent index,
/// and the child index, then converts it into the child-bounty `Beneficiary`.
///
/// Uses the prefix `"mcb"` (multi-asset child bounty) so account IDs do not collide with the
/// legacy child-bounties pallet, which uses `"cb"`.
///
/// # Type Parameters
/// - `Id`: The pallet ID getter
/// - `T`: The pallet configuration
/// - `C`: Converter from `T::AccountId` to `T::Beneficiary`. Use `Identity` when types are the
///   same.
/// - `I`: Instance parameter (default: `()`)
pub struct MultiAssetChildBountySourceFromPalletId<Id, T, C, I = ()>(
	core::marker::PhantomData<(Id, T, C, I)>,
);

impl<Id, T, C, I> TryConvert<(BountyIndex, BountyIndex, T::AssetKind), T::Beneficiary>
	for MultiAssetChildBountySourceFromPalletId<Id, T, C, I>
where
	Id: Get<PalletId>,
	T: pallet_multi_asset_bounties::Config<I>,
	C: Convert<T::AccountId, T::Beneficiary>,
{
	fn try_convert(
		(parent_bounty_id, child_bounty_id, _asset_kind): (BountyIndex, BountyIndex, T::AssetKind),
	) -> Result<T::Beneficiary, (BountyIndex, BountyIndex, T::AssetKind)> {
		let account: T::AccountId =
			Id::get().into_sub_account_truncating(("mcb", parent_bounty_id, child_bounty_id));
		Ok(C::convert(account))
	}
}
