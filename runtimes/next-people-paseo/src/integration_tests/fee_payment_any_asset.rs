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

//! Transaction fees, XCM execution fees and XCM delivery fees are all payable in any asset that
//! either an `pallet-asset-conversion` pool or a governance-registered `pallet-asset-rate` rate
//! can price, whichever of the two asks the payer for less.

use super::*;
use crate::Assets as AssetsPallet;
use frame_support::{
	assert_ok,
	traits::{fungibles::Mutate as FungiblesMutate, tokens::ConversionToAssetBalance},
	weights::WeightToFee as WeightToFeeT,
};
use pallet_asset_conversion_tx_payment::OnChargeAssetTransaction;
use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
use sp_runtime::{traits::Dispatchable, FixedU128};
use xcm::latest::prelude::*;

/// The charging half of `pallet-asset-conversion-tx-payment`, as this runtime configures it.
type Charger = <Runtime as pallet_asset_conversion_tx_payment::Config>::OnChargeAssetTransaction;
type Trader = <crate::xcm_config::XcmConfig as xcm_executor::Config>::Trader;
type AssetExchanger = <crate::xcm_config::XcmConfig as xcm_executor::Config>::AssetExchanger;

/// The account every fee ends up in: the collator staking pot for transaction and execution fees,
/// the treasury for delivery fees.
fn staking_pot() -> AccountId {
	CollatorSelection::account_id()
}

/// An asset issued by Asset Hub's trust backed `Assets` pallet, as seen from here.
///
/// Deliberately *not* the external asset: that one is registered by `new_test_ext` and pinning
/// these tests to it would leave the generic paths untested.
fn foreign_asset(id: u128) -> Location {
	Location::new(1, [Parachain(ASSET_HUB_ID), PalletInstance(50), GeneralIndex(id)])
}

/// Registers `asset` as a sufficient asset and gives Alice PAS and plenty of it.
fn register(asset: &Location) {
	let alice = Sr25519Keyring::Alice.to_account_id();
	assert_ok!(AssetsPallet::force_create(
		RuntimeOrigin::root(),
		asset.clone(),
		alice.clone().into(),
		true, // is_sufficient
		1,    // min_balance
	));
	assert_ok!(Balances::mint_into(&alice, 100_000 * UNITS));
	assert_ok!(<AssetsPallet as FungiblesMutate<_>>::mint_into(
		asset.clone(),
		&alice,
		100_000 * UNITS
	));
}

/// Opens a PAS/`asset` pool and seeds it at one for one. Anyone can do this; no governance origin
/// is involved.
fn open_pool(asset: &Location) {
	open_pool_with(asset, 1_000 * UNITS, 1_000 * UNITS);
}

/// Opens a PAS/`asset` pool holding `native` PAS on the PAS side and `amount` of `asset` on the
/// other, which is what sets the price the pool quotes.
fn open_pool_with(asset: &Location, native: Balance, amount: Balance) {
	let alice = Sr25519Keyring::Alice.to_account_id();
	let native_location = crate::xcm_config::RelayLocation::get();
	assert_ok!(AssetConversion::create_pool(
		RuntimeOrigin::signed(alice.clone()),
		alloc::boxed::Box::new(native_location.clone()),
		alloc::boxed::Box::new(asset.clone()),
	));
	assert_ok!(AssetConversion::add_liquidity(
		RuntimeOrigin::signed(alice.clone()),
		alloc::boxed::Box::new(native_location),
		alloc::boxed::Box::new(asset.clone()),
		native,
		amount,
		1,
		1,
		alice,
	));
}

/// Registers a governance rate saying one unit of `asset` is worth `native_per_unit` PAS.
fn set_rate(asset: &Location, native_per_unit: u32) {
	assert_ok!(AssetRate::create(
		RuntimeOrigin::root(),
		alloc::boxed::Box::new(asset.clone()),
		FixedU128::from_u32(native_per_unit),
	));
}

