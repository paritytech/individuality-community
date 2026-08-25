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

//! Filter behavior tests for external-asset teleport between this People parachain and
//! Asset Hub. Pins the result of `IsTeleporter::contains` and `IsReserve::contains`
//! across the relevant origin / asset combinations so the
//! reserve-transfer-to-teleport switch can't silently regress.

use crate::{
	people::{ExternalAssetLocation, EXTERNAL_ASSET_ID},
	xcm_config::{AssetHubLocation, RelayLocation, XcmConfig},
};
use frame_support::traits::ContainsPair;
use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
use xcm::latest::prelude::*;

type IsTeleporter = <XcmConfig as xcm_executor::Config>::IsTeleporter;
type IsReserve = <XcmConfig as xcm_executor::Config>::IsReserve;

fn external_asset(amount: u128) -> Asset {
	(ExternalAssetLocation::get(), amount).into()
}

/// Build the canonical AH-side asset Location for an arbitrary trust-backed
/// asset id from AH.
fn ah_asset(asset_id: u128, amount: u128) -> Asset {
	(
		Location::new(1, [Parachain(ASSET_HUB_ID), PalletInstance(50), GeneralIndex(asset_id)]),
		amount,
	)
		.into()
}

// --- IsTeleporter -----------------------------------------------------------

#[test]
fn external_asset_teleport_from_asset_hub_is_accepted() {
	assert!(IsTeleporter::contains(&external_asset(1_000), &AssetHubLocation::get()));
}

#[test]
fn external_asset_teleport_from_relay_is_rejected() {
	assert!(!IsTeleporter::contains(&external_asset(1_000), &RelayLocation::get()));
}

#[test]
fn external_asset_teleport_from_random_sibling_is_rejected() {
	let random_sibling = Location::new(1, [Parachain(4242)]);
	assert!(!IsTeleporter::contains(&external_asset(1_000), &random_sibling));
}

#[test]
fn pas_teleport_from_relay_still_works() {
	// Regression: relay native teleport rule unaffected.
	let pas: Asset = (RelayLocation::get(), 1_000u128).into();
	assert!(IsTeleporter::contains(&pas, &RelayLocation::get()));
}

// --- IsReserve --------------------------------------------------------------

#[test]
fn external_asset_reserve_from_asset_hub_is_rejected() {
	// External assets are teleport-only; this test pins that behavior down.
	assert!(!IsReserve::contains(&external_asset(1_000), &AssetHubLocation::get()));
}

#[test]
fn other_ah_asset_reserve_from_asset_hub_is_accepted() {
	// Regression: every other AH-issued trust-backed asset still flows as a
	// reserve transfer.
	let other = ah_asset(u128::from(EXTERNAL_ASSET_ID + 1), 1_000);
	assert!(IsReserve::contains(&other, &AssetHubLocation::get()));
}

/// Only assets Asset Hub itself issues may be reserve transferred here from Asset Hub.
///
/// Accepting an asset from a chain that is not its real reserve gives it two reserves, and
/// `ReserveAssetDeposited` mints locally, so the impostor reserve can credit this chain with
/// holdings nobody is backing. The cases below are the ones that would actually hurt.
#[test]
fn only_asset_hub_native_assets_are_reserve_accepted_from_asset_hub() {
	let asset_hub = AssetHubLocation::get();
	let accepted = |asset: Location| IsReserve::contains(&(asset, 1_000u128).into(), &asset_hub);

	// Asset Hub is the reserve for the assets it issues: trust backed assets and pool tokens.
	assert!(accepted(Location::new(
		1,
		[Parachain(ASSET_HUB_ID), PalletInstance(50), GeneralIndex(4242)]
	)));
	assert!(accepted(Location::new(
		1,
		[Parachain(ASSET_HUB_ID), PalletInstance(55), GeneralIndex(7)]
	)));

	// PAS must never arrive as a reserve asset: `FungibleTransactor` has no checking account, so
	// depositing it mints into `Balances`. PAS only ever arrives by teleport.
	assert!(!accepted(RelayLocation::get()));

	// Neither may Asset Hub vouch for assets it merely custodies.
	assert!(!accepted(Location::new(2, [GlobalConsensus(NetworkId::Ethereum { chain_id: 1 })])));
	assert!(!accepted(Location::new(2, [GlobalConsensus(NetworkId::Kusama)])));
	assert!(!accepted(Location::new(1, [Parachain(2000), GeneralIndex(1)])));

	// And an Asset Hub asset is only accepted *from* Asset Hub.
	let ah_asset =
		Location::new(1, [Parachain(ASSET_HUB_ID), PalletInstance(50), GeneralIndex(4242)]);
	assert!(!IsReserve::contains(&(ah_asset.clone(), 1_000u128).into(), &RelayLocation::get()));
	assert!(!IsReserve::contains(
		&(ah_asset, 1_000u128).into(),
		&Location::new(1, [Parachain(4242)])
	));
}

#[test]
fn other_ah_asset_reserve_from_relay_is_rejected() {
	let other = ah_asset(u128::from(EXTERNAL_ASSET_ID + 1), 1_000);
	assert!(!IsReserve::contains(&other, &RelayLocation::get()));
}
