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

//! Tests for asset replicas force-created by the assets forwarder on Asset Hub.
//!
//! Executes the program the forwarder sends through this runtime's XCM executor and pins the
//! origin gate: only the forwarder's location, descended from Asset Hub, may force-create or
//! force-update assets here. The program shape must stay in sync with
//! `indiv-pallet-assets-forwarder`, whose unit tests pin the same shape on the sending side.

use super::new_test_ext;
use crate::{
	xcm_config::{AssetHubLocation, XcmConfig},
	AccountId, Assets, Runtime, RuntimeCall,
};
use codec::Encode;
use frame_support::traits::PalletInfoAccess;
use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
use sp_runtime::MultiAddress;
use xcm::latest::prelude::*;
use xcm_builder::{DescribeAllTerminal, DescribeFamily, HashedDescription};
use xcm_executor::{
	traits::{ConvertLocation, Properties, ShouldExecute},
	XcmExecutor,
};

/// The forwarder's pallet index in next-asset-hub-paseo `construct_runtime!`, which
/// `AssetsForwarderLocation` in this runtime hardcodes.
const FORWARDER_PALLET_INDEX: u8 = 37;
/// The trust-backed assets pallet index on Asset Hub.
const AH_ASSETS_PALLET_INDEX: u8 = 50;
const ASSET_ID: u128 = 1234;
const MIN_BALANCE: u128 = 5;

fn remote_asset_id() -> Location {
	Location::new(
		1,
		[Parachain(ASSET_HUB_ID), PalletInstance(AH_ASSETS_PALLET_INDEX), GeneralIndex(ASSET_ID)],
	)
}

fn forwarder_location() -> Location {
	Location::new(1, [Parachain(ASSET_HUB_ID), PalletInstance(FORWARDER_PALLET_INDEX)])
}

/// The replica owner the forwarder embeds: the sovereign account of its own location, derived
/// with the standard family description.
fn owner_account() -> AccountId {
	HashedDescription::<AccountId, DescribeFamily<DescribeAllTerminal>>::convert_location(
		&forwarder_location(),
	)
	.expect("forwarder location converts to an account")
}

fn assets_pallet_index() -> u8 {
	<Assets as PalletInfoAccess>::index() as u8
}

fn force_create_call(is_sufficient: bool) -> Vec<u8> {
	let call = pallet_assets::Call::<Runtime>::force_create {
		id: remote_asset_id(),
		owner: MultiAddress::Id(owner_account()),
		is_sufficient,
		min_balance: MIN_BALANCE,
	};
	(assets_pallet_index(), call).encode()
}

fn force_asset_status_call(is_sufficient: bool, min_balance: u128) -> Vec<u8> {
	let owner = MultiAddress::Id(owner_account());
	let call = pallet_assets::Call::<Runtime>::force_asset_status {
		id: remote_asset_id(),
		owner: owner.clone(),
		issuer: owner.clone(),
		admin: owner.clone(),
		freezer: owner,
		min_balance,
		is_sufficient,
		is_frozen: false,
	};
	(assets_pallet_index(), call).encode()
}

/// The program the forwarder sends, parameterised over what the negative tests vary.
fn forward_program(origin_kind: OriginKind, descend_to: u8) -> Xcm<RuntimeCall> {
	Xcm(alloc::vec![
		UnpaidExecution { weight_limit: Unlimited, check_origin: None },
		DescendOrigin([PalletInstance(descend_to)].into()),
		Transact { origin_kind, fallback_max_weight: None, call: force_create_call(true).into() },
		ExpectTransactStatus(MaybeErrorCode::Success),
	])
}

fn sync_program(is_sufficient: bool, min_balance: u128) -> Xcm<RuntimeCall> {
	Xcm(alloc::vec![
		UnpaidExecution { weight_limit: Unlimited, check_origin: None },
		DescendOrigin([PalletInstance(FORWARDER_PALLET_INDEX)].into()),
		Transact {
			origin_kind: OriginKind::Xcm,
			fallback_max_weight: None,
			call: force_asset_status_call(is_sufficient, min_balance).into()
		},
		ExpectTransactStatus(MaybeErrorCode::Success),
	])
}