/// One asset per pricing path, all registered and held by Alice.
struct PricedAssets {
	/// A pool against PAS and no rate.
	pooled: Location,
	/// A governance rate of 4 PAS per unit and no pool.
	rated: Location,
	/// Both, with the pool quoting the lower price.
	pool_cheaper: Location,
	/// Both, with the registered rate quoting the lower price.
	rate_cheaper: Location,
	/// Neither.
	unpriced: Location,
}

/// Registers one asset per pricing path. Alice holds all of them.
fn priced_assets() -> PricedAssets {
	let assets = PricedAssets {
		pooled: foreign_asset(111),
		rated: foreign_asset(222),
		pool_cheaper: foreign_asset(333),
		rate_cheaper: foreign_asset(444),
		unpriced: foreign_asset(555),
	};
	for asset in [
		&assets.pooled,
		&assets.rated,
		&assets.pool_cheaper,
		&assets.rate_cheaper,
		&assets.unpriced,
	] {
		assert_ne!(
			*asset,
			ExternalAssetLocation::get(),
			"fee tests must not use the external asset"
		);
		register(asset);
	}

	open_pool(&assets.pooled);
	set_rate(&assets.rated, 4);

	// One unit is worth 10 PAS in the pool but only 1 by the registered rate, so the pool asks for
	// a tenth of what the rate does.
	open_pool_with(&assets.pool_cheaper, 1_000 * UNITS, 100 * UNITS);
	set_rate(&assets.pool_cheaper, 1);

	// One unit is worth 1 PAS in the pool but 4 by the registered rate, so the rate asks for a
	// quarter of what the pool does.
	open_pool(&assets.rate_cheaper);
	set_rate(&assets.rate_cheaper, 4);

	assets
}

/// What the pool charges in `asset` for exactly `native` PAS.
fn pool_price_of(asset: &Location, native: Balance) -> Balance {
	AssetConversion::quote_price_tokens_for_exact_tokens(
		asset.clone(),
		crate::xcm_config::RelayLocation::get(),
		native,
		true,
	)
	.expect("the asset is in a pool with PAS")
}

/// What the governance rate charges in `asset` for `native` PAS.
fn rate_price_of(asset: &Location, native: Balance) -> Balance {
	AssetRate::to_asset_balance(native, asset.clone()).expect("the asset has a rate")
}

/// What the governance rate charges in `asset`, or `None` if it has no rate.
fn rate_price_if_any(asset: &Location, native: Balance) -> Option<Balance> {
	AssetRate::to_asset_balance(native, asset.clone()).ok()
}

/// The cheaper of the two oracles for `asset`, which is what the runtime must actually charge.
///
/// Panics if neither prices it, so a test that expects a price cannot silently pass on an asset
/// nothing can price.
fn cheapest_price_of(asset: &Location, native: Balance) -> Balance {
	let pool = AssetConversion::quote_price_tokens_for_exact_tokens(
		asset.clone(),
		crate::xcm_config::RelayLocation::get(),
		native,
		true,
	);
	match (pool, rate_price_if_any(asset, native)) {
		(Some(pool), Some(rate)) => pool.min(rate),
		(Some(pool), None) => pool,
		(None, Some(rate)) => rate,
		(None, None) => panic!("{asset:?} is priced by neither oracle"),
	}
}

