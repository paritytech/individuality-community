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

use next_people_paseo_runtime::{
	people::ExternalAssetLocation,
	xcm_config::{AssetHubLocation, RelayLocation},
	Runtime,
};
use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
use xcm::{latest::prelude::*, IntoVersion, VersionedAsset, VersionedLocation};
use xcm_runtime_apis::trusted_query::{runtime_decl_for_trusted_query_api::TrustedQueryApi, Error};

fn reserve_asset_location() -> Location {
	Location::new(1, [Parachain(ASSET_HUB_ID), PalletInstance(50), GeneralIndex(1)])
}

#[test]
fn trusted_reserve_query_uses_people_filters() {
	let cases = [
		(reserve_asset_location(), AssetHubLocation::get(), true),
		(reserve_asset_location(), RelayLocation::get(), false),
		(ExternalAssetLocation::get(), AssetHubLocation::get(), false),
		(RelayLocation::get(), AssetHubLocation::get(), false),
		(Location::here(), AssetHubLocation::get(), false),
	];

	for (asset_location, origin, expected) in cases {
		for version in [3, 4, 5] {
			let asset: Asset = (asset_location.clone(), 1u128).into();
			assert_eq!(
				Runtime::is_trusted_reserve(
					VersionedAsset::from(asset).into_version(version).unwrap(),
					VersionedLocation::from(origin.clone()).into_version(version).unwrap(),
				),
				Ok(expected),
				"asset {asset_location:?}, origin {origin:?}, XCM version {version}"
			);
		}
	}
}

#[test]
fn trusted_teleporter_query_uses_people_filters() {
	let cases = [
		(RelayLocation::get(), RelayLocation::get(), true),
		(RelayLocation::get(), AssetHubLocation::get(), true),
		(ExternalAssetLocation::get(), AssetHubLocation::get(), true),
		(ExternalAssetLocation::get(), RelayLocation::get(), false),
		(reserve_asset_location(), AssetHubLocation::get(), false),
		(RelayLocation::get(), Location::here(), false),
		(RelayLocation::get(), Location::new(1, [Parachain(2000)]), false),
	];

	for (asset_location, origin, expected) in cases {
		for version in [3, 4, 5] {
			let asset: Asset = (asset_location.clone(), 1u128).into();
			assert_eq!(
				Runtime::is_trusted_teleporter(
					VersionedAsset::from(asset).into_version(version).unwrap(),
					VersionedLocation::from(origin.clone()).into_version(version).unwrap(),
				),
				Ok(expected),
				"asset {asset_location:?}, origin {origin:?}, XCM version {version}"
			);
		}
	}
}

#[test]
fn trusted_queries_report_asset_conversion_errors() {
	let asset = VersionedAsset::V3(xcm::v3::MultiAsset {
		id: xcm::v3::AssetId::Abstract([0; 32]),
		fun: xcm::v3::Fungibility::Fungible(1),
	});
	let origin = VersionedLocation::from(AssetHubLocation::get());

	assert_eq!(
		Runtime::is_trusted_reserve(asset.clone(), origin.clone()),
		Err(Error::VersionedAssetConversionFailed)
	);
	assert_eq!(
		Runtime::is_trusted_teleporter(asset, origin),
		Err(Error::VersionedAssetConversionFailed)
	);
}
