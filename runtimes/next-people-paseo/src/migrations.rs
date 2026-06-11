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

use alloc::vec::Vec;
use assets_common::{
	local_and_foreign_assets::ForeignAssetReserveData,
	migrations::foreign_assets_reserves::ForeignAssetsReservesProvider,
};
use xcm::v5::{Junction::Parachain, Location};

fn reserve_data_for(asset_id: &Location) -> Option<ForeignAssetReserveData> {
	let (parents, interior) = asset_id.unpack();
	if parents != 1 {
		return None;
	}
	let reserve = match interior.first() {
		Some(Parachain(id)) => Location::new(1, [Parachain(*id)]),
		_ => return None,
	};
	Some((reserve, false).into())
}

pub struct PeoplePaseoAssetsReservesProvider;
impl ForeignAssetsReservesProvider for PeoplePaseoAssetsReservesProvider {
	type ReserveData = ForeignAssetReserveData;

	fn reserves_for(asset_id: &Location) -> Vec<Self::ReserveData> {
		reserve_data_for(asset_id).into_iter().collect()
	}

	#[cfg(feature = "try-runtime")]
	fn check_reserves_for(asset_id: &Location, reserves: Vec<Self::ReserveData>) -> bool {
		reserves == Self::reserves_for(asset_id)
	}
}
