// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0

use crate::{
	people::ExternalAssetLocation,
	xcm_config::{AssetTransactors, RelayLocation, WeightToNativeFee, XcmConfig},
	AccountId, AssetRate, Assets, Balance, Balances, Block, Runtime, RuntimeGenesisConfig,
	RuntimeOrigin, EXISTENTIAL_DEPOSIT,
};
use frame_support::{
	assert_ok,
	traits::{fungible, fungibles, TypedGet},
	weights::WeightToFee,
};
use pallet_collator_selection::StakingPotAccountId;
use sp_io::TestExternalities;
use sp_keyring::Sr25519Keyring;
use sp_runtime::{BuildStorage, FixedU128};
use xcm::{latest::prelude::*, IntoVersion, VersionedAssetId};
use xcm_executor::{
	traits::{TransactAsset, WeightTrader},
	AssetsInHolding,
};
use xcm_runtime_apis::fees::{
	runtime_decl_for_xcm_payment_api::XcmPaymentApi, Error as PaymentError,
};

type Trader = <XcmConfig as xcm_executor::Config>::Trader;
const WEIGHT: Weight = Weight::from_parts(1_000_000_000, 0);

fn new_test_ext() -> TestExternalities {
	RuntimeGenesisConfig {
		balances: pallet_balances::GenesisConfig {
			balances: vec![(payer(), 1_000_000 * EXISTENTIAL_DEPOSIT)],
			..Default::default()
		},
		..Default::default()
	}
	.build_storage()
	.unwrap()
	.into()
}

fn payer() -> AccountId {
	Sr25519Keyring::Alice.to_account_id()
}

fn payer_location() -> Location {
	AccountId32 { network: None, id: payer().into() }.into()
}

fn other_asset() -> Location {
	Location::new(1, [Parachain(4242), PalletInstance(50), GeneralIndex(7)])
}

fn create_asset(location: &Location, min_balance: Balance) {
	assert_ok!(Assets::force_create(
		RuntimeOrigin::root(),
		location.clone(),
		payer().into(),
		true,
		min_balance
	));
}

fn register_rate(location: &Location, rate: u32) {
	assert_ok!(AssetRate::create(
		RuntimeOrigin::root(),
		Box::new(location.clone()),
		FixedU128::from_u32(rate)
	));
}

fn mint_asset(location: &Location, amount: Balance) {
	assert_ok!(Assets::mint(
		RuntimeOrigin::signed(payer()),
		location.clone(),
		payer().into(),
		amount
	));
}

fn withdraw(location: &Location, amount: Balance) -> AssetsInHolding {
	AssetTransactors::withdraw_asset(&(location.clone(), amount).into(), &payer_location(), None)
		.unwrap()
}

fn deposit(holding: AssetsInHolding) {
	assert_ok!(AssetTransactors::deposit_asset(holding, &payer_location(), None));
}

fn quote(location: &Location, weight: Weight) -> Result<Balance, PaymentError> {
	<Runtime as XcmPaymentApi<Block>>::query_weight_to_asset_fee(
		weight,
		AssetId(location.clone()).into(),
	)
}

fn accepted_assets(version: u32) -> Vec<VersionedAssetId> {
	<Runtime as XcmPaymentApi<Block>>::query_acceptable_payment_assets(version).unwrap()
}

fn asset_balance(location: &Location, account: &AccountId) -> Balance {
	<Assets as fungibles::Inspect<AccountId>>::balance(location.clone(), account)
}

