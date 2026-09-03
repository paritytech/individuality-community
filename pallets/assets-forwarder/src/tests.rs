// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0
//
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

use crate::{
	mock::{Assets as SourceAssets, *},
	Error, Event, ForwardedAssets,
};

use codec::{Decode, Encode};
use frame_support::{
	assert_noop, assert_ok,
	traits::fungible::{InspectHold, Mutate},
};
use xcm::latest::prelude::*;
use xcm_executor::traits::ConvertLocation;

const ASSET_ID: u32 = 7;
const MIN_BALANCE: u128 = 5;

/// The forwarded asset's id from the destination's perspective.
fn remote_asset_id() -> Location {
	Location::new(1, [Parachain(1500), PalletInstance(50), GeneralIndex(ASSET_ID.into())])
}

/// The location the destination authenticates and derives the replica owner from.
fn forwarder_location_on_destination() -> Location {
	Location::new(1, [Parachain(1500), PalletInstance(37)])
}

fn expected_owner() -> AccountId {
	<Test as crate::Config>::DestinationAccountOf::convert_location(
		&forwarder_location_on_destination(),
	)
	.expect("forwarder location converts to an account")
}

fn create_asset(is_sufficient: bool) {
	assert_ok!(SourceAssets::force_create(
		RuntimeOrigin::root(),
		ASSET_ID.into(),
		ALICE.into(),
		is_sufficient,
		MIN_BALANCE
	));
	// Local metadata exists so the tests can pin that it is never forwarded.
	assert_ok!(SourceAssets::force_set_metadata(
		RuntimeOrigin::root(),
		ASSET_ID.into(),
		b"Token".to_vec(),
		b"TOK".to_vec(),
		12,
		false
	));
}

/// Decodes the `call` bytes of every `Transact` in the sent message as destination runtime calls,
/// using the mock's destination-like `pallet-assets` instance.
fn sent_transacts() -> Vec<RuntimeCall> {
	let (destination, message) = sent_xcm().pop().expect("a message was sent");
	assert_eq!(destination, Location::new(1, [Parachain(1502)]));
	message
		.0
		.iter()
		.filter_map(|instruction| match instruction {
			Transact { origin_kind, fallback_max_weight, call } => {
				assert_eq!(*origin_kind, OriginKind::Xcm);
				assert_eq!(*fallback_max_weight, None);
				let bytes = call.clone().into_encoded();
				Some(RuntimeCall::decode(&mut &bytes[..]).expect("decodes as a runtime call"))
			},
			_ => None,
		})
		.collect::<Vec<_>>()
}

#[test]
fn forward_asset_works() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));

		// The deposit is held from the caller.
		assert_eq!(
			Balances::balance_on_hold(&crate::HoldReason::ForwardDeposit.into(), &BOB),
			ForwardDeposit::get()
		);
		let info = ForwardedAssets::<Test>::get(ASSET_ID).expect("asset is recorded");
		assert_eq!(info.depositor, BOB);
		assert_eq!(info.deposit, ForwardDeposit::get());
		assert_eq!(info.min_balance, MIN_BALANCE);
		assert!(info.is_sufficient);

		// The delivery fees are charged to the caller's location.
		let fees = charged_fees();
		assert_eq!(fees.len(), 1);
		assert_eq!(
			fees[0].0,
			Location::new(0, [AccountId32 { network: Some(NetworkId::Polkadot), id: BOB.into() }])
		);

		System::assert_last_event(
			Event::AssetForwarded {
				asset_id: ASSET_ID,
				remote_asset_id: remote_asset_id(),
				is_sufficient: true,
				message_id: sent_xcm()[0].1.using_encoded(sp_io::hashing::blake2_256),
			}
			.into(),
		);
	});
}