/// XCM execution fees are charged through whichever of the pool and the governance-registered rate
/// asks the payer for less.
#[test]
fn xcm_execution_fees_charge_the_cheaper_of_pool_and_rate() {
	new_test_ext().execute_with(|| {
		let a = priced_assets();

		let weight = Weight::from_parts(1_000_000_000, 10_000);
		let native_fee = WeightToFee::weight_to_fee(&weight);
		let quote = |asset: &Location| {
			PolkadotXcm::query_weight_to_asset_fee::<Trader>(weight, AssetId(asset.clone()).into())
		};

		// PAS itself is priced by the plain `UsingComponents` trader.
		let native = crate::xcm_config::RelayLocation::get();
		assert_eq!(quote(&native).unwrap(), native_fee);
		// A pool-only asset is swapped for exactly `native_fee` worth of PAS.
		assert_eq!(quote(&a.pooled).unwrap(), pool_price_of(&a.pooled, native_fee));
		// A rate-only asset is taken in kind at that rate.
		assert_eq!(quote(&a.rated).unwrap(), native_fee / 4);
		// Every priced asset is charged the *minimum* of the two oracles, whichever that is.
		for asset in [&a.pooled, &a.rated, &a.pool_cheaper, &a.rate_cheaper] {
			assert_eq!(
				quote(asset).unwrap(),
				cheapest_price_of(asset, native_fee),
				"{asset:?} must be charged the cheaper of pool and rate",
			);
		}
		// And the two really do disagree, in both directions, so the minimum is not a no-op.
		assert!(
			pool_price_of(&a.pool_cheaper, native_fee) < rate_price_of(&a.pool_cheaper, native_fee)
		);
		assert!(
			rate_price_of(&a.rate_cheaper, native_fee) < pool_price_of(&a.rate_cheaper, native_fee)
		);
		// And an asset nobody priced buys no execution.
		assert!(quote(&a.unpriced).is_err());
	});
}

/// The same rule applies to delivery fees, which the routers always quote in PAS.
#[test]
fn xcm_delivery_fees_charge_the_cheaper_of_pool_and_rate() {
	new_test_ext().execute_with(|| {
		let a = priced_assets();

		// Only sibling delivery is priced on this chain, and it needs an open channel.
		let destination = Location::new(1, [Parachain(ASSET_HUB_ID)]);
		ParachainSystem::open_outbound_hrmp_channel_for_benchmarks_or_tests(ASSET_HUB_ID.into());

		let message = Xcm::<()>::builder_unsafe().clear_origin().build();
		let quote = |asset: &Location| -> Result<xcm::latest::Assets, ()> {
			PolkadotXcm::query_delivery_fees::<AssetExchanger>(
				VersionedLocation::from(destination.clone()),
				VersionedXcm::from(message.clone()),
				AssetId(asset.clone()).into(),
			)
			.map(|fees| {
				xcm::latest::Assets::try_from(fees).expect("fees are in the latest version")
			})
			.map_err(|_| ())
		};

		let native = crate::xcm_config::RelayLocation::get();
		let in_native = quote(&native).expect("Asset Hub is routable");
		let Some(Asset { fun: Fungible(native_fee), .. }) = in_native.get(0).cloned() else {
			panic!("delivery fees are a single fungible asset: {in_native:?}");
		};
		assert!(!native_fee.is_zero());

		let through_pool = |asset: &Location| {
			AssetConversion::quote_price_exact_tokens_for_tokens(
				native.clone(),
				asset.clone(),
				native_fee,
				true,
			)
			.expect("the asset is in a pool with PAS")
		};

		// A pool-only asset is sold for the PAS; a rate-only asset is taken in kind at its rate.
		assert_eq!(quote(&a.pooled).unwrap(), (a.pooled.clone(), through_pool(&a.pooled)).into());
		assert_eq!(quote(&a.rated).unwrap(), (a.rated.clone(), native_fee / 4).into());
		// Every priced asset is quoted the *minimum* of the two oracles. Delivery fees are priced
		// in the other direction from execution fees, "what does this PAS buy" rather than "what
		// does this cost", so the pool leg is quoted with `exact_tokens_for_tokens`.
		for asset in [&a.pooled, &a.rated, &a.pool_cheaper, &a.rate_cheaper] {
			let pool = AssetConversion::quote_price_exact_tokens_for_tokens(
				native.clone(),
				asset.clone(),
				native_fee,
				true,
			);
			let cheapest = match (pool, rate_price_if_any(asset, native_fee)) {
				(Some(pool), Some(rate)) => pool.min(rate),
				(Some(pool), None) => pool,
				(None, Some(rate)) => rate,
				(None, None) => panic!("{asset:?} is priced by neither oracle"),
			};
			assert_eq!(
				quote(asset).unwrap(),
				(asset.clone(), cheapest).into(),
				"{asset:?} must be quoted the cheaper of pool and rate",
			);
		}
		// And the two really do disagree, in both directions.
		assert!(through_pool(&a.pool_cheaper) < native_fee);
		assert!(native_fee / 4 < through_pool(&a.rate_cheaper));
		// And an asset nobody priced cannot pay for delivery either.
		assert!(quote(&a.unpriced).is_err());
	});
}

