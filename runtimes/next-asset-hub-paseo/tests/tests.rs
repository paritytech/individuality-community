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

//! Tests for the Paseo Asset Hub (previously known as Statemint) chain.

use asset_test_utils::{
	include_create_and_manage_foreign_assets_for_local_consensus_parachain_assets_works,
	include_teleports_for_foreign_assets_works,
	test_cases_over_bridge::{
		receive_reserve_asset_deposited_from_different_consensus_works, TestBridgingConfig,
	},
	CollatorSessionKey, CollatorSessionKeys, ExtBuilder, GovernanceOrigin, SlotDurations,
};
use assets_common::local_and_foreign_assets::ForeignAssetReserveData;
use codec::{Decode, Encode};
use frame_support::{
	assert_err, assert_ok,
	traits::{fungibles::InspectEnumerable, ContainsPair},
};
use next_asset_hub_paseo_runtime::{
	genesis_config_presets::EXTERNAL_ASSET_ID,
	xcm_config::{
		bridging, CheckingAccount, DotLocation, ExternalAssetLocation, LocationToAccountId,
		RelayChainLocation, StakingPot, TrustBackedAssetsPalletLocation, XcmConfig,
	},
	AllPalletsWithoutSystem, AssetDeposit, Assets, Balances, Block, ExistentialDeposit,
	ForeignAssets, ForeignAssetsInstance, MetadataDepositBase, MetadataDepositPerByte,
	ParachainSystem, PolkadotXcm, Runtime, RuntimeCall, RuntimeEvent, RuntimeOrigin, SessionKeys,
	ToKusamaXcmRouterInstance, TrustBackedAssetsInstance, XcmpQueue, SLOT_DURATION,
};
use parachains_common::{AccountId, AssetIdForTrustBackedAssets, AuraId, Balance};
use sp_consensus_aura::SlotDuration;
use sp_core::crypto::Ss58Codec;
use sp_runtime::{traits::MaybeEquivalence, Either, TryRuntimeError};
use system_parachains_constants::paseo::{
	consensus::RELAY_CHAIN_SLOT_DURATION_MILLIS, currency::UNITS,
	fee::WeightToFee as PaseoWeightToFee,
};
use xcm::latest::{
	prelude::{Assets as XcmAssets, *},
	WESTEND_GENESIS_HASH,
};
use xcm_builder::WithLatestLocationConverter;
use xcm_executor::traits::ConvertLocation;
use xcm_runtime_apis::conversions::LocationToAccountHelper;

const ALICE: [u8; 32] = [1u8; 32];
const SOME_ASSET_ADMIN: [u8; 32] = [5u8; 32];

frame_support::parameter_types! {
	// Local OpenGov
	pub Governance: GovernanceOrigin<RuntimeOrigin> = GovernanceOrigin::Origin(RuntimeOrigin::root());
}

type AssetIdForTrustBackedAssetsConvertLatest =
	assets_common::AssetIdForTrustBackedAssetsConvert<TrustBackedAssetsPalletLocation>;

type RuntimeHelper = asset_test_utils::RuntimeHelper<Runtime, AllPalletsWithoutSystem>;
type WeightToFee = PaseoWeightToFee<Runtime>;

fn collator_session_key(account: [u8; 32]) -> CollatorSessionKey<Runtime> {
	CollatorSessionKey::new(
		AccountId::from(account),
		AccountId::from(account),
		SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(account)) },
	)
}

fn collator_session_keys() -> CollatorSessionKeys<Runtime> {
	CollatorSessionKeys::default().add(collator_session_key(ALICE))
}

fn slot_durations() -> SlotDurations {
	SlotDurations {
		relay: SlotDuration::from_millis(RELAY_CHAIN_SLOT_DURATION_MILLIS.into()),
		para: SlotDuration::from_millis(SLOT_DURATION),
	}
}

#[test]
fn test_ed_is_one_hundredth_of_relay() {
	ExtBuilder::<Runtime>::default()
		.with_tracing()
		.with_collators(vec![AccountId::from(ALICE)])
		.with_session_keys(vec![(
			AccountId::from(ALICE),
			AccountId::from(ALICE),
			SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(ALICE)) },
		)])
		.build()
		.execute_with(|| {
			let relay_ed = paseo_runtime_constants::currency::EXISTENTIAL_DEPOSIT;
			let asset_hub_ed = ExistentialDeposit::get();
			assert_eq!(relay_ed / 100, asset_hub_ed);
		});
}

#[test]
fn test_assets_balances_api_works() {
	use assets_common::runtime_api::runtime_decl_for_fungibles_api::FungiblesApi;

	ExtBuilder::<Runtime>::default()
		.with_tracing()
		.with_collators(vec![AccountId::from(ALICE)])
		.with_session_keys(vec![(
			AccountId::from(ALICE),
			AccountId::from(ALICE),
			SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(ALICE)) },
		)])
		.build()
		.execute_with(|| {
			let local_asset_id = 1;
			let foreign_asset_id_location =
				Location::new(1, [Parachain(1234), GeneralIndex(12345)]);

			// check before
			assert_eq!(Assets::balance(local_asset_id, AccountId::from(ALICE)), 0);
			assert_eq!(
				ForeignAssets::balance(foreign_asset_id_location.clone(), AccountId::from(ALICE)),
				0
			);
			assert_eq!(Balances::free_balance(AccountId::from(ALICE)), 0);
			assert!(Runtime::query_account_balances(AccountId::from(ALICE))
				.unwrap()
				.try_as::<XcmAssets>()
				.unwrap()
				.is_none());

			// Drip some balance
			use frame_support::traits::fungible::Mutate;
			let some_currency = ExistentialDeposit::get();
			Balances::mint_into(&AccountId::from(ALICE), some_currency).unwrap();

			// We need root origin to create a sufficient asset
			let minimum_asset_balance = 3333333_u128;
			assert_ok!(Assets::force_create(
				RuntimeHelper::root_origin(),
				local_asset_id.into(),
				AccountId::from(ALICE).into(),
				true,
				minimum_asset_balance
			));

			// We first mint enough asset for the account to exist for assets
			assert_ok!(Assets::mint(
				RuntimeHelper::origin_of(AccountId::from(ALICE)),
				local_asset_id.into(),
				AccountId::from(ALICE).into(),
				minimum_asset_balance
			));

			// create foreign asset
			let foreign_asset_minimum_asset_balance = 3333333_u128;
			assert_ok!(ForeignAssets::force_create(
				RuntimeHelper::root_origin(),
				foreign_asset_id_location.clone(),
				AccountId::from(SOME_ASSET_ADMIN).into(),
				false,
				foreign_asset_minimum_asset_balance
			));

			// We first mint enough asset for the account to exist for assets
			assert_ok!(ForeignAssets::mint(
				RuntimeHelper::origin_of(AccountId::from(SOME_ASSET_ADMIN)),
				foreign_asset_id_location.clone(),
				AccountId::from(ALICE).into(),
				6 * foreign_asset_minimum_asset_balance
			));

			// check after
			assert_eq!(
				Assets::balance(local_asset_id, AccountId::from(ALICE)),
				minimum_asset_balance
			);
			assert_eq!(
				ForeignAssets::balance(foreign_asset_id_location.clone(), AccountId::from(ALICE)),
				6 * minimum_asset_balance
			);
			assert_eq!(Balances::free_balance(AccountId::from(ALICE)), some_currency);

			let result: XcmAssets = Runtime::query_account_balances(AccountId::from(ALICE))
				.unwrap()
				.try_into()
				.unwrap();
			assert_eq!(result.len(), 3);

			// check currency
			assert!(result.inner().iter().any(|asset| asset.eq(
				&assets_common::fungible_conversion::convert_balance::<DotLocation, Balance>(
					some_currency
				)
				.unwrap()
			)));
			// check trusted asset
			assert!(result.inner().iter().any(|asset| asset.eq(&(
				AssetIdForTrustBackedAssetsConvertLatest::convert_back(&local_asset_id).unwrap(),
				minimum_asset_balance
			)
				.into())));
			// check foreign asset
			assert!(result.inner().iter().any(|asset| asset.eq(&(
				WithLatestLocationConverter::convert_back(&foreign_asset_id_location).unwrap(),
				6 * foreign_asset_minimum_asset_balance
			)
				.into())));
		});
}