#[test]
fn forwarded_message_has_expected_shape() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));

		let (_, message) = sent_xcm().pop().unwrap();
		// Unpaid execution first, then the origin descends into the pallet, then the single
		// transact followed by a status check.
		assert_eq!(message.0.len(), 4);
		assert!(matches!(
			message.0[0],
			UnpaidExecution { weight_limit: Unlimited, check_origin: None }
		));
		assert_eq!(message.0[1], DescendOrigin([PalletInstance(37)].into()));
		assert!(matches!(message.0[2], Transact { .. }));
		assert_eq!(message.0[3], ExpectTransactStatus(MaybeErrorCode::Success));
	});
}

#[test]
fn forwarded_calls_decode_on_destination() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));

		// Metadata exists locally but only the creation call is sent.
		let calls = sent_transacts();
		assert_eq!(calls.len(), 1);
		assert_eq!(
			calls[0],
			RuntimeCall::DestAssets(pallet_assets::Call::force_create {
				id: remote_asset_id(),
				owner: expected_owner().into(),
				is_sufficient: true,
				min_balance: MIN_BALANCE,
			})
		);
	});
}

#[test]
fn forward_asset_mirrors_insufficiency() {
	new_test_ext().execute_with(|| {
		create_asset(false);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));

		let calls = sent_transacts();
		assert!(matches!(
			&calls[0],
			RuntimeCall::DestAssets(pallet_assets::Call::force_create { is_sufficient: false, .. })
		));
	});
}

#[test]
fn forward_asset_fails_for_unknown_asset() {
	new_test_ext().execute_with(|| {
		assert_noop!(
			Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()),
			Error::<Test>::UnknownAsset
		);
	});
}

#[test]
fn forward_asset_fails_for_destroying_asset() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(SourceAssets::start_destroy(RuntimeOrigin::root(), ASSET_ID.into()));
		assert_noop!(
			Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()),
			Error::<Test>::AssetNotLive
		);
	});
}

#[test]
fn forward_asset_fails_when_already_forwarded() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));
		assert_noop!(
			Forwarder::forward_asset(RuntimeOrigin::signed(ALICE), ASSET_ID.into()),
			Error::<Test>::AlreadyForwarded
		);
	});
}

#[test]
fn forward_asset_fails_without_deposit_funds() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		let poor: AccountId = sp_core::crypto::AccountId32::new([9u8; 32]);
		assert_ok!(Balances::mint_into(&poor, 1));
		assert_noop!(
			Forwarder::forward_asset(RuntimeOrigin::signed(poor), ASSET_ID.into()),
			sp_runtime::TokenError::FundsUnavailable
		);
	});
}

#[test]
fn forward_asset_rolls_back_on_send_failure() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		SendFails::set(&true);
		assert_noop!(
			Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()),
			Error::<Test>::SendFailed
		);
		assert_eq!(Balances::balance_on_hold(&crate::HoldReason::ForwardDeposit.into(), &BOB), 0);
		assert!(ForwardedAssets::<Test>::get(ASSET_ID).is_none());
	});
}

#[test]
fn forward_asset_rolls_back_on_fee_failure() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		FeeChargeFails::set(&true);
		assert_noop!(
			Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()),
			Error::<Test>::FeesNotPaid
		);
	});
}

#[test]
fn forward_asset_skips_fees_when_waived() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		FeesWaived::set(&true);
		// Charging is disabled to prove the waived path never reaches it.
		FeeChargeFails::set(&true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));
		assert!(charged_fees().is_empty());
	});
}

#[test]
fn sync_asset_status_works() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));

		// The asset stops being sufficient and gets a new minimum balance on the source chain.
		assert_ok!(SourceAssets::force_asset_status(
			RuntimeOrigin::root(),
			ASSET_ID.into(),
			ALICE.into(),
			ALICE.into(),
			ALICE.into(),
			ALICE.into(),
			MIN_BALANCE * 2,
			false,
			false
		));
		assert_ok!(Forwarder::sync_asset_status(RuntimeOrigin::signed(ALICE), ASSET_ID.into()));

		let calls = sent_transacts();
		assert_eq!(calls.len(), 1);
		let owner: sp_runtime::MultiAddress<AccountId, ()> = expected_owner().into();
		assert_eq!(
			calls[0],
			RuntimeCall::DestAssets(pallet_assets::Call::force_asset_status {
				id: remote_asset_id(),
				owner: owner.clone(),
				issuer: owner.clone(),
				admin: owner.clone(),
				freezer: owner,
				min_balance: MIN_BALANCE * 2,
				is_sufficient: false,
				is_frozen: false,
			})
		);
		System::assert_last_event(
			Event::AssetStatusSynced {
				asset_id: ASSET_ID,
				is_sufficient: false,
				message_id: sent_xcm()[1].1.using_encoded(sp_io::hashing::blake2_256),
			}
			.into(),
		);

		// The record tracks the values just sent.
		let info = ForwardedAssets::<Test>::get(ASSET_ID).expect("asset is recorded");
		assert_eq!(info.min_balance, MIN_BALANCE * 2);
		assert!(!info.is_sufficient);
	});
}