fn execute(origin: Location, message: Xcm<RuntimeCall>) -> Outcome {
	let mut hash = message.using_encoded(sp_io::hashing::blake2_256);
	XcmExecutor::<XcmConfig>::prepare_and_execute(
		origin,
		message,
		&mut hash,
		Weight::MAX,
		Weight::zero(),
	)
}

#[test]
fn assets_pallet_index_matches_forwarder_contract() {
	// The forwarder on Asset Hub hardcodes this index as `RemoteAssetsPalletIndex`.
	assert_eq!(assets_pallet_index(), 14);
}

#[test]
fn forwarder_program_creates_asset() {
	new_test_ext().execute_with(|| {
		let outcome = execute(AssetHubLocation::get(), forward_program(OriginKind::Xcm, 37));
		assert!(outcome.ensure_complete().is_ok());

		let details =
			pallet_assets::Asset::<Runtime>::get(remote_asset_id()).expect("asset exists");
		assert_eq!(details.owner, owner_account());
		assert_eq!(details.issuer, owner_account());
		assert_eq!(details.min_balance, MIN_BALANCE);
		assert!(details.is_sufficient);

		// Replicas carry no metadata; their identity is the asset id location.
		let metadata = pallet_assets::Metadata::<Runtime>::get(remote_asset_id());
		assert!(metadata.name.is_empty());
		assert!(metadata.symbol.is_empty());
		assert_eq!(metadata.decimals, 0);
	});
}

#[test]
fn forwarder_program_syncs_asset_status() {
	new_test_ext().execute_with(|| {
		let outcome = execute(AssetHubLocation::get(), forward_program(OriginKind::Xcm, 37));
		assert!(outcome.ensure_complete().is_ok());

		let outcome = execute(AssetHubLocation::get(), sync_program(false, MIN_BALANCE * 2));
		assert!(outcome.ensure_complete().is_ok());

		let details =
			pallet_assets::Asset::<Runtime>::get(remote_asset_id()).expect("asset exists");
		assert_eq!(details.min_balance, MIN_BALANCE * 2);
		assert!(!details.is_sufficient);
	});
}

#[test]
fn wrong_pallet_index_cannot_force_create() {
	new_test_ext().execute_with(|| {
		let outcome = execute(AssetHubLocation::get(), forward_program(OriginKind::Xcm, 38));
		assert!(outcome.ensure_complete().is_err());
		assert!(pallet_assets::Asset::<Runtime>::get(remote_asset_id()).is_none());
	});
}

#[test]
fn non_asset_hub_sibling_cannot_force_create() {
	new_test_ext().execute_with(|| {
		let hydration = Location::new(1, [Parachain(2034)]);
		let outcome = execute(hydration, forward_program(OriginKind::Xcm, 37));
		assert!(outcome.ensure_complete().is_err());
		assert!(pallet_assets::Asset::<Runtime>::get(remote_asset_id()).is_none());
	});
}

#[test]
fn sovereign_account_origin_kind_cannot_force_create() {
	new_test_ext().execute_with(|| {
		let outcome =
			execute(AssetHubLocation::get(), forward_program(OriginKind::SovereignAccount, 37));
		assert!(outcome.ensure_complete().is_err());
		assert!(pallet_assets::Asset::<Runtime>::get(remote_asset_id()).is_none());
	});
}

#[test]
fn barrier_admits_forward_program_only_from_asset_hub() {
	type Barrier = <XcmConfig as xcm_executor::Config>::Barrier;

	let mut program = forward_program(OriginKind::Xcm, 37);
	let mut properties = Properties { weight_credit: Weight::zero(), message_id: None };
	assert!(Barrier::should_execute(
		&AssetHubLocation::get(),
		program.inner_mut(),
		Weight::MAX,
		&mut properties
	)
	.is_ok());

	let mut program = forward_program(OriginKind::Xcm, 37);
	assert!(Barrier::should_execute(
		&Location::new(1, [Parachain(2034)]),
		program.inner_mut(),
		Weight::MAX,
		&mut properties
	)
	.is_err());
}