asset_test_utils::include_teleports_for_native_asset_works!(
	Runtime,
	AllPalletsWithoutSystem,
	XcmConfig,
	// TODO: after AHM change this from `()` to `CheckingAccount`
	(),
	WeightToFee,
	ParachainSystem,
	collator_session_keys(),
	slot_durations(),
	ExistentialDeposit::get(),
	Box::new(|runtime_event_encoded: Vec<u8>| {
		match RuntimeEvent::decode(&mut &runtime_event_encoded[..]) {
			Ok(RuntimeEvent::PolkadotXcm(event)) => Some(event),
			_ => None,
		}
	}),
	1000
);

include_teleports_for_foreign_assets_works!(
	Runtime,
	AllPalletsWithoutSystem,
	XcmConfig,
	CheckingAccount,
	WeightToFee,
	ParachainSystem,
	LocationToAccountId,
	ForeignAssetsInstance,
	collator_session_keys(),
	slot_durations(),
	ExistentialDeposit::get(),
	Box::new(|runtime_event_encoded: Vec<u8>| {
		match RuntimeEvent::decode(&mut &runtime_event_encoded[..]) {
			Ok(RuntimeEvent::PolkadotXcm(event)) => Some(event),
			_ => None,
		}
	}),
	Box::new(|runtime_event_encoded: Vec<u8>| {
		match RuntimeEvent::decode(&mut &runtime_event_encoded[..]) {
			Ok(RuntimeEvent::XcmpQueue(event)) => Some(event),
			_ => None,
		}
	})
);

asset_test_utils::include_asset_transactor_transfer_with_local_consensus_currency_works!(
	Runtime,
	XcmConfig,
	collator_session_keys(),
	ExistentialDeposit::get(),
	Box::new(|| {
		assert!(Assets::asset_ids().collect::<Vec<_>>().is_empty());
		assert!(ForeignAssets::asset_ids().collect::<Vec<_>>().is_empty());
	}),
	Box::new(|| {
		assert!(Assets::asset_ids().collect::<Vec<_>>().is_empty());
		assert!(ForeignAssets::asset_ids().collect::<Vec<_>>().is_empty());
	})
);

asset_test_utils::include_asset_transactor_transfer_with_pallet_assets_instance_works!(
	asset_transactor_transfer_with_trust_backed_assets_works,
	Runtime,
	XcmConfig,
	TrustBackedAssetsInstance,
	AssetIdForTrustBackedAssets,
	AssetIdForTrustBackedAssetsConvertLatest,
	collator_session_keys(),
	ExistentialDeposit::get(),
	12345,
	Box::new(|| {
		assert!(ForeignAssets::asset_ids().collect::<Vec<_>>().is_empty());
	}),
	Box::new(|| {
		assert!(ForeignAssets::asset_ids().collect::<Vec<_>>().is_empty());
	})
);

asset_test_utils::include_asset_transactor_transfer_with_pallet_assets_instance_works!(
	asset_transactor_transfer_with_foreign_assets_works,
	Runtime,
	XcmConfig,
	ForeignAssetsInstance,
	Location,
	WithLatestLocationConverter<Location>,
	collator_session_keys(),
	ExistentialDeposit::get(),
	Location::new(1, [Parachain(1313), GeneralIndex(12345)]),
	Box::new(|| {
		assert!(Assets::asset_ids().collect::<Vec<_>>().is_empty());
	}),
	Box::new(|| {
		assert!(Assets::asset_ids().collect::<Vec<_>>().is_empty());
	})
);

include_create_and_manage_foreign_assets_for_local_consensus_parachain_assets_works!(
	Runtime,
	XcmConfig,
	WeightToFee,
	LocationToAccountId,
	ForeignAssetsInstance,
	Location,
	WithLatestLocationConverter<Location>,
	collator_session_keys(),
	ExistentialDeposit::get(),
	AssetDeposit::get(),
	MetadataDepositBase::get(),
	MetadataDepositPerByte::get(),
	Box::new(|pallet_asset_call| RuntimeCall::ForeignAssets(pallet_asset_call).encode()),
	Box::new(|runtime_event_encoded: Vec<u8>| {
		match RuntimeEvent::decode(&mut &runtime_event_encoded[..]) {
			Ok(RuntimeEvent::ForeignAssets(pallet_asset_event)) => Some(pallet_asset_event),
			_ => None,
		}
	}),
	Box::new(|| {
		assert!(Assets::asset_ids().collect::<Vec<_>>().is_empty());
		assert!(ForeignAssets::asset_ids().collect::<Vec<_>>().is_empty());
	}),
	Box::new(|| {
		assert!(Assets::asset_ids().collect::<Vec<_>>().is_empty());
		assert_eq!(ForeignAssets::asset_ids().collect::<Vec<_>>().len(), 1);
	})
);

fn bridging_to_asset_hub_kusama() -> TestBridgingConfig {
	PolkadotXcm::force_xcm_version(
		RuntimeOrigin::root(),
		Box::new(bridging::to_kusama::AssetHubKusama::get()),
		XCM_VERSION,
	)
	.expect("version saved!");
	TestBridgingConfig {
		bridged_network: bridging::to_kusama::KusamaNetwork::get(),
		local_bridge_hub_para_id: bridging::SiblingBridgeHubParaId::get(),
		local_bridge_hub_location: bridging::SiblingBridgeHub::get(),
		bridged_target_location: bridging::to_kusama::AssetHubKusama::get(),
	}
}

#[test]
fn receive_reserve_asset_deposited_ksm_from_asset_hub_kusama_fees_paid_by_pool_swap_works() {
	const BLOCK_AUTHOR_ACCOUNT: [u8; 32] = [13; 32];
	let block_author_account = AccountId::from(BLOCK_AUTHOR_ACCOUNT);
	let staking_pot = StakingPot::get();

	let foreign_asset_id_location_v5 = Location::new(2, [GlobalConsensus(NetworkId::Kusama)]);
	let reserve_location = Location::new(2, [GlobalConsensus(NetworkId::Kusama), Parachain(1000)]);
	let foreign_asset_reserve_data =
		ForeignAssetReserveData { reserve: reserve_location, teleportable: false };
	let foreign_asset_id_minimum_balance = 1_000_000_000;
	// sovereign account as foreign asset owner (can be whoever for this scenario)
	let foreign_asset_owner = LocationToAccountId::convert_location(&Location::parent()).unwrap();
	let foreign_asset_create_params = (
		foreign_asset_owner.clone(),
		foreign_asset_id_location_v5.clone(),
		foreign_asset_reserve_data,
		foreign_asset_id_minimum_balance,
	);
	let pool_params = (
		foreign_asset_owner,
		foreign_asset_id_location_v5.clone(),
		foreign_asset_id_minimum_balance,
	);

	receive_reserve_asset_deposited_from_different_consensus_works::<
		Runtime,
		AllPalletsWithoutSystem,
		XcmConfig,
		ForeignAssetsInstance,
	>(
		collator_session_keys().add(collator_session_key(BLOCK_AUTHOR_ACCOUNT)),
		ExistentialDeposit::get(),
		AccountId::from([73; 32]),
		block_author_account.clone(),
		// receiving KSMs
		foreign_asset_create_params,
		1000000000000,
		|| {
			// setup pool for paying fees to touch `SwapFirstAssetTrader`
			asset_test_utils::test_cases::setup_pool_for_paying_fees_with_foreign_assets::<
				Runtime,
				RuntimeOrigin,
			>(ExistentialDeposit::get(), pool_params);
			// staking pot account for collecting local native fees from `BuyExecution`
			let _ = Balances::force_set_balance(
				RuntimeOrigin::root(),
				StakingPot::get().into(),
				ExistentialDeposit::get(),
			);
			// prepare bridge configuration
			bridging_to_asset_hub_kusama()
		},
		(
			[PalletInstance(
				bp_bridge_hub_paseo::WITH_BRIDGE_POLKADOT_TO_KUSAMA_MESSAGES_PALLET_INDEX,
			)]
			.into(),
			GlobalConsensus(Kusama),
			[Parachain(1000)].into(),
		),
		|| {
			// check staking pot for ED
			assert_eq!(Balances::free_balance(&staking_pot), ExistentialDeposit::get());
			// check now foreign asset for staking pot
			assert_eq!(
				ForeignAssets::balance(foreign_asset_id_location_v5.clone(), &staking_pot),
				0
			);
		},
		|| {
			// `SwapFirstAssetTrader` - staking pot receives xcm fees in KSMs
			assert!(Balances::free_balance(&staking_pot) > ExistentialDeposit::get());
			// staking pot receives no foreign assets
			assert_eq!(
				ForeignAssets::balance(foreign_asset_id_location_v5.clone(), &staking_pot),
				0
			);
		},
	)
}

