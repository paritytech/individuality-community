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

//! Public transaction-extension identifiers for both supported People pipelines.

use super::*;
use codec::{Compact, Decode, Encode};
use frame_support::{assert_ok, traits::SignedTransactionBuilder};
use indiv_pallet_people_airdrops::DrawSalts;
use sp_runtime::{
	traits::TransactionExtension,
	transaction_validity::{InvalidTransaction, TransactionValidityError},
};

const V0_PIPELINE: [&str; 11] = [
	"AuthorizeCall",
	"CheckNonZeroSender",
	"CheckSpecVersion",
	"CheckTxVersion",
	"CheckGenesis",
	"CheckMortality",
	"CheckNonce",
	"CheckWeight",
	"ChargeAssetTxPayment",
	"CheckMetadataHash",
	"StorageWeightReclaim",
];

const V1_PIPELINE: [&str; 23] = [
	"UnitTransactionExtension",
	"VerifyMultiSignature",
	"AsPerson",
	"AsProofOfInkParticipant",
	"ScoreAsParticipant",
	"GameAsInvited",
	"PeopleLiteAuth",
	"AsMember",
	"AsCoinage",
	"AsResources",
	"HonourAuth",
	"AuthorizeCall",
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
	"StorageWeightReclaim",
];

fn remark(remark: &[u8]) -> RuntimeCall {
	RuntimeCall::System(frame_system::Call::<Runtime>::remark_with_event {
		remark: remark.to_vec(),
	})
}

fn standard_v0_extension(nonce: u32) -> TxExtensionV0 {
	(
		frame_system::AuthorizeCall::<Runtime>::new(),
		frame_system::CheckNonZeroSender::<Runtime>::new(),
		frame_system::CheckSpecVersion::<Runtime>::new(),
		frame_system::CheckTxVersion::<Runtime>::new(),
		frame_system::CheckGenesis::<Runtime>::new(),
		frame_system::CheckEra::<Runtime>::from(generic::Era::Immortal),
		frame_system::CheckNonce::<Runtime>::from(nonce),
		frame_system::CheckWeight::<Runtime>::new(),
		pallet_asset_tx_payment::ChargeAssetTxPayment::<Runtime>::from(0u128, None),
		frame_metadata_hash_extension::CheckMetadataHash::<Runtime>::new(false),
	)
		.into()
}

fn signed_v0(who: Sr25519Keyring, call: RuntimeCall) -> UncheckedExtrinsic {
	let account = who.to_account_id();
	let ext = standard_v0_extension(frame_system::Pallet::<Runtime>::account_nonce(&account));
	let payload = generic::SignedPayload::new(call.clone(), ext.clone())
		.expect("the V0 standard payload is constructible");
	let signature = payload.using_encoded(|bytes| who.sign(bytes));
	UncheckedExtrinsic::new_signed_transaction(
		call,
		account.into(),
		MultiSignature::Sr25519(signature),
		ext,
	)
}

fn authorized_v0(call: RuntimeCall) -> UncheckedExtrinsic {
	UncheckedExtrinsic::from_parts(
		call,
		generic::Preamble::General(sp_runtime::traits::ExtensionVariant::V0(
			standard_v0_extension(0),
		)),
	)
}

fn due_draw_salt_cleanup_call(event_id: [u8; 32]) -> RuntimeCall {
	RuntimeCall::PeopleAirdrops(indiv_pallet_people_airdrops::Call::clean_up_draw_salt { event_id })
}

fn set_due_draw_salt_cleanup(event_id: [u8; 32]) {
	DrawSalts::<Runtime>::insert(event_id, ([0; 32], 60));
	pallet_timestamp::Now::<Runtime>::put(61_000u64);
}

#[test]
fn transaction_extension_pipelines_are_the_expected_ones() {
	let v0 = <TxExtensionV0 as TransactionExtension<RuntimeCall>>::metadata()
		.into_iter()
		.map(|metadata| metadata.identifier)
		.collect::<Vec<_>>();
	assert_eq!(v0, V0_PIPELINE);

	let v1 = <TxExtensionV1 as TransactionExtension<RuntimeCall>>::metadata()
		.into_iter()
		.map(|metadata| metadata.identifier)
		.collect::<Vec<_>>();
	assert_eq!(v1, V1_PIPELINE);
}

