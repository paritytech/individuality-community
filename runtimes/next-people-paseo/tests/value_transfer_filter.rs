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

use codec::Encode;
use frame_support::traits::Contains;
use indiv_pallet_value_transfer_auth::extension::block_flag;
use next_people_paseo_runtime::{
	value_transfer_filter::PeopleNextValueTransferFilter,
	xcm_config::{NextAhLocation, XcmOriginToTransactDispatchOrigin},
	Runtime, RuntimeCall, RuntimeOrigin,
};
use paseo_runtime_constants::ProtectedAssetLocation;
use sp_runtime::{AccountId32, MultiAddress};
use verifiable::{ring::bandersnatch::BandersnatchVrfVerifiable as Crypto, GenerateVerifiable};
use xcm::latest::{Location, OriginKind};
use xcm_executor::traits::ConvertOrigin;

fn coinage_transfer() -> RuntimeCall {
	RuntimeCall::Coinage(indiv_pallet_coinage::Call::transfer { to: AccountId32::new([1u8; 32]) })
}

fn load_recycler_with_external_asset() -> RuntimeCall {
	let who = AccountId32::new([9u8; 32]);
	let secret = Crypto::new_secret([42u8; 32]);
	let member_key = Crypto::member_from_secret(&secret);
	let proof_of_ownership =
		Crypto::sign(&secret, &who.encode()).expect("test member key signs account id");

	RuntimeCall::Coinage(indiv_pallet_coinage::Call::load_recycler_with_external_asset {
		preservation: indiv_pallet_coinage::CodecPreservation::Expendable,
		value: 1,
		member_key,
		proof_of_ownership,
	})
}

fn load_recycler_with_external_asset_unpaid() -> RuntimeCall {
	let who = AccountId32::new([9u8; 32]);
	let secret = Crypto::new_secret([43u8; 32]);
	let member_key = Crypto::member_from_secret(&secret);
	let proof_of_ownership =
		Crypto::sign(&secret, &who.encode()).expect("test member key signs account id");

	RuntimeCall::Coinage(indiv_pallet_coinage::Call::load_recycler_with_external_asset_unpaid {
		preservation: indiv_pallet_coinage::CodecPreservation::Expendable,
		value: 1,
		member_key,
		proof_of_ownership,
	})
}

fn load_recycler_with_external_asset_unpaid_batch() -> RuntimeCall {
	let who = AccountId32::new([9u8; 32]);
	let secret = Crypto::new_secret([44u8; 32]);
	let member_key = Crypto::member_from_secret(&secret);
	let proof_of_ownership =
		Crypto::sign(&secret, &who.encode()).expect("test member key signs account id");
	let item = indiv_pallet_coinage::UnpaidLoadInput::<Runtime> {
		preservation: indiv_pallet_coinage::CodecPreservation::Expendable,
		value: 1,
		member_key,
		proof_of_ownership,
	};

	RuntimeCall::Coinage(
		indiv_pallet_coinage::Call::load_recycler_with_external_asset_unpaid_batch {
			items: vec![item].try_into().expect("single unpaid load fits batch bound"),
		},
	)
}

fn load_recycler_with_coin() -> RuntimeCall {
	let who = AccountId32::new([9u8; 32]);
	let secret = Crypto::new_secret([45u8; 32]);
	let member_key = Crypto::member_from_secret(&secret);
	let proof_of_ownership =
		Crypto::sign(&secret, &who.encode()).expect("test member key signs account id");

	RuntimeCall::Coinage(indiv_pallet_coinage::Call::load_recycler_with_coin {
		member_key,
		proof_of_ownership,
	})
}

#[test]
fn coinage_transfer_is_value() {
	assert!(PeopleNextValueTransferFilter::contains(&coinage_transfer()));
}