#[test]
fn reserve_transfer_native_asset_to_non_teleport_para_works() {
	asset_test_utils::test_cases::reserve_transfer_native_asset_to_non_teleport_para_works::<
		Runtime,
		AllPalletsWithoutSystem,
		XcmConfig,
		ParachainSystem,
		XcmpQueue,
		LocationToAccountId,
	>(
		collator_session_keys(),
		slot_durations(),
		ExistentialDeposit::get(),
		AccountId::from(ALICE),
		Box::new(|runtime_event_encoded: Vec<u8>| {
			match RuntimeEvent::decode(&mut &runtime_event_encoded[..]) {
				Ok(RuntimeEvent::PolkadotXcm(event)) => Some(event),
				_ => None,
			}
		}),
		Box::new(|runtime_event_encoded: Vec<u8>| {
			match RuntimeEvent::decode(&mut &runtime_event_encoded[..]) {
				Ok(RuntimeEvent::XcmpQueue(event)) => Some(event),
				_ => None,
			}
		}),
		WeightLimit::Unlimited,
	);
}

#[test]
fn report_bridge_status_from_xcm_bridge_router_for_kusama_works() {
	asset_test_utils::test_cases_over_bridge::report_bridge_status_from_xcm_bridge_router_works::<
		Runtime,
		AllPalletsWithoutSystem,
		XcmConfig,
		LocationToAccountId,
		ToKusamaXcmRouterInstance,
	>(
		collator_session_keys(),
		bridging_to_asset_hub_kusama,
		|| bp_asset_hub_paseo::build_congestion_message(Default::default(), true).into(),
		|| bp_asset_hub_paseo::build_congestion_message(Default::default(), false).into(),
	)
}

#[test]
fn test_report_bridge_status_call_compatibility() {
	// if this test fails, make sure `bp_asset_hub_kusama` has valid encoding
	assert_eq!(
		RuntimeCall::ToKusamaXcmRouter(pallet_xcm_bridge_hub_router::Call::report_bridge_status {
			bridge_id: Default::default(),
			is_congested: true,
		})
		.encode(),
		bp_asset_hub_paseo::Call::ToKusamaXcmRouter(
			bp_asset_hub_paseo::XcmBridgeHubRouterCall::report_bridge_status {
				bridge_id: Default::default(),
				is_congested: true,
			}
		)
		.encode()
	)
}

#[test]
fn check_sane_weight_report_bridge_status() {
	use pallet_xcm_bridge_hub_router::WeightInfo;
	let actual = <Runtime as pallet_xcm_bridge_hub_router::Config<
		ToKusamaXcmRouterInstance,
	>>::WeightInfo::report_bridge_status();
	let max_weight = bp_asset_hub_paseo::XcmBridgeHubRouterTransactCallMaxWeight::get();
	assert!(
		actual.all_lte(max_weight),
		"max_weight: {max_weight:?} should be adjusted to actual {actual:?}"
	);
}

#[test]
fn change_xcm_bridge_hub_router_base_fee_by_governance_works() {
	asset_test_utils::test_cases::change_storage_constant_by_governance_works::<
		Runtime,
		bridging::XcmBridgeHubRouterBaseFee,
		Balance,
	>(
		collator_session_keys(),
		1000,
		Governance::get(),
		|| {
			log::error!(
				target: "bridges::estimate",
				"`bridging::XcmBridgeHubRouterBaseFee` actual value: {} for runtime: {}",
				bridging::XcmBridgeHubRouterBaseFee::get(),
				<Runtime as frame_system::Config>::Version::get(),
			);
			(
				bridging::XcmBridgeHubRouterBaseFee::key().to_vec(),
				bridging::XcmBridgeHubRouterBaseFee::get(),
			)
		},
		|old_value| {
			if let Some(new_value) = old_value.checked_add(1) {
				new_value
			} else {
				old_value.checked_sub(1).unwrap()
			}
		},
	)
}

#[test]
fn change_xcm_bridge_hub_router_byte_fee_by_governance_works() {
	asset_test_utils::test_cases::change_storage_constant_by_governance_works::<
		Runtime,
		bridging::XcmBridgeHubRouterByteFee,
		Balance,
	>(
		collator_session_keys(),
		1000,
		Governance::get(),
		|| {
			(
				bridging::XcmBridgeHubRouterByteFee::key().to_vec(),
				bridging::XcmBridgeHubRouterByteFee::get(),
			)
		},
		|old_value| {
			if let Some(new_value) = old_value.checked_add(1) {
				new_value
			} else {
				old_value.checked_sub(1).unwrap()
			}
		},
	)
}

#[test]
fn change_xcm_bridge_hub_ethereum_base_fee_by_governance_works() {
	asset_test_utils::test_cases::change_storage_constant_by_governance_works::<
		Runtime,
		bridging::to_ethereum::BridgeHubEthereumBaseFee,
		Balance,
	>(
		collator_session_keys(),
		1000,
		Governance::get(),
		|| {
			(
				bridging::to_ethereum::BridgeHubEthereumBaseFee::key().to_vec(),
				bridging::to_ethereum::BridgeHubEthereumBaseFee::get(),
			)
		},
		|old_value| {
			if let Some(new_value) = old_value.checked_add(1) {
				new_value
			} else {
				old_value.checked_sub(1).unwrap()
			}
		},
	)
}