#[test]
fn sync_asset_status_rejects_unchanged_status() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));

		// Nothing changed since the forward.
		assert_noop!(
			Forwarder::sync_asset_status(RuntimeOrigin::signed(ALICE), ASSET_ID.into()),
			Error::<Test>::StatusUnchanged
		);

		// After a change one sync passes, then the guard closes again.
		assert_ok!(SourceAssets::force_asset_status(
			RuntimeOrigin::root(),
			ASSET_ID.into(),
			ALICE.into(),
			ALICE.into(),
			ALICE.into(),
			ALICE.into(),
			MIN_BALANCE,
			false,
			false
		));
		assert_ok!(Forwarder::sync_asset_status(RuntimeOrigin::signed(ALICE), ASSET_ID.into()));
		assert_noop!(
			Forwarder::sync_asset_status(RuntimeOrigin::signed(ALICE), ASSET_ID.into()),
			Error::<Test>::StatusUnchanged
		);
	});
}

#[test]
fn sync_asset_status_fails_when_not_forwarded() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_noop!(
			Forwarder::sync_asset_status(RuntimeOrigin::signed(BOB), ASSET_ID.into()),
			Error::<Test>::NotForwarded
		);
	});
}

#[test]
fn remove_forwarded_asset_works() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));
		assert_ok!(Forwarder::remove_forwarded_asset(RuntimeOrigin::root(), ASSET_ID.into()));

		// The record is gone and the deposit is released to the depositor, not burnt.
		assert!(ForwardedAssets::<Test>::get(ASSET_ID).is_none());
		assert_eq!(Balances::balance_on_hold(&crate::HoldReason::ForwardDeposit.into(), &BOB), 0);
		assert_eq!(Balances::free_balance(&BOB), 1_000_000_000);
		System::assert_last_event(Event::ForwardRemoved { asset_id: ASSET_ID }.into());

		// The asset can be forwarded again, with a fresh deposit.
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(ALICE), ASSET_ID.into()));
		assert_eq!(
			Balances::balance_on_hold(&crate::HoldReason::ForwardDeposit.into(), &ALICE),
			ForwardDeposit::get()
		);
	});
}

#[test]
fn remove_forwarded_asset_fails_for_non_manager() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_ok!(Forwarder::forward_asset(RuntimeOrigin::signed(BOB), ASSET_ID.into()));
		assert_noop!(
			Forwarder::remove_forwarded_asset(RuntimeOrigin::signed(ALICE), ASSET_ID.into()),
			sp_runtime::DispatchError::BadOrigin
		);
		assert!(ForwardedAssets::<Test>::get(ASSET_ID).is_some());
	});
}

#[test]
fn remove_forwarded_asset_fails_when_not_forwarded() {
	new_test_ext().execute_with(|| {
		create_asset(true);
		assert_noop!(
			Forwarder::remove_forwarded_asset(RuntimeOrigin::root(), ASSET_ID.into()),
			Error::<Test>::NotForwarded
		);
	});
}

#[test]
fn remote_owner_account_matches_destination_derivation() {
	new_test_ext().execute_with(|| {
		// The owner embedded in the calls is the sovereign account the destination would derive
		// for this pallet's location with the standard family description.
		assert_eq!(Forwarder::remote_owner_account().unwrap(), expected_owner());
	});
}