#[test]
fn rated_assets_pay_and_refund_in_kind() {
	new_test_ext().execute_with(|| {
		for (asset, rate) in [(ExternalAssetLocation::get(), 10_000), (other_asset(), 4)] {
			create_asset(&asset, 1);
			register_rate(&asset, rate);
			let expected = WeightToNativeFee::weight_to_fee(&WEIGHT) / u128::from(rate);
			assert!(expected > 2);
			assert_eq!(quote(&asset, WEIGHT), Ok(expected));
			let payment = expected * 3;
			mint_asset(&asset, payment);
			let issuance = <Assets as fungibles::Inspect<AccountId>>::total_issuance(asset.clone());
			let pot = StakingPotAccountId::<Runtime>::get();
			let native_before = Balances::free_balance(&pot);
			let context = XcmContext::with_message_id([0; 32]);
			let mut trader = Trader::new();
			let change = trader.buy_weight(WEIGHT, withdraw(&asset, payment), &context).unwrap();
			assert_eq!(
				change.fungible_assets_iter().collect::<Vec<_>>(),
				vec![(asset.clone(), payment - expected).into()]
			);
			deposit(change);
			let unused = WEIGHT / 2;
			let expected_refund = WeightToNativeFee::weight_to_fee(&unused) / u128::from(rate);
			let refund = trader.refund_weight(unused, &context).unwrap();
			assert_eq!(
				refund.fungible_assets_iter().collect::<Vec<_>>(),
				vec![(asset.clone(), expected_refund).into()]
			);
			deposit(refund);
			drop(trader);
			assert_eq!(asset_balance(&asset, &pot), expected - expected_refund);
			assert_eq!(asset_balance(&asset, &payer()), payment - expected + expected_refund);
			assert_eq!(Balances::free_balance(&pot), native_before);
			assert_eq!(<Assets as fungibles::Inspect<AccountId>>::total_issuance(asset), issuance);
		}
	});
}

#[test]
fn native_fees_still_reach_the_staking_pot() {
	new_test_ext().execute_with(|| {
		let asset = RelayLocation::get();
		let expected = WeightToNativeFee::weight_to_fee(&WEIGHT);
		assert_eq!(quote(&asset, WEIGHT), Ok(expected));
		let pot = StakingPotAccountId::<Runtime>::get();
		assert_ok!(<Balances as fungible::Mutate<AccountId>>::mint_into(&pot, EXISTENTIAL_DEPOSIT));
		let before = Balances::free_balance(&pot);
		let mut trader = Trader::new();
		let change = trader
			.buy_weight(
				WEIGHT,
				withdraw(&asset, expected * 2),
				&XcmContext::with_message_id([0; 32]),
			)
			.unwrap();
		deposit(change);
		drop(trader);
		assert_eq!(Balances::free_balance(pot), before + expected);
	});
}

#[test]
fn rate_updates_and_removal_change_quotes_and_acceptance() {
	new_test_ext().execute_with(|| {
		let asset = other_asset();
		create_asset(&asset, 1);
		register_rate(&asset, 2);
		let fee = quote(&asset, WEIGHT).unwrap();
		assert_ok!(AssetRate::update(
			RuntimeOrigin::root(),
			Box::new(asset.clone()),
			FixedU128::from_u32(4)
		));
		assert_eq!(quote(&asset, WEIGHT), Ok(fee / 2));
		assert_ok!(AssetRate::remove(RuntimeOrigin::root(), Box::new(asset.clone())));
		assert_eq!(quote(&asset, WEIGHT), Err(PaymentError::AssetNotFound));
		assert!(!accepted_assets(5).contains(&AssetId(asset.clone()).into()));
		mint_asset(&asset, fee);
		let mut trader = Trader::new();
		let (payment, _) = trader
			.buy_weight(WEIGHT, withdraw(&asset, fee), &XcmContext::with_message_id([0; 32]))
			.unwrap_err();
		deposit(payment);
		drop(trader);
		assert_eq!(asset_balance(&asset, &payer()), fee);
		assert_eq!(asset_balance(&asset, &StakingPotAccountId::<Runtime>::get()), 0);
	});
}

#[test]
fn insufficient_payment_is_returned_unchanged() {
	new_test_ext().execute_with(|| {
		let asset = other_asset();
		create_asset(&asset, 1);
		register_rate(&asset, 4);
		let payment = quote(&asset, WEIGHT).unwrap() - 1;
		mint_asset(&asset, payment);
		let mut trader = Trader::new();
		let (returned, error) = trader
			.buy_weight(WEIGHT, withdraw(&asset, payment), &XcmContext::with_message_id([0; 32]))
			.unwrap_err();
		assert_eq!(error, XcmError::TooExpensive);
		deposit(returned);
		drop(trader);
		assert_eq!(asset_balance(&asset, &payer()), payment);
		assert_eq!(asset_balance(&asset, &StakingPotAccountId::<Runtime>::get()), 0);
	});
}