#[test]
fn location_conversion_works() {
	let alice_32 =
		AccountId32 { network: None, id: polkadot_core_primitives::AccountId::from(ALICE).into() };
	let bob_20 = AccountKey20 { network: None, key: [123u8; 20] };

	// the purpose of hardcoded values is to catch an unintended location conversion logic change.
	struct TestCase {
		description: &'static str,
		location: Location,
		expected_account_id_str: &'static str,
	}

	let test_cases = vec![
		// DescribeTerminus
		TestCase {
			description: "DescribeTerminus Parent",
			location: Location::new(1, Here),
			expected_account_id_str: "5Dt6dpkWPwLaH4BBCKJwjiWrFVAGyYk3tLUabvyn4v7KtESG",
		},
		TestCase {
			description: "DescribeTerminus Sibling",
			location: Location::new(1, [Parachain(1111)]),
			expected_account_id_str: "5Eg2fnssmmJnF3z1iZ1NouAuzciDaaDQH7qURAy3w15jULDk",
		},
		// DescribePalletTerminal
		TestCase {
			description: "DescribePalletTerminal Parent",
			location: Location::new(1, [PalletInstance(50)]),
			expected_account_id_str: "5CnwemvaAXkWFVwibiCvf2EjqwiqBi29S5cLLydZLEaEw6jZ",
		},
		TestCase {
			description: "DescribePalletTerminal Sibling",
			location: Location::new(1, [Parachain(1111), PalletInstance(50)]),
			expected_account_id_str: "5GFBgPjpEQPdaxEnFirUoa51u5erVx84twYxJVuBRAT2UP2g",
		},
		// DescribeAccountId32Terminal
		TestCase {
			description: "DescribeAccountId32Terminal Parent",
			location: Location::new(1, [alice_32]),
			expected_account_id_str: "5DN5SGsuUG7PAqFL47J9meViwdnk9AdeSWKFkcHC45hEzVz4",
		},
		TestCase {
			description: "DescribeAccountId32Terminal Sibling",
			location: Location::new(1, [Parachain(1111), alice_32]),
			expected_account_id_str: "5DGRXLYwWGce7wvm14vX1Ms4Vf118FSWQbJkyQigY2pfm6bg",
		},
		// DescribeAccountKey20Terminal
		TestCase {
			description: "DescribeAccountKey20Terminal Parent",
			location: Location::new(1, [bob_20]),
			expected_account_id_str: "5CJeW9bdeos6EmaEofTUiNrvyVobMBfWbdQvhTe6UciGjH2n",
		},
		TestCase {
			description: "DescribeAccountKey20Terminal Sibling",
			location: Location::new(1, [Parachain(1111), bob_20]),
			expected_account_id_str: "5CE6V5AKH8H4rg2aq5KMbvaVUDMumHKVPPQEEDMHPy3GmJQp",
		},
		// DescribeTreasuryVoiceTerminal
		TestCase {
			description: "DescribeTreasuryVoiceTerminal Parent",
			location: Location::new(1, [Plurality { id: BodyId::Treasury, part: BodyPart::Voice }]),
			expected_account_id_str: "5CUjnE2vgcUCuhxPwFoQ5r7p1DkhujgvMNDHaF2bLqRp4D5F",
		},
		TestCase {
			description: "DescribeTreasuryVoiceTerminal Sibling",
			location: Location::new(
				1,
				[Parachain(1111), Plurality { id: BodyId::Treasury, part: BodyPart::Voice }],
			),
			expected_account_id_str: "5G6TDwaVgbWmhqRUKjBhRRnH4ry9L9cjRymUEmiRsLbSE4gB",
		},
		// DescribeBodyTerminal
		TestCase {
			description: "DescribeBodyTerminal Parent",
			location: Location::new(1, [Plurality { id: BodyId::Unit, part: BodyPart::Voice }]),
			expected_account_id_str: "5EBRMTBkDisEXsaN283SRbzx9Xf2PXwUxxFCJohSGo4jYe6B",
		},
		TestCase {
			description: "DescribeBodyTerminal Sibling",
			location: Location::new(
				1,
				[Parachain(1111), Plurality { id: BodyId::Unit, part: BodyPart::Voice }],
			),
			expected_account_id_str: "5DBoExvojy8tYnHgLL97phNH975CyT45PWTZEeGoBZfAyRMH",
		},
	];

	for tc in test_cases {
		let expected = polkadot_core_primitives::AccountId::from_string(tc.expected_account_id_str)
			.expect("Invalid AccountId string");

		let got = LocationToAccountHelper::<polkadot_core_primitives::AccountId, LocationToAccountId>::convert_location(
			tc.location.into(),
		)
			.unwrap();

		assert_eq!(got, expected, "{}", tc.description);
	}
}

#[test]
fn xcm_payment_api_works() {
	parachains_runtimes_test_utils::test_cases::xcm_payment_api_with_native_token_works::<
		Runtime,
		RuntimeCall,
		RuntimeOrigin,
		Block,
		WeightToFee,
	>();
	asset_test_utils::test_cases::xcm_payment_api_with_pools_works::<
		Runtime,
		RuntimeCall,
		RuntimeOrigin,
		Block,
		WeightToFee,
	>();
	asset_test_utils::test_cases::xcm_payment_api_foreign_asset_pool_works::<
		Runtime,
		RuntimeCall,
		RuntimeOrigin,
		LocationToAccountId,
		Block,
		WeightToFee,
	>(ExistentialDeposit::get(), WESTEND_GENESIS_HASH);
}

#[test]
fn test_xcm_v4_to_v5_works() {
	// Test some common XCM location patterns to ensure V4 -> V5 compatibility
	let test_locations_v4 = vec![
		// Relay chain
		xcm::v4::Location::new(1, xcm::v4::Junctions::Here),
		// Sibling parachain
		xcm::v4::Location::new(1, [xcm::v4::Junction::Parachain(1000)]),
		// Asset on sibling parachain
		xcm::v4::Location::new(
			1,
			[
				xcm::v4::Junction::Parachain(1000),
				xcm::v4::Junction::PalletInstance(50),
				xcm::v4::Junction::GeneralIndex(1984),
			],
		),
		// Global consensus location
		xcm::v4::Location::new(
			1,
			[xcm::v4::Junction::GlobalConsensus(xcm::v4::NetworkId::Polkadot)],
		),
	];

	for v4_location in test_locations_v4 {
		// Test V4 -> V5 conversion
		let v5_location = xcm::v5::Location::try_from(v4_location.clone())
			.map_err(|_| TryRuntimeError::Other("Failed to convert V4 location to V5"))
			.unwrap();

		// Test that we can encode/decode V5 location
		let encoded = v5_location.encode();
		let decoded = xcm::v5::Location::decode(&mut &encoded[..])
			.map_err(|_| TryRuntimeError::Other("Failed to decode V5 location"))
			.unwrap();

		assert_eq!(v5_location, decoded, "V5 location encode/decode round-trip failed");

		// Test V4 encoded -> V5 decoded compatibility
		let encoded_v4 = v4_location.encode();
		let decoded_v5 = xcm::v5::Location::decode(&mut &encoded_v4[..])
			.map_err(|_| TryRuntimeError::Other("Failed to decode V4 encoded location as V5"))
			.unwrap();

		// try-from is compatible
		assert_eq!(
			decoded_v5, v5_location,
			"V4 encoded -> V5 decoded should match try_from conversion"
		);

		// encode/decode is compatible
		assert_eq!(encoded_v4, decoded_v5.encode(), "V4 encoded should match V5 re-encoded");
	}
}

#[test]
fn authorized_aliases_work() {
	ExtBuilder::<Runtime>::default()
		.with_tracing()
		.with_collators(vec![AccountId::from(ALICE)])
		.with_session_keys(vec![(
			AccountId::from(ALICE),
			AccountId::from(ALICE),
			SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(ALICE)) },
		)])
		.build()
		.execute_with(|| {
			use frame_support::traits::fungible::Mutate;
			let alice: AccountId = ALICE.into();
			let local_alice = Location::new(0, AccountId32 { network: None, id: ALICE });
			let alice_on_sibling_para =
				Location::new(1, [Parachain(42), AccountId32 { network: None, id: ALICE }]);
			let alice_on_relay = Location::new(1, AccountId32 { network: None, id: ALICE });
			let bob_on_relay = Location::new(1, AccountId32 { network: None, id: [42_u8; 32] });

			assert_ok!(Balances::mint_into(&alice, 2 * UNITS));

			// neither `alice_on_sibling_para`, `alice_on_relay`, `bob_on_relay` are allowed to
			// alias into `local_alice`
			for aliaser in [&alice_on_sibling_para, &alice_on_relay, &bob_on_relay] {
				assert!(!<XcmConfig as xcm_executor::Config>::Aliasers::contains(
					aliaser,
					&local_alice
				));
			}

			// Alice explicitly authorizes `alice_on_sibling_para` to alias her local account
			assert_ok!(PolkadotXcm::add_authorized_alias(
				RuntimeHelper::origin_of(alice.clone()),
				Box::new(alice_on_sibling_para.clone().into()),
				None
			));

			// `alice_on_sibling_para` now explicitly allowed to alias into `local_alice`
			assert!(<XcmConfig as xcm_executor::Config>::Aliasers::contains(
				&alice_on_sibling_para,
				&local_alice
			));
			// as expected, `alice_on_relay` and `bob_on_relay` still can't alias into `local_alice`
			for aliaser in [&alice_on_relay, &bob_on_relay] {
				assert!(!<XcmConfig as xcm_executor::Config>::Aliasers::contains(
					aliaser,
					&local_alice
				));
			}

			// Alice explicitly authorizes `alice_on_relay` to alias her local account
			assert_ok!(PolkadotXcm::add_authorized_alias(
				RuntimeHelper::origin_of(alice.clone()),
				Box::new(alice_on_relay.clone().into()),
				None
			));
			// Now both `alice_on_relay` and `alice_on_sibling_para` can alias into her local
			// account
			for aliaser in [&alice_on_relay, &alice_on_sibling_para] {
				assert!(<XcmConfig as xcm_executor::Config>::Aliasers::contains(
					aliaser,
					&local_alice
				));
			}

			// Alice removes authorization for `alice_on_relay` to alias her local account
			assert_ok!(PolkadotXcm::remove_authorized_alias(
				RuntimeHelper::origin_of(alice.clone()),
				Box::new(alice_on_relay.clone().into())
			));

			// `alice_on_relay` no longer allowed to alias into `local_alice`
			assert!(!<XcmConfig as xcm_executor::Config>::Aliasers::contains(
				&alice_on_relay,
				&local_alice
			));

			// `alice_on_sibling_para` still allowed to alias into `local_alice`
			assert!(<XcmConfig as xcm_executor::Config>::Aliasers::contains(
				&alice_on_sibling_para,
				&local_alice
			));
		})
}