#[test]
fn coinage_unload_recycler_into_coin_is_value() {
	let call = RuntimeCall::Coinage(indiv_pallet_coinage::Call::unload_recycler_into_coin {
		aliases: Default::default(),
		value: 0,
		index: 0,
		revision: 0,
		to: AccountId32::new([0u8; 32]),
	});
	assert!(PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn coinage_split_is_value_and_blocked_by_base_call_filter() {
	let call =
		RuntimeCall::Coinage(indiv_pallet_coinage::Call::split { split_into: Default::default() });

	assert!(PeopleNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
}

#[test]
fn coinage_load_recycler_with_coin_is_value_and_blocked_by_base_call_filter() {
	let call = load_recycler_with_coin();

	assert!(PeopleNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
}

#[test]
fn coinage_direct_offboard_coin_into_external_asset_is_value_and_blocked_by_base_call_filter() {
	let call = RuntimeCall::Coinage(
		indiv_pallet_coinage::Call::direct_offboard_coin_into_external_asset {
			to: AccountId32::new([0u8; 32]),
		},
	);

	assert!(PeopleNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
}

#[test]
fn coinage_load_recycler_with_external_asset_is_not_value() {
	let call = load_recycler_with_external_asset();

	assert!(!PeopleNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
}

#[test]
fn coinage_load_recycler_with_external_asset_unpaid_is_not_value_and_passes_base_call_filter() {
	let call = load_recycler_with_external_asset_unpaid();

	assert!(!PeopleNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
}

#[test]
fn coinage_load_recycler_with_external_asset_unpaid_batch_is_not_value_and_passes_base_call_filter()
{
	let call = load_recycler_with_external_asset_unpaid_batch();

	assert!(!PeopleNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
}

#[test]
fn assets_protected_asset_transfer_is_value() {
	let call = RuntimeCall::Assets(pallet_assets::Call::transfer {
		id: ProtectedAssetLocation::get(),
		target: MultiAddress::Id(AccountId32::new([0u8; 32])),
		amount: 100,
	});
	assert!(PeopleNextValueTransferFilter::contains(&call));
	block_flag::block();
	assert!(!<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
	block_flag::unblock();
	assert!(<Runtime as frame_system::Config>::BaseCallFilter::contains(&call));
	block_flag::block();
}

#[test]
fn assets_non_protected_asset_transfer_is_not_value() {
	let call = RuntimeCall::Assets(pallet_assets::Call::transfer {
		id: xcm::latest::Location::here(),
		target: MultiAddress::Id(AccountId32::new([0u8; 32])),
		amount: 100,
	});
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn balances_transfer_is_not_value() {
	let call = RuntimeCall::Balances(pallet_balances::Call::transfer_allow_death {
		dest: MultiAddress::Id(AccountId32::new([0u8; 32])),
		value: 100,
	});
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn xcm_limited_teleport_assets_is_not_value() {
	let call = RuntimeCall::PolkadotXcm(pallet_xcm::Call::limited_teleport_assets {
		dest: Box::new(xcm::VersionedLocation::V5(xcm::latest::Location::here())),
		beneficiary: Box::new(xcm::VersionedLocation::V5(xcm::latest::Location::here())),
		assets: Box::new(xcm::VersionedAssets::V5(Default::default())),
		fee_asset_item: 0,
		weight_limit: xcm::latest::WeightLimit::Unlimited,
	});
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn system_remark_is_non_value() {
	let call = RuntimeCall::System(frame_system::Call::remark { remark: Default::default() });
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn utility_batch_wrapping_coinage_transfer_does_not_recurse() {
	let call =
		RuntimeCall::Utility(pallet_utility::Call::batch { calls: vec![coinage_transfer()] });
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn proxy_wrapping_coinage_transfer_does_not_recurse() {
	let call = RuntimeCall::Proxy(pallet_proxy::Call::proxy {
		real: MultiAddress::Id(AccountId32::new([0u8; 32])),
		force_proxy_type: None,
		call: Box::new(coinage_transfer()),
	});
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn multisig_wrapping_coinage_transfer_does_not_recurse() {
	let call = RuntimeCall::Multisig(pallet_multisig::Call::as_multi_threshold_1 {
		other_signatories: vec![],
		call: Box::new(coinage_transfer()),
	});
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn utility_as_derivative_wrapping_coinage_transfer_does_not_recurse() {
	let call = RuntimeCall::Utility(pallet_utility::Call::as_derivative {
		index: 0u16,
		call: Box::new(coinage_transfer()),
	});
	assert!(!PeopleNextValueTransferFilter::contains(&call));
}

#[test]
fn next_asset_hub_converts_to_superuser() {
	let origin: RuntimeOrigin = XcmOriginToTransactDispatchOrigin::convert_origin(
		NextAhLocation::get(),
		OriginKind::Superuser,
	)
	.expect("AH-next converts to root");

	assert!(frame_system::ensure_root(origin).is_ok());
}

#[test]
fn relay_parent_does_not_convert_to_superuser() {
	let result: Result<RuntimeOrigin, _> = XcmOriginToTransactDispatchOrigin::convert_origin(
		Location::parent(),
		OriginKind::Superuser,
	);

	assert!(result.is_err());
}