#[test]
fn quotes_and_charges_respect_the_asset_minimum_balance() {
	new_test_ext().execute_with(|| {
		let asset = other_asset();
		let minimum = WeightToNativeFee::weight_to_fee(&WEIGHT) * 2;
		create_asset(&asset, minimum);
		register_rate(&asset, 1);
		assert_eq!(quote(&asset, WEIGHT), Ok(minimum));
		mint_asset(&asset, minimum * 2);
		let mut trader = Trader::new();
		let change = trader
			.buy_weight(
				WEIGHT,
				withdraw(&asset, minimum * 2),
				&XcmContext::with_message_id([0; 32]),
			)
			.unwrap();
		deposit(change);
		drop(trader);
		assert_eq!(asset_balance(&asset, &StakingPotAccountId::<Runtime>::get()), minimum);
		assert_eq!(asset_balance(&asset, &payer()), minimum);
	});
}

#[test]
fn fee_api_filters_rated_locations_and_preserves_xcm_versions() {
	new_test_ext().execute_with(|| {
		let asset = other_asset();
		create_asset(&asset, 1);
		register_rate(&asset, 4);
		register_rate(&RelayLocation::get(), 2);
		let excluded = [
			Location::here(),
			Location::new(0, PalletInstance(50)),
			Location::new(2, [GlobalConsensus(NetworkId::Polkadot), Parachain(4242)]),
		];
		for location in &excluded {
			register_rate(location, 1);
			assert_eq!(quote(location, WEIGHT), Err(PaymentError::AssetNotFound));
		}
		for version in [3, 4, 5] {
			let actual = accepted_assets(version);
			let expected = [RelayLocation::get(), asset.clone()].map(|location| {
				VersionedAssetId::from(AssetId(location)).into_version(version).unwrap()
			});
			assert_eq!(actual, expected);
			for id in actual {
				assert!(<Runtime as XcmPaymentApi<Block>>::query_weight_to_asset_fee(WEIGHT, id)
					.is_ok());
			}
		}
		assert!(accepted_assets(2).is_empty());
		assert!(accepted_assets(6).is_empty());
		assert_eq!(
			quote(&RelayLocation::get(), WEIGHT),
			Ok(WeightToNativeFee::weight_to_fee(&WEIGHT))
		);
	});
}

#[test]
fn quotes_do_not_change_asset_balances_or_issuance() {
	new_test_ext().execute_with(|| {
		let asset = other_asset();
		create_asset(&asset, 1);
		register_rate(&asset, 4);
		mint_asset(&asset, 1_000_000);
		let before = sp_io::storage::root(sp_runtime::StateVersion::V1);
		assert!(quote(&asset, WEIGHT).is_ok());
		assert!(quote(&RelayLocation::get(), WEIGHT).is_ok());
		assert_eq!(sp_io::storage::root(sp_runtime::StateVersion::V1), before);
	});
}

#[test]
fn non_sufficient_asset_needs_a_provider_for_the_staking_pot() {
	new_test_ext().execute_with(|| {
		let asset = other_asset();
		assert_ok!(Assets::force_create(
			RuntimeOrigin::root(),
			asset.clone(),
			payer().into(),
			false,
			1
		));
		register_rate(&asset, 4);
		let fee = quote(&asset, WEIGHT).unwrap();
		let pot = StakingPotAccountId::<Runtime>::get();
		mint_asset(&asset, fee * 3);
		let mut trader = Trader::new();
		let change = trader
			.buy_weight(WEIGHT, withdraw(&asset, fee), &XcmContext::with_message_id([0; 32]))
			.unwrap();
		assert!(change.is_empty());
		drop(trader);
		assert_eq!(asset_balance(&asset, &pot), 0);
		assert_eq!(
			<Assets as fungibles::Inspect<AccountId>>::total_issuance(asset.clone()),
			fee * 2
		);

		assert_ok!(<Balances as fungible::Mutate<AccountId>>::mint_into(&pot, EXISTENTIAL_DEPOSIT));
		let mut trader = Trader::new();
		let change = trader
			.buy_weight(WEIGHT, withdraw(&asset, fee), &XcmContext::with_message_id([0; 32]))
			.unwrap();
		assert!(change.is_empty());
		drop(trader);
		assert_eq!(asset_balance(&asset, &pot), fee);
		assert_eq!(<Assets as fungibles::Inspect<AccountId>>::total_issuance(asset), fee * 2);
	});
}
