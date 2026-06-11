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

use codec::Compact;
use frame_support::traits::Contains;
use indiv_pallet_value_transfer_auth::extension::block_flag;
use next_asset_hub_paseo_runtime::{
	value_transfer_filter::AhNextValueTransferFilter,
	xcm_config::XcmOriginToTransactDispatchOrigin, Runtime, RuntimeCall, RuntimeOrigin,
};
use paseo_runtime_constants::{ProtectedAssetLocation, PROTECTED_ASSET_ID};
use sp_runtime::MultiAddress;
use xcm::latest::{Location, OriginKind};
use xcm_executor::traits::ConvertOrigin;

type AccountId = sp_core::crypto::AccountId32;

fn alice() -> AccountId {
	AccountId::new([1u8; 32])
}

fn zero() -> AccountId {
	AccountId::new([0u8; 32])
}

fn protected_asset_transfer() -> RuntimeCall {
	RuntimeCall::Assets(pallet_assets::Call::transfer {
		id: Compact(PROTECTED_ASSET_ID),
		target: MultiAddress::Id(alice()),
		amount: 500,
	})
}

#[test]
fn assets_protected_asset_transfer_classifies_as_value() {
	let call = protected_asset_transfer();
	assert!(AhNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
	block_flag::unblock();
	assert!(<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
	block_flag::block();
}

#[test]
fn assets_non_protected_asset_transfer_classifies_as_non_value() {
	let call = RuntimeCall::Assets(pallet_assets::Call::transfer {
		id: Compact(PROTECTED_ASSET_ID.wrapping_add(1)),
		target: MultiAddress::Id(alice()),
		amount: 500,
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn balances_transfer_classifies_as_non_value() {
	let call = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
		dest: MultiAddress::Id(alice()),
		value: 1_000,
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn xcm_limited_reserve_transfer_classifies_as_non_value() {
	use xcm::{
		latest::prelude::{Assets as XcmAssets, Here, Location, WeightLimit},
		VersionedAssets, VersionedLocation,
	};

	let call = RuntimeCall::PolkadotXcm(pallet_xcm::Call::limited_reserve_transfer_assets {
		dest: Box::new(VersionedLocation::V5(Location::new(1, Here))),
		beneficiary: Box::new(VersionedLocation::V5(Location::here())),
		assets: Box::new(VersionedAssets::V5(XcmAssets::new())),
		fee_asset_item: 0,
		weight_limit: WeightLimit::Unlimited,
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn system_remark_classifies_as_non_value() {
	let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn utility_batch_wrapping_protected_asset_transfer_does_not_recurse() {
	let call = RuntimeCall::Utility(pallet_utility::Call::batch {
		calls: vec![protected_asset_transfer()],
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn proxy_wrapping_protected_asset_transfer_does_not_recurse() {
	let call = RuntimeCall::Proxy(pallet_proxy::Call::proxy {
		real: MultiAddress::Id(zero()),
		force_proxy_type: None,
		call: Box::new(protected_asset_transfer()),
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn multisig_wrapping_protected_asset_transfer_does_not_recurse() {
	let call = RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
		other_signatories: vec![],
		call: Box::new(protected_asset_transfer()),
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn utility_as_derivative_wrapping_protected_asset_transfer_does_not_recurse() {
	let call = RuntimeCall::Utility(pallet_utility::Call::as_derivative {
		index: 0u16,
		call: Box::new(protected_asset_transfer()),
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn relay_parent_converts_to_superuser() {
	// `ParentAsSuperuser` is required for the relay's staking-async messages
	// (`Transact { origin_kind: Superuser, .. }` session reports/offences) to dispatch as root.
	let result: Result<RuntimeOrigin, _> = XcmOriginToTransactDispatchOrigin::convert_origin(
		Location::parent(),
		OriginKind::Superuser,
	);

	assert!(result.is_ok());
}

fn protected_asset_loc() -> Location {
	ProtectedAssetLocation::get()
}

fn non_protected_asset_loc() -> Location {
	Location::parent()
}

#[test]
fn asset_conversion_add_liquidity_with_protected_asset_is_value() {
	let call = RuntimeCall::AssetConversion(pallet_asset_conversion::Call::add_liquidity {
		asset1: Box::new(protected_asset_loc()),
		asset2: Box::new(non_protected_asset_loc()),
		amount1_desired: 100,
		amount2_desired: 100,
		amount1_min: 0,
		amount2_min: 0,
		mint_to: alice(),
	});
	assert!(AhNextValueTransferFilter::contains(&call));
}

#[test]
fn asset_conversion_add_liquidity_without_protected_asset_is_not_value() {
	let call = RuntimeCall::AssetConversion(pallet_asset_conversion::Call::add_liquidity {
		asset1: Box::new(non_protected_asset_loc()),
		asset2: Box::new(Location::new(1, [xcm::latest::Junction::Parachain(2000)])),
		amount1_desired: 100,
		amount2_desired: 100,
		amount1_min: 0,
		amount2_min: 0,
		mint_to: alice(),
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn asset_conversion_remove_liquidity_with_protected_asset_is_value() {
	let call = RuntimeCall::AssetConversion(pallet_asset_conversion::Call::remove_liquidity {
		asset1: Box::new(non_protected_asset_loc()),
		asset2: Box::new(protected_asset_loc()),
		lp_token_burn: 100,
		amount1_min_receive: 0,
		amount2_min_receive: 0,
		withdraw_to: alice(),
	});
	assert!(AhNextValueTransferFilter::contains(&call));
}

#[test]
fn asset_conversion_swap_exact_with_protected_asset_in_path_is_value() {
	let call =
		RuntimeCall::AssetConversion(pallet_asset_conversion::Call::swap_exact_tokens_for_tokens {
			path: vec![Box::new(non_protected_asset_loc()), Box::new(protected_asset_loc())],
			amount_in: 100,
			amount_out_min: 0,
			send_to: alice(),
			keep_alive: false,
		});
	assert!(AhNextValueTransferFilter::contains(&call));
}

#[test]
fn asset_conversion_swap_tokens_for_exact_with_protected_asset_mid_path_is_value() {
	let call =
		RuntimeCall::AssetConversion(pallet_asset_conversion::Call::swap_tokens_for_exact_tokens {
			path: vec![
				Box::new(non_protected_asset_loc()),
				Box::new(protected_asset_loc()),
				Box::new(Location::new(1, [xcm::latest::Junction::Parachain(2000)])),
			],
			amount_out: 100,
			amount_in_max: u128::MAX,
			send_to: alice(),
			keep_alive: false,
		});
	assert!(AhNextValueTransferFilter::contains(&call));
}

#[test]
fn asset_conversion_swap_without_protected_asset_is_not_value() {
	let call =
		RuntimeCall::AssetConversion(pallet_asset_conversion::Call::swap_exact_tokens_for_tokens {
			path: vec![
				Box::new(non_protected_asset_loc()),
				Box::new(Location::new(1, [xcm::latest::Junction::Parachain(2000)])),
			],
			amount_in: 100,
			amount_out_min: 0,
			send_to: alice(),
			keep_alive: false,
		});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn asset_conversion_create_pool_with_protected_asset_is_not_value() {
	let call = RuntimeCall::AssetConversion(pallet_asset_conversion::Call::create_pool {
		asset1: Box::new(protected_asset_loc()),
		asset2: Box::new(non_protected_asset_loc()),
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}

#[test]
fn asset_conversion_touch_with_protected_asset_is_not_value() {
	let call = RuntimeCall::AssetConversion(pallet_asset_conversion::Call::touch {
		asset1: Box::new(protected_asset_loc()),
		asset2: Box::new(non_protected_asset_loc()),
	});
	assert!(!AhNextValueTransferFilter::contains(&call));
}