/// Transaction fees take the same rule, and settle in the asset the winning path implies: PAS into
/// the staking pot for the pool, the asset itself for the rate.
#[test]
fn transaction_fees_charge_the_cheaper_of_pool_and_rate() {
	new_test_ext().execute_with(|| {
		let a = priced_assets();

		let payer = Sr25519Keyring::Alice.to_account_id();
		let pot = staking_pot();
		let call: RuntimeCall = frame_system::Call::remark { remark: Vec::new() }.into();
		let info = frame_support::dispatch::GetDispatchInfo::get_dispatch_info(&call);
		let post_info = <RuntimeCall as Dispatchable>::PostInfo::default();
		let fee: Balance = UNITS / 100;

		// Charges `fee` in `asset` and returns what the payer was charged.
		let charge = |asset: &Location| -> Balance {
			assert_ok!(Charger::can_withdraw_fee(&payer, asset.clone(), fee));
			let before = <AssetsPallet as frame_support::traits::fungibles::Inspect<_>>::balance(
				asset.clone(),
				&payer,
			);
			let paid = Charger::withdraw_fee(&payer, &call, &info, asset.clone(), fee, 0)
				.expect("the asset is priced");
			let charged = Charger::correct_and_deposit_fee(
				&payer,
				&info,
				&post_info,
				fee,
				0,
				asset.clone(),
				paid,
			)
			.unwrap();
			assert_eq!(
				<AssetsPallet as frame_support::traits::fungibles::Inspect<_>>::balance(
					asset.clone(),
					&payer
				),
				before - charged
			);
			charged
		};
		let pot_holds = |asset: &Location| {
			<AssetsPallet as frame_support::traits::fungibles::Inspect<_>>::balance(
				asset.clone(),
				&pot,
			)
		};

		// An asset nobody priced cannot pay for a transaction.
		assert!(Charger::can_withdraw_fee(&payer, a.unpriced.clone(), fee).is_err());

		// A pool-only asset is swapped, so the staking pot is paid in PAS. The expected price is
		// read before charging, because the swap itself moves the pool.
		let pot_native_before = Balances::balance(&pot);
		let expected = pool_price_of(&a.pooled, fee);
		assert_eq!(charge(&a.pooled), expected);
		assert_eq!(Balances::balance(&pot), pot_native_before + fee);

		// A rate-only asset is taken in kind, so the pot is paid in the asset and no PAS moves.
		let pot_asset_before = pot_holds(&a.rated);
		let pot_native_before = Balances::balance(&pot);
		let expected = rate_price_of(&a.rated, fee);
		assert_eq!(expected, fee / 4);
		assert_eq!(charge(&a.rated), expected);
		assert_eq!(pot_holds(&a.rated), pot_asset_before + expected);
		assert_eq!(Balances::balance(&pot), pot_native_before);

		// Whichever oracle is cheaper is the one that charges, and the settlement asset follows it.
		// `charge` moves the pools, so each expectation is read immediately before its charge.
		let pot_native_before = Balances::balance(&pot);
		let expected = pool_price_of(&a.pool_cheaper, fee);
		assert!(expected < rate_price_of(&a.pool_cheaper, fee));
		assert_eq!(charge(&a.pool_cheaper), expected);
		assert_eq!(Balances::balance(&pot), pot_native_before + fee);

		let pot_asset_before = pot_holds(&a.rate_cheaper);
		let pot_native_before = Balances::balance(&pot);
		let expected = rate_price_of(&a.rate_cheaper, fee);
		assert!(expected < pool_price_of(&a.rate_cheaper, fee));
		assert_eq!(charge(&a.rate_cheaper), expected);
		assert_eq!(pot_holds(&a.rate_cheaper), pot_asset_before + expected);
		assert_eq!(Balances::balance(&pot), pot_native_before);
	});
}

