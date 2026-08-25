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

//! Tests for transaction payment with external assets.

use super::*;
use xcm::latest::prelude::*;

// Helper: build a signed extrinsic that pays transaction fee in external asset and optional tip.
fn build_signed_ext_with_external_asset_payment(
	who: &sr25519::Pair,
	call: RuntimeCall,
	tip: Balance,
) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let who_account = pair_to_account_id(who);

	// update payment extension to pay in external asset with optional tip
	tx_ext.0 .9 =
		pallet_skip_feeless_payment::SkipCheckIfFeeless::<
			Runtime,
			pallet_asset_conversion_tx_payment::ChargeAssetTxPayment<Runtime>,
		>::from(pallet_asset_conversion_tx_payment::ChargeAssetTxPayment::<Runtime>::from(
			tip,
			Some(ExternalAssetLocation::get()),
		));

	// update CheckNonce
	{
		let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);
		tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);
	}

	// update VerifySignature
	{
		let rest_ext = (
			(
				tx_ext.0 .0 .2.clone(),
				tx_ext.0 .0 .3.clone(),
				tx_ext.0 .0 .4.clone(),
				tx_ext.0 .0 .5.clone(),
				tx_ext.0 .0 .6.clone(),
				tx_ext.0 .0 .7.clone(),
				tx_ext.0 .0 .8.clone(),
				tx_ext.0 .0 .9.clone(),
				tx_ext.0 .0 .10.clone(),
			),
			tx_ext.0 .1.clone(),
			tx_ext.0 .2.clone(),
			tx_ext.0 .3.clone(),
			tx_ext.0 .4.clone(),
			tx_ext.0 .5.clone(),
			tx_ext.0 .6.clone(),
			tx_ext.0 .7.clone(),
			tx_ext.0 .8.clone(),
			tx_ext.0 .9.clone(),
		);

		let msg = {
			let implication_base = (0u8, &call);
			let implication_explicit = &rest_ext;
			let implication_implicit = &rest_ext.implicit().unwrap();
			let encoded_implications =
				(implication_base, implication_explicit, implication_implicit).encode();
			sp_io::hashing::blake2_256(&encoded_implications)
		};

		let raw_sig = who.sign(&msg);

		tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
			MultiSignature::from(raw_sig),
			who_account.clone(),
		);
	}

	finalize_uxt(call, tx_ext)
}

#[test]
fn tx_fee_in_external_asset_with_refund() {
	new_test_ext().execute_with(|| {
		let bob = Sr25519Keyring::Bob.pair();
		let bob_id = Sr25519Keyring::Bob.to_account_id();

		// Mint enough external asset to Bob
		FungibleExternalAsset::mint_into(&bob_id, 10 * UNITS).expect("mint external asset to bob");

		// Pre-measure pot and bob balances
		let pot = CollatorSelection::account_id();
		let pot_before = FungibleExternalAsset::balance(&pot);
		let bob_before = FungibleExternalAsset::balance(&bob_id);

		// Use a remark (Pays::No). Tip remains payable; fees should be refunded.
		let tip: Balance = CENTS; // small but visible
		let call = frame_system::Call::<Runtime>::remark { remark: Vec::new() };
		let uxt = build_signed_ext_with_external_asset_payment(&bob, call.into(), tip);
		Executive::apply_extrinsic(uxt).expect("tx valid").expect("dispatch success");

		// Only the tip should be finally paid to the collators pot in external asset.
		let pot_after = FungibleExternalAsset::balance(&pot);
		let bob_after = FungibleExternalAsset::balance(&bob_id);

		assert!(pot_after > pot_before);
		assert_eq!(bob_before.saturating_sub(bob_after), pot_after.saturating_sub(pot_before));

		// Ensure native balance unchanged (fee not taken in native)
		// Note: no native mint here, so just check it's zero both before and after.
		assert_eq!(Balances::free_balance(bob_id.clone()), 0);
	});
}

#[test]
fn xcm_execute_paid_in_external_asset() {
	new_test_ext().execute_with(|| {
		let bob = Sr25519Keyring::Bob.pair();
		let bob_id = Sr25519Keyring::Bob.to_account_id();

		// Mint external asset to Bob to cover tx fee and XCM weight purchase
		FungibleExternalAsset::mint_into(&bob_id, 100 * UNITS).expect("mint external asset to bob");

		// Measure balances before
		let collator_pot = CollatorSelection::account_id();
		let collator_pot_before = FungibleExternalAsset::balance(&collator_pot);
		let bob_before = FungibleExternalAsset::balance(&bob_id);

		// Build an XCM which buys execution using the external asset.
		// Use Unlimited weight limit so exactly-required fees are withdrawn.
		let fees_asset = Asset {
			id: AssetId(ExternalAssetLocation::get()),
			// Provide a large amount; trader will withdraw only what is needed.
			fun: (10 * UNITS).into(),
		};
		let msg = Xcm(vec![BuyExecution { fees: fees_asset, weight_limit: Unlimited }]);
		let vmsg = VersionedXcm::from(msg);

		// Execute the XCM locally; pay extrinsic fee in external asset too (no tip).
		let call = pallet_xcm::Call::<Runtime>::execute {
			message: Box::new(vmsg),
			// Paseo's XCM weights may differ; use a generous limit
			max_weight: Weight::from_parts(500_000_000, 20_000),
		};
		let uxt = build_signed_ext_with_external_asset_payment(&bob, call.into(), 0);
		use frame_support::dispatch::GetDispatchInfo;
		dbg!(&uxt.get_dispatch_info());
		Executive::apply_extrinsic(uxt).expect("tx valid").expect("dispatch success");

		// After execution:
		// - Collator pot should have received the transaction fee (in external asset).
		// - Treasury should have received the XCM execution fee (in external asset).
		let collator_pot_after = FungibleExternalAsset::balance(&collator_pot);
		let bob_after = FungibleExternalAsset::balance(&bob_id);

		assert!(collator_pot_after > collator_pot_before, "collator pot should increase");
		assert!(bob_after < bob_before, "bob should spend external asset");
		assert_eq!(
			collator_pot_after.saturating_sub(collator_pot_before),
			bob_before.saturating_sub(bob_after)
		);

		// Native unaffected for this payer
		assert_eq!(Balances::free_balance(bob_id.clone()), 0);
	});
}