#[test]
fn governance_authorize_upgrade_works() {
	use paseo_runtime_constants::system_parachain::{ASSET_HUB_ID, COLLECTIVES_ID};

	// no - random non-system para
	assert_err!(
		parachains_runtimes_test_utils::test_cases::can_governance_authorize_upgrade::<
			Runtime,
			RuntimeOrigin,
		>(GovernanceOrigin::Location(Location::new(1, Parachain(12334)))),
		Either::Right(InstructionError { index: 0, error: XcmError::Barrier })
	);
	// no - random system para
	assert_err!(
		parachains_runtimes_test_utils::test_cases::can_governance_authorize_upgrade::<
			Runtime,
			RuntimeOrigin,
		>(GovernanceOrigin::Location(Location::new(1, Parachain(1765)))),
		Either::Right(InstructionError { index: 1, error: XcmError::BadOrigin })
	);
	// no - AssetHub (this runtime is itself AH-next, governance is local rather than XCM-delivered)
	assert_err!(
		parachains_runtimes_test_utils::test_cases::can_governance_authorize_upgrade::<
			Runtime,
			RuntimeOrigin,
		>(GovernanceOrigin::Location(Location::new(1, Parachain(ASSET_HUB_ID)))),
		Either::Right(InstructionError { index: 1, error: XcmError::BadOrigin })
	);
	// no - Collectives
	assert_err!(
		parachains_runtimes_test_utils::test_cases::can_governance_authorize_upgrade::<
			Runtime,
			RuntimeOrigin,
		>(GovernanceOrigin::Location(Location::new(1, Parachain(COLLECTIVES_ID)))),
		Either::Right(InstructionError { index: 1, error: XcmError::BadOrigin })
	);
	// no - Collectives Voice of Fellows plurality
	assert_err!(
		parachains_runtimes_test_utils::test_cases::can_governance_authorize_upgrade::<
			Runtime,
			RuntimeOrigin,
		>(GovernanceOrigin::LocationAndDescendOrigin(
			Location::new(1, Parachain(COLLECTIVES_ID)),
			Plurality { id: BodyId::Technical, part: BodyPart::Voice }.into()
		)),
		Either::Right(InstructionError { index: 2, error: XcmError::BadOrigin })
	);

	// yes - relaychain. `ParentAsSuperuser` is required so the relay's staking-async messages
	// (`Transact { origin_kind: Superuser, .. }` session reports/offences from
	// `pallet-staking-async-ah-client`) dispatch as root; origin converters cannot filter by
	// call, so this also re-enables relay-authorized upgrades.
	// Revisit if the staking transport moves off the Superuser origin.
	assert_ok!(parachains_runtimes_test_utils::test_cases::can_governance_authorize_upgrade::<
		Runtime,
		RuntimeOrigin,
	>(GovernanceOrigin::Location(RelayChainLocation::get())));
}

mod dap {
	use codec::Encode;
	use frame_support::{
		assert_ok,
		traits::{
			fungible::{Inspect, Mutate},
			OnIdle, SignedTransactionBuilder,
		},
		weights::Weight,
	};
	use next_asset_hub_paseo_runtime::{
		AllPalletsWithoutSystem, Balances, EthExtraImpl, Executive, ExistentialDeposit, Runtime,
		RuntimeCall, RuntimeOrigin, SessionKeys, System, TxExtension, UncheckedExtrinsic,
	};
	use pallet_revive::evm::runtime::EthExtra;
	use parachains_common::{AccountId, AuraId};
	use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
	use sp_keyring::Sr25519Keyring;
	use sp_runtime::MultiSignature;

	use asset_test_utils::ExtBuilder;

	use super::ALICE;

	const BOB: [u8; 32] = [2u8; 32];

	fn construct_extrinsic(sender: Sr25519Keyring, call: RuntimeCall) -> UncheckedExtrinsic {
		let account_id = AccountId::from(sender.public());
		let nonce = frame_system::Pallet::<Runtime>::account(&account_id).nonce;
		// `EthExtra::get_eth_extension` returns the same Substrate-side `TxExtension` we sign over,
		// constructed with the right defaults for nonce/tip and skipping the PGAS branch.
		let tx_ext: TxExtension = EthExtraImpl::get_eth_extension(nonce, 0);
		let payload =
			sp_runtime::generic::SignedPayload::new(call.clone(), tx_ext.clone()).unwrap();
		let signature = payload.using_encoded(|e| sender.sign(e));
		UncheckedExtrinsic::new_signed_transaction(
			call,
			account_id.into(),
			MultiSignature::Sr25519(signature),
			tx_ext,
		)
	}

	#[test]
	fn tx_fees_go_to_dap_buffer() {
		let alice = AccountId::from(Sr25519Keyring::Alice);
		let buffer = pallet_dap::Pallet::<Runtime>::buffer_account();
		let staging = pallet_dap::Pallet::<Runtime>::staging_account();
		let ed = ExistentialDeposit::get();

		// `OnUnbalanced` deposits land in the DAP staging account first; `on_idle` later drains
		// the surplus above ED into the buffer. Pre-fund staging with ED so the drain
		// (`Preservation::Preserve`) can transfer the full fee.
		ExtBuilder::<Runtime>::default()
			.with_collators(vec![alice.clone()])
			.with_session_keys(vec![(
				alice.clone(),
				alice.clone(),
				SessionKeys { aura: AuraId::from(Sr25519Keyring::Alice.public()) },
			)])
			.with_balances(vec![
				(alice.clone(), 100 * ed),
				(buffer.clone(), ed),
				(staging.clone(), ed),
			])
			.with_para_id(ASSET_HUB_ID.into())
			.build()
			.execute_with(|| {
				let alice_before = <Balances as Inspect<AccountId>>::balance(&alice);
				let buffer_before = <Balances as Inspect<AccountId>>::balance(&buffer);
				let issuance_before = <Balances as Inspect<AccountId>>::total_issuance();

				let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
				let xt = construct_extrinsic(Sr25519Keyring::Alice, call);
				assert_ok!(Executive::apply_extrinsic(xt).unwrap());

				let alice_after = <Balances as Inspect<AccountId>>::balance(&alice);
				let fee_paid = alice_before - alice_after;
				assert!(fee_paid > 0, "a fee should have been paid");

				<AllPalletsWithoutSystem as OnIdle<_>>::on_idle(
					System::block_number(),
					Weight::MAX,
				);

				let buffer_after = <Balances as Inspect<AccountId>>::balance(&buffer);
				let issuance_after = <Balances as Inspect<AccountId>>::total_issuance();

				assert_eq!(buffer_after, buffer_before + fee_paid);
				assert_eq!(issuance_before, issuance_after);
			});
	}

	#[test]
	fn dust_removal_goes_to_dap_buffer() {
		let alice = AccountId::from(ALICE);
		let bob = AccountId::from(BOB);
		let buffer = pallet_dap::Pallet::<Runtime>::buffer_account();
		let staging = pallet_dap::Pallet::<Runtime>::staging_account();
		let ed = ExistentialDeposit::get();
		let dust = ed / 2;

		ExtBuilder::<Runtime>::default()
			.with_collators(vec![AccountId::from(ALICE)])
			.with_session_keys(vec![(
				AccountId::from(ALICE),
				AccountId::from(ALICE),
				SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(ALICE)) },
			)])
			.build()
			.execute_with(|| {
				assert_ok!(<Balances as Mutate<AccountId>>::mint_into(&bob, ed + dust));
				assert_ok!(<Balances as Mutate<AccountId>>::mint_into(&alice, 100 * ed));
				assert_ok!(<Balances as Mutate<AccountId>>::mint_into(&buffer, ed));
				// pallet-dap routes `OnUnbalanced` deposits through staging first; pre-fund it
				// with ED so resolving sub-ED dust into it succeeds under `Preserve`.
				assert_ok!(<Balances as Mutate<AccountId>>::mint_into(&staging, ed));

				let buffer_before = <Balances as Inspect<AccountId>>::balance(&buffer);

				// Transfer ED away from bob, leaving dust < ED → account reaped.
				assert_ok!(Balances::transfer_allow_death(
					RuntimeOrigin::signed(bob.clone()),
					alice.clone().into(),
					ed,
				));

				<AllPalletsWithoutSystem as OnIdle<_>>::on_idle(
					System::block_number(),
					Weight::MAX,
				);

				let buffer_after = <Balances as Inspect<AccountId>>::balance(&buffer);
				assert_eq!(buffer_after, buffer_before + dust);
				assert_eq!(<Balances as Inspect<AccountId>>::balance(&bob), 0);
			});
	}
}