/// A pool so thin that a single fee moves its price must never make an asset more expensive than
/// the rate governance registered for it.
///
/// Anyone can open a pool for any registered asset, and slippage, unlike a mispricing, is not
/// something arbitrage repairs: a correctly priced 1 PAS pool still doubles the cost of a 0.5 PAS
/// fee. Taking the cheaper of the two oracles is what makes that unprofitable to attempt.
#[test]
fn a_thin_pool_never_overcharges_against_the_registered_rate() {
	new_test_ext().execute_with(|| {
		let asset = foreign_asset(777);
		register(&asset);
		// One unit of the asset is worth one PAS, per governance.
		set_rate(&asset, 1);
		// A griefer opens the thinnest pool that will take a fee: 1 PAS a side.
		open_pool_with(&asset, UNITS, UNITS);

		let payer = Sr25519Keyring::Alice.to_account_id();
		let call: RuntimeCall = frame_system::Call::remark { remark: Vec::new() }.into();
		let info = frame_support::dispatch::GetDispatchInfo::get_dispatch_info(&call);

		// Half the pool's PAS side: on the constant-product curve this costs about twice the rate
		// price, so the pool must lose.
		let fee: Balance = UNITS / 2;
		assert!(pool_price_of(&asset, fee) > rate_price_of(&asset, fee) * 3 / 2);

		let before = <AssetsPallet as frame_support::traits::fungibles::Inspect<_>>::balance(
			asset.clone(),
			&payer,
		);
		let paid = Charger::withdraw_fee(&payer, &call, &info, asset.clone(), fee, 0)
			.expect("the rate prices the asset");
		let charged = Charger::correct_and_deposit_fee(
			&payer,
			&info,
			&<RuntimeCall as Dispatchable>::PostInfo::default(),
			fee,
			0,
			asset.clone(),
			paid,
		)
		.unwrap();
		assert_eq!(charged, rate_price_of(&asset, fee), "the rate should have won");
		assert_eq!(
			<AssetsPallet as frame_support::traits::fungibles::Inspect<_>>::balance(asset, &payer),
			before - charged
		);
	});
}

/// The XCM payment API advertises PAS plus everything the oracles can price: pooled assets and
/// rated assets, each listed once.
#[test]
fn xcm_payment_api_lists_every_payable_asset() {
	use xcm_runtime_apis::fees::runtime_decl_for_xcm_payment_api::XcmPaymentApiV2;

	new_test_ext().execute_with(|| {
		let a = priced_assets();

		let acceptable = Runtime::query_acceptable_payment_assets(xcm::latest::VERSION)
			.expect("the assets are all in the latest version")
			.into_iter()
			.map(|asset| AssetId::try_from(asset).expect("latest version"))
			.collect::<Vec<_>>();

		for expected in [
			&crate::xcm_config::RelayLocation::get(),
			&a.pooled,
			&a.rated,
			&a.pool_cheaper,
			&a.rate_cheaper,
		] {
			assert_eq!(
				acceptable.iter().filter(|id| id.0 == *expected).count(),
				1,
				"{expected:?} should be advertised exactly once, got {acceptable:?}",
			);
		}
		// Nothing prices this one, so it is not advertised.
		assert!(!acceptable.iter().any(|id| id.0 == a.unpriced));
	});
}

/// `ChargeAssetTxPayment` is served by `pallet-asset-conversion-tx-payment` rather than
/// `pallet-asset-tx-payment`, which is only allowed because the two encode identically: a compact
/// tip followed by an optional asset `Location`. Pin those bytes, because signers already built
/// against the old pallet target them.
#[test]
fn charge_asset_tx_payment_encoding_is_unchanged() {
	type ChargeAssetTxPayment = pallet_asset_conversion_tx_payment::ChargeAssetTxPayment<Runtime>;

	assert_eq!(ChargeAssetTxPayment::from(0, None).encode(), vec![0x00, 0x00]);
	assert_eq!(
		ChargeAssetTxPayment::from(1, Some(Location::parent())).encode(),
		vec![0x04, 0x01, 0x01, 0x00],
	);
}