#[test]
fn legacy_v4_signed_transaction_uses_v0_and_charges_the_standard_fee() {
	new_test_ext().execute_with(|| {
		let account = Sr25519Keyring::Alice.to_account_id();
		let before = Balances::free_balance(&account);
		let encoded = signed_v0(Sr25519Keyring::Alice, remark(b"v0")).encode();
		let decoded = UncheckedExtrinsic::decode(&mut &encoded[..])
			.expect("a V4 signed V0 transaction decodes using the real runtime type");
		assert!(matches!(decoded.preamble, generic::Preamble::Signed(..)));
		Executive::apply_extrinsic(decoded)
			.expect("the standard V0 transaction is valid")
			.expect("the standard V0 transaction dispatches");
		assert_eq!(frame_system::Pallet::<Runtime>::account_nonce(&account), 1);
		assert!(Balances::free_balance(&account) < before, "the normal fee is charged");
	});
}

#[test]
fn general_v1_without_an_authorization_is_rejected() {
	new_test_ext().execute_with(|| {
		let unsigned = finalize_uxt(remark(b"unsigned"), base_tx_ext(remark(b"unsigned")));
		assert_eq!(
			Executive::apply_extrinsic(unsigned),
			Err(TransactionValidityError::Invalid(InvalidTransaction::UnknownOrigin)),
			"an ordinary V1 call cannot use a disabled VerifySignature as its origin"
		);
	});
}

#[test]
fn general_v0_authorized_call_dispatches_only_while_its_preconditions_hold() {
	new_test_ext().execute_with(|| {
		let event_id = [87; 32];
		set_due_draw_salt_cleanup(event_id);
		let encoded = authorized_v0(due_draw_salt_cleanup_call(event_id)).encode();
		let decoded = UncheckedExtrinsic::decode(&mut &encoded[..])
			.expect("a V0 authorised transaction decodes using the real runtime type");
		assert_ok!(Executive::apply_extrinsic(decoded).unwrap());
		assert!(!DrawSalts::<Runtime>::contains_key(event_id));

		// The first dispatch consumes the salt. A fresh V0 transaction must therefore
		// be rejected by the pallet-level `AuthorizeCall` precondition before dispatch.
		assert_eq!(
			Executive::apply_extrinsic(authorized_v0(due_draw_salt_cleanup_call(event_id)))
				.unwrap_err(),
			TransactionValidityError::Invalid(InvalidTransaction::Custom(
				indiv_pallet_people_airdrops::AuthorizeInvalidity::UnknownDraw as u8
			)),
			"V0 AuthorizeCall must not bypass the draw-salt cleanup preconditions"
		);
	});
}

#[test]
fn general_v1_rejects_a_signature_over_the_v0_implication() {
	new_test_ext().execute_with(|| {
		let signed_as_v0 =
			build_signed_ext_at_version(&Sr25519Keyring::Alice.pair(), remark(b"version"), 0);
		assert!(matches!(
			signed_as_v0.preamble,
			generic::Preamble::General(sp_runtime::traits::ExtensionVariant::Other(_))
		));
		assert_eq!(
			Executive::apply_extrinsic(signed_as_v0),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof)),
			"V1 VerifySignature must reject a signature that omits the V1 pipeline version"
		);
	});
}

#[test]
fn unsupported_general_extension_version_is_rejected_before_dispatch() {
	new_test_ext().execute_with(|| {
		let encoded = build_signed_ext(&Sr25519Keyring::Alice.pair(), remark(b"version")).encode();
		let mut version_two = encoded;
		let mut input = &version_two[..];
		Compact::<u32>::decode(&mut input).expect("an extrinsic has a compact length prefix");
		let preamble_offset = version_two.len() - input.len();
		// General extrinsics encode their format byte followed by the extension pipeline version.
		assert_eq!(version_two[preamble_offset + 1], INDIVIDUALITY_EXTENSION_VERSION);
		version_two[preamble_offset + 1] = 2;
		assert!(
			UncheckedExtrinsic::decode(&mut &version_two[..]).is_err(),
			"only the advertised V0 and V1 pipelines are decodable"
		);
	});
}