mod pgas_fees {
	use codec::Encode;
	use frame_support::{
		assert_ok,
		dispatch::GetDispatchInfo,
		traits::{
			fungible::Inspect as FungibleInspect,
			fungibles::{Inspect as FungiblesInspect, Mutate as FungiblesMutate},
			SignedTransactionBuilder,
		},
	};
	use next_asset_hub_paseo_runtime::{
		Assets, Balances, Executive, ExistentialDeposit, NftClaims, PgasAssetId, PgasMinBalance,
		Runtime, RuntimeCall, RuntimeEvent, RuntimeOrigin, Scarcity, SessionKeys, System,
		TxExtension, UncheckedExtrinsic,
	};
	use parachains_common::{AccountId, AuraId};
	use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
	use sp_keyring::Sr25519Keyring;
	use sp_runtime::{
		generic,
		transaction_validity::{InvalidTransaction, TransactionValidityError},
		MultiSignature,
	};

	use asset_test_utils::ExtBuilder;

	use super::ALICE;

	/// Builds a signed extrinsic whose `ChargePGAS` has the PGAS path enabled. The `dap` module's
	/// helper cannot be reused: it goes through `EthExtraImpl::get_eth_extension`, which
	/// constructs `ChargePGAS` with `new_skip_pgas`.
	fn construct_extrinsic(sender: Sr25519Keyring, call: RuntimeCall) -> UncheckedExtrinsic {
		let account_id = AccountId::from(sender.public());
		let nonce = frame_system::Pallet::<Runtime>::account(&account_id).nonce;
		let tx_ext = TxExtension::from((
			(
				(),
				pallet_scarcity::extension::AsScarcity::<Runtime>::new(None),
				frame_system::AuthorizeCall::<Runtime>::new(),
				indiv_pallet_pgas::AsPgas::<Runtime>::new(None),
				indiv_pallet_dotns_gateway::AsDotnsGateway::<Runtime>::new(None),
			),
			indiv_pallet_origin_restriction::RestrictOrigin::<Runtime>::new(true),
			frame_system::CheckNonZeroSender::<Runtime>::new(),
			frame_system::CheckSpecVersion::<Runtime>::new(),
			frame_system::CheckTxVersion::<Runtime>::new(),
			frame_system::CheckGenesis::<Runtime>::new(),
			frame_system::CheckEra::<Runtime>::from(generic::Era::Immortal),
			frame_system::CheckNonce::<Runtime>::from(nonce),
			frame_system::CheckWeight::<Runtime>::new(),
			pallet_pgas_allowance::ChargePGAS::<
				Runtime,
				pallet_asset_conversion_tx_payment::ChargeAssetTxPayment<Runtime>,
			>::from(pallet_asset_conversion_tx_payment::ChargeAssetTxPayment::<Runtime>::from(
				0, None,
			)),
			frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
			pallet_revive::evm::tx_extension::SetOrigin::<Runtime>::default(),
		));
		let payload = generic::SignedPayload::new(call.clone(), tx_ext.clone()).unwrap();
		let signature = payload.using_encoded(|e| sender.sign(e));
		UncheckedExtrinsic::new_signed_transaction(
			call,
			account_id.into(),
			MultiSignature::Sr25519(signature),
			tx_ext,
		)
	}

	#[test]
	fn pgas_pays_the_fee_of_a_non_revive_call() {
		let alice = AccountId::from(ALICE);
		let bob = AccountId::from(Sr25519Keyring::Bob.public());
		let charlie = AccountId::from(Sr25519Keyring::Charlie.public());

		ExtBuilder::<Runtime>::default()
			.with_collators(vec![alice.clone()])
			.with_session_keys(vec![(
				alice.clone(),
				alice.clone(),
				SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(ALICE)) },
			)])
			.with_para_id(ASSET_HUB_ID.into())
			.build()
			.execute_with(|| {
				assert_ok!(indiv_pallet_pgas::Pallet::<Runtime>::do_create_pgas_asset());
				let pgas = PgasAssetId::get();
				let endowment = 100 * ExistentialDeposit::get();
				assert_ok!(<Assets as FungiblesMutate<AccountId>>::mint_into(
					pgas, &bob, endowment
				));

				let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
				let xt_bob = construct_extrinsic(Sr25519Keyring::Bob, call.clone());
				let xt_charlie = construct_extrinsic(Sr25519Keyring::Charlie, call);

				assert_eq!(<Balances as FungibleInspect<AccountId>>::balance(&bob), 0);
				assert_eq!(<Balances as FungibleInspect<AccountId>>::balance(&charlie), 0);
				assert!(!frame_system::Pallet::<Runtime>::account_exists(&charlie));

				// Endow Charlie one planck short of the fee. The balance keeps his account alive
				let fee = pallet_transaction_payment::Pallet::<Runtime>::compute_fee(
					xt_charlie.encoded_size() as u32,
					&xt_charlie.get_dispatch_info(),
					0,
				);
				let charlie_endowment = fee - 1;
				assert!(
					charlie_endowment >= PgasMinBalance::get(),
					"the PGAS endowment must be holdable yet insufficient for the fee"
				);
				assert_ok!(<Assets as FungiblesMutate<AccountId>>::mint_into(
					pgas,
					&charlie,
					charlie_endowment
				));
				assert!(frame_system::Pallet::<Runtime>::account_exists(&charlie));

				assert_ok!(Executive::apply_extrinsic(xt_bob).unwrap());

				let paid = endowment - <Assets as FungiblesInspect<AccountId>>::balance(pgas, &bob);
				assert!(paid > 0, "the fee should have been taken in PGAS");
				assert_eq!(<Balances as FungibleInspect<AccountId>>::balance(&bob), 0);
				assert!(
					System::events().iter().any(|record| matches!(
						record.event,
						RuntimeEvent::PgasAllowance(
							pallet_pgas_allowance::Event::PGASFeePaid { actual_fee, .. }
						) if actual_fee == paid
					)),
					"a PGASFeePaid event should report the fee burned"
				);

				assert_eq!(
					Executive::apply_extrinsic(xt_charlie),
					Err(TransactionValidityError::Invalid(InvalidTransaction::Payment))
				);
				assert_eq!(
					<Assets as FungiblesInspect<AccountId>>::balance(pgas, &charlie),
					charlie_endowment,
					"a rejected transaction does not touch the signer's PGAS"
				);
			});
	}

	/// A person's NFT claim is paid for out of the PGAS they mint daily, holding no native
	/// balance at all.
	#[test]
	fn pgas_pays_the_fee_of_a_persons_nft_claim() {
		let alice = AccountId::from(ALICE);
		let bob = AccountId::from(Sr25519Keyring::Bob.public());
		let owner = AccountId::from([254u8; 32]);
		let mint_to = AccountId::from([2u8; 32]);
		let alias = [9u8; 32];
		let credit = [7u8; 32];

		ExtBuilder::<Runtime>::default()
			.with_collators(vec![alice.clone()])
			.with_session_keys(vec![(
				alice.clone(),
				alice.clone(),
				SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(ALICE)) },
			)])
			// The collection owner pays Scarcity's deposits; Bob is deliberately left with none.
			.with_balances(vec![(owner.clone(), 1_000 * ExistentialDeposit::get())])
			.with_para_id(ASSET_HUB_ID.into())
			.build()
			.execute_with(|| {
				assert_ok!(indiv_pallet_pgas::Pallet::<Runtime>::do_create_pgas_asset());
				let pgas = PgasAssetId::get();
				let endowment = 100 * ExistentialDeposit::get();
				assert_ok!(<Assets as FungiblesMutate<AccountId>>::mint_into(
					pgas, &bob, endowment
				));

				// The collection and item a claim mints into, set up and registered for claims
				// by their owner beforehand.
				let collection = 0;
				assert_ok!(Scarcity::do_create_collection(owner.clone()));
				assert_ok!(Scarcity::do_define_item(
					owner.clone(),
					collection,
					pallet_scarcity::Transferability::Transferable,
					Vec::new()
				));
				assert_ok!(NftClaims::set_collection_minter(
					RuntimeOrigin::signed(owner),
					collection,
					Some(indiv_pallet_nft_claims::ItemSelection::Random)
				));

				// Bob's account is bound to the alias the game chain awarded the credit to.
				indiv_pallet_alias_accounts::AccountToAlias::<Runtime>::insert(
					&bob,
					indiv_pallet_alias_accounts::AliasAccountInfo {
						collection: *indiv_pallet_alias_accounts::PEOPLE_IDENTIFIER,
						revision: 0,
						ring: 0,
						ca: indiv_support::traits::ContextualAlias { context: [0u8; 32], alias },
					},
				);

				// A one-leaf tree's root is that leaf's hash, over the alias rather than Bob.
				let leaf = indiv_support::credit_trees::credit_leaf(
					&indiv_support::identity::AccountOrPerson::<AccountId>::Person(alias),
					&credit,
				);
				indiv_pallet_nft_claims::CreditTrees::<Runtime>::insert(
					1u32,
					indiv_support::credit_trees::NftClaimCreditTree {
						game_index: 0,
						root: indiv_support::credit_trees::CreditProofNode(
							sp_io::hashing::blake2_256(&leaf.encode()),
						),
						leaf_count: 1,
						timestamp: 0,
					},
				);

				let call = RuntimeCall::NftClaims(indiv_pallet_nft_claims::Call::claim {
					claimant: indiv_pallet_nft_claims::ClaimantKind::Person,
					block: 1,
					credit,
					leaf_index: 0,
					proof: Default::default(),
					collection,
					mint_to: mint_to.clone(),
				});
				let xt = construct_extrinsic(Sr25519Keyring::Bob, call);

				assert_ok!(Executive::apply_extrinsic(xt).unwrap());

				assert!(indiv_pallet_nft_claims::ClaimedCredits::<Runtime>::contains_key(
					1u32, leaf
				));
				assert!(pallet_scarcity::NftsByOwner::<Runtime>::contains_key(&mint_to));

				let paid = endowment - <Assets as FungiblesInspect<AccountId>>::balance(pgas, &bob);
				assert!(paid > 0, "the claim's fee should have been taken in PGAS");
				assert_eq!(
					<Balances as FungibleInspect<AccountId>>::balance(&bob),
					0,
					"the claimant holds no native balance, so nothing else can have paid"
				);
				assert!(
					System::events().iter().any(|record| matches!(
						record.event,
						RuntimeEvent::PgasAllowance(
							pallet_pgas_allowance::Event::PGASFeePaid { actual_fee, .. }
						) if actual_fee == paid
					)),
					"a PGASFeePaid event should report the fee burned"
				);
			});
	}

	/// A signer whose PGAS balance does not cover the fee pays it in the native asset instead.
	#[test]
	fn dot_pays_the_fee_when_pgas_is_insufficient() {
		let alice = AccountId::from(ALICE);
		let bob = AccountId::from(Sr25519Keyring::Bob.public());
		let endowment = 100 * ExistentialDeposit::get();

		ExtBuilder::<Runtime>::default()
			.with_collators(vec![alice.clone()])
			.with_session_keys(vec![(
				alice.clone(),
				alice.clone(),
				SessionKeys { aura: AuraId::from(sp_core::sr25519::Public::from_raw(ALICE)) },
			)])
			.with_balances(vec![
				(bob.clone(), endowment),
				(pallet_dap::Pallet::<Runtime>::staging_account(), ExistentialDeposit::get()),
			])
			.with_para_id(ASSET_HUB_ID.into())
			.build()
			.execute_with(|| {
				assert_ok!(indiv_pallet_pgas::Pallet::<Runtime>::do_create_pgas_asset());
				let pgas = PgasAssetId::get();

				let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![] });
				let xt = construct_extrinsic(Sr25519Keyring::Bob, call);

				let info = xt.get_dispatch_info();
				let fee = pallet_transaction_payment::Pallet::<Runtime>::compute_fee(
					xt.encoded_size() as u32,
					&info,
					0,
				);
				let pgas_endowment = fee - 1;
				assert!(
					pgas_endowment >= PgasMinBalance::get(),
					"the PGAS endowment must be holdable yet insufficient for the fee"
				);
				assert_ok!(<Assets as FungiblesMutate<AccountId>>::mint_into(
					pgas,
					&bob,
					pgas_endowment
				));

				assert_ok!(Executive::apply_extrinsic(xt).unwrap());

				let paid = endowment - <Balances as FungibleInspect<AccountId>>::balance(&bob);
				assert!(paid > 0, "the fee should have been taken from the native balance");
				assert_eq!(
					<Assets as FungiblesInspect<AccountId>>::balance(pgas, &bob),
					pgas_endowment,
					"the insufficient PGAS balance should be left untouched"
				);
				assert!(
					System::events().iter().any(|record| matches!(
						record.event,
						RuntimeEvent::TransactionPayment(
							pallet_transaction_payment::Event::TransactionFeePaid {
								actual_fee, ..
							}
						) if actual_fee == paid
					)),
					"a TransactionFeePaid event should report the native fee"
				);
				assert!(
					!System::events().iter().any(|record| matches!(
						record.event,
						RuntimeEvent::PgasAllowance(
							pallet_pgas_allowance::Event::PGASFeePaid { .. }
						)
					)),
					"no fee should have been taken in PGAS"
				);
			});
	}
}

mod external_asset_teleport {
	use super::*;
	use paseo_runtime_constants::system_parachain::PEOPLE_ID;

	type IsTeleporter = <XcmConfig as xcm_executor::Config>::IsTeleporter;

	fn people_origin() -> Location {
		Location::new(1, [Parachain(PEOPLE_ID)])
	}

	fn external_asset(amount: u128) -> Asset {
		(ExternalAssetLocation::get(), amount).into()
	}

	#[test]
	fn external_asset_teleport_from_people_is_accepted() {
		assert!(IsTeleporter::contains(&external_asset(1_000), &people_origin()));
	}

	#[test]
	fn external_asset_teleport_from_relay_is_rejected() {
		assert!(!IsTeleporter::contains(&external_asset(1_000), &Location::parent()));
	}

	#[test]
	fn external_asset_teleport_from_random_sibling_is_rejected() {
		let random_sibling = Location::new(1, [Parachain(4242)]);
		assert!(!IsTeleporter::contains(&external_asset(1_000), &random_sibling));
	}

	#[test]
	fn wrong_asset_teleport_from_people_is_rejected() {
		// Different GeneralIndex (not the external asset).
		let wrong_asset: Asset = (
			Location::new(0, [PalletInstance(50), GeneralIndex((EXTERNAL_ASSET_ID + 1) as u128)]),
			1_000u128,
		)
			.into();
		assert!(!IsTeleporter::contains(&wrong_asset, &people_origin()));
	}

	#[test]
	fn wrong_pallet_index_teleport_from_people_is_rejected() {
		// Right asset id, wrong pallet instance.
		let wrong_asset: Asset = (
			Location::new(0, [PalletInstance(99), GeneralIndex(EXTERNAL_ASSET_ID as u128)]),
			1_000u128,
		)
			.into();
		assert!(!IsTeleporter::contains(&wrong_asset, &people_origin()));
	}

	#[test]
	fn dot_teleport_from_relay_still_works() {
		// Regression: pre-existing native-asset teleport rules unaffected.
		let dot: Asset = (DotLocation::get(), 1_000u128).into();
		assert!(IsTeleporter::contains(&dot, &Location::parent()));
	}
}

/// The transaction pipeline is what decides whether a call can reach dispatch unpaid, so its shape
/// is an invariant of this chain, not an implementation detail.
mod tx_extension_pipeline {
	use next_asset_hub_paseo_runtime::{RuntimeCall, TxExtension};
	use sp_runtime::traits::TransactionExtension;

	/// Every extension of the pipeline, in the order it runs.
	///
	/// The four ahead of `RestrictOrigins` are the ones that replace the origin, and everything
	/// that charges the transaction runs after it. An extension that installs an origin the
	/// payment extensions do not charge therefore needs an allowance in
	/// `pallet-origin-restriction` to bound it, which is why an addition anywhere in this list is
	/// a deliberate change rather than an implementation detail.
	const PIPELINE: [&str; 17] = [
		"UnitTransactionExtension",
		"AsScarcity",
		"AuthorizeCall",
		"AsPgas",
		"AsDotnsGateway",
		"RestrictOrigins",
		"CheckNonZeroSender",
		"CheckSpecVersion",
		"CheckTxVersion",
		"CheckGenesis",
		"CheckMortality",
		"CheckNonce",
		"CheckWeight",
		"ChargeAssetTxPayment",
		"CheckMetadataHash",
		"EthSetOrigin",
		"StorageWeightReclaim",
	];

	#[test]
	fn the_pipeline_is_the_expected_one() {
		let identifiers = <TxExtension as TransactionExtension<RuntimeCall>>::metadata()
			.into_iter()
			.map(|meta| meta.identifier)
			.collect::<Vec<_>>();

		assert_eq!(identifiers, PIPELINE);
	}
}

/// `EnsureCreditClaimant` is the only path from a transaction to a claimant identity, and
/// `ClaimantKind` is the only thing that selects between the two: no extension installs an alias
/// origin, so a claim resolves the person from the signer's binding.
mod credit_claimant_origin {
	use frame_support::traits::{EnsureOriginWithArg, Get};
	use indiv_pallet_alias_accounts::{AccountToAlias, AliasAccountInfo, PEOPLE_IDENTIFIER};
	use indiv_pallet_members_subscriber::{types::RingCommitmentRecord, RingRoots};
	use indiv_pallet_nft_claims::ClaimantKind;
	use indiv_support::{
		crypto::BandersnatchVrfVerifiable,
		identity::AccountOrPerson,
		traits::{Context, ContextualAlias, PersonhoodLookup},
	};
	use next_asset_hub_paseo_runtime::{
		AliasAccounts, EnsureCreditClaimant, Runtime, RuntimeOrigin,
	};
	use parachains_common::AccountId;
	use sp_runtime::BoundedVec;
	use verifiable::{ring::RingDomainSize, GenerateVerifiable};

	const ALIAS: [u8; 32] = [9u8; 32];
	const CONTEXT: Context = [3u8; 32];
	/// Time the seeded ring roots are committed at, in seconds.
	const SOURCE_TIME: u64 = 1_000_000;

	/// Window `personhood_info` keeps accepting a superseded revision for.
	fn grace() -> u64 {
		<<Runtime as indiv_pallet_alias_accounts::Config>::CleanupGracePeriod as Get<u64>>::get()
	}

	fn signer() -> AccountId {
		AccountId::from([8u8; 32])
	}

	/// Records `revisions` roots for ring 0 of the people collection, numbered from 0 and all
	/// committed at [`SOURCE_TIME`]. The grace policy reads only the revision numbers and their
	/// source times, so an empty ring commitment stands in for the real root.
	fn seed_ring(revisions: u32) {
		let root = BandersnatchVrfVerifiable::finish_members(
			BandersnatchVrfVerifiable::start_members(RingDomainSize::Domain11),
		);
		let roots = (0..revisions)
			.map(|revision| RingCommitmentRecord {
				root: root.clone(),
				revision,
				source_time: SOURCE_TIME,
				source_sequence: 1,
			})
			.collect::<Vec<_>>();
		RingRoots::<Runtime>::insert(
			*PEOPLE_IDENTIFIER,
			0,
			BoundedVec::try_from(roots).expect("revisions within MaxRecentRootsPerRing"),
		);
	}

	/// Moves the clock the grace policy reads to `SOURCE_TIME + offset` seconds. Writes `Now`
	/// directly, since `set_timestamp` runs Aura's `OnTimestampSet` hook, which requires the slot
	/// to match.
	fn set_now(offset: u64) {
		pallet_timestamp::Now::<Runtime>::put(
			SOURCE_TIME.saturating_add(offset).saturating_mul(1_000),
		);
	}

	/// The alias `personhood_info` resolves for `signer()` in [`CONTEXT`].
	fn personhood_alias() -> Option<[u8; 32]> {
		<AliasAccounts as PersonhoodLookup<AccountId, _>>::personhood_info(&signer(), &CONTEXT)
			.0
			.map(|(_collection, alias)| alias)
	}

	/// Binds `signer()` to [`ALIAS`], as `set_alias_account` does for a person who proved a ring
	/// membership.
	fn bind_alias() {
		AccountToAlias::<Runtime>::insert(
			signer(),
			AliasAccountInfo {
				collection: *PEOPLE_IDENTIFIER,
				ring: 0,
				revision: 0,
				ca: ContextualAlias { alias: ALIAS, context: CONTEXT },
			},
		);
	}

	/// `try_origin`'s success value, dropping the origin it hands back on failure so this does not
	/// depend on `RuntimeOrigin` being printable.
	fn claimant(origin: RuntimeOrigin, kind: ClaimantKind) -> Option<AccountOrPerson<AccountId>> {
		EnsureCreditClaimant::try_origin(origin, &kind).ok()
	}

	#[test]
	fn a_signer_claims_what_was_awarded_to_its_account() {
		sp_io::TestExternalities::default().execute_with(|| {
			let origin = RuntimeOrigin::signed(signer());
			assert_eq!(
				claimant(origin, ClaimantKind::Account),
				Some(AccountOrPerson::Account(signer()))
			);
		});
	}

	/// Revision 0 is the ring's latest, so the binding is one both lookups accept. This is the
	/// baseline [`a_stale_binding_still_resolves_to_its_person`] moves away from.
	#[test]
	fn a_signer_claims_as_the_person_its_account_is_bound_to() {
		sp_io::TestExternalities::default().execute_with(|| {
			bind_alias();
			seed_ring(1);
			set_now(0);
			assert_eq!(personhood_alias(), Some(ALIAS));

			let origin = RuntimeOrigin::signed(signer());
			assert_eq!(
				claimant(origin, ClaimantKind::Person),
				Some(AccountOrPerson::Person(ALIAS))
			);
		});
	}

	/// Claiming as a person is what the binding authorizes, so an account without one is rejected
	/// rather than falling back to claiming as itself.
	#[test]
	fn an_unbound_signer_cannot_claim_as_a_person() {
		sp_io::TestExternalities::default().execute_with(|| {
			let origin = RuntimeOrigin::signed(signer());
			assert_eq!(claimant(origin, ClaimantKind::Person), None);
		});
	}

	/// Revision 1 supersedes the binding's revision 0 and the grace period has passed, so
	/// `personhood_info` refuses the binding. The credit is awarded to the alias before the claim,
	/// so the claim still resolves the same person.
	#[test]
	fn a_stale_binding_still_resolves_to_its_person() {
		sp_io::TestExternalities::default().execute_with(|| {
			bind_alias();
			seed_ring(2);
			set_now(grace() + 1);
			assert_eq!(personhood_alias(), None);

			let origin = RuntimeOrigin::signed(signer());
			assert_eq!(
				claimant(origin, ClaimantKind::Person),
				Some(AccountOrPerson::Person(ALIAS))
			);
		});
	}

	#[test]
	fn an_unsigned_origin_cannot_claim() {
		sp_io::TestExternalities::default().execute_with(|| {
			bind_alias();
			assert_eq!(claimant(RuntimeOrigin::root(), ClaimantKind::Person), None);
			assert_eq!(claimant(RuntimeOrigin::none(), ClaimantKind::Account), None);
		});
	}
}
