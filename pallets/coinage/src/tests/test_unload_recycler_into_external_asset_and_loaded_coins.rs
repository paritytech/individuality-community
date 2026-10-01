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

use crate::{
	mock::*,
	pallet::{self, Error},
	*,
};
use codec::Encode;
use frame_support::{
	assert_err_ignore_postinfo, assert_ok, dispatch::GetDispatchInfo,
	traits::fungibles::InspectHold as _, weights::Weight, BoundedVec,
};
use frame_system::AuthorizeCall;
use indiv_support::traits::Alias;
use sp_crypto_hashing::blake2_256;
use sp_runtime::{bounded_vec, testing::UintAuthorityId};
use verifiable::GenerateVerifiable;

fn build_unload_ext(
	call: RuntimeCall,
	period: u32,
	counter: u32,
	recycler_secrets: &[Secret],
	value: Denomination,
	index: u32,
	people_alias_override: Option<Alias>,
) -> Extrinsic {
	let inherited_implication = ((0u8, &call), (), ());
	let proven_msg = sp_io::hashing::blake2_256(&inherited_implication.encode());

	let mut alias_proofs_vec = Vec::new();
	let ring_members = Coinage::get_recycler_members(TEST_INSTANCE_ID, value, index);

	for secret in recycler_secrets {
		let member = CryptoOf::<Test>::member_from_secret(secret);
		let commitment =
			CryptoOf::<Test>::open(recycler_ring_size(), &member, ring_members.clone().into_iter())
				.unwrap();
		let (proof, _) = CryptoOf::<Test>::create(
			commitment,
			secret,
			UNLOADING_RECYCLER_CONTEXT.as_ref(),
			&proven_msg,
		)
		.unwrap();
		alias_proofs_vec.push(proof);
	}

	let alias_proofs = BoundedVec::try_from(alias_proofs_vec).expect("Too many proofs");

	let intent_msg = sp_io::hashing::blake2_256(
		&[alias_proofs.encode(), inherited_implication.encode()].concat(),
	);
	let context = crate::pallet::free_unload_token_context(period, counter);
	let people_alias = people_alias_override.unwrap_or([0u8; 32]);
	let people_proof = MembershipProof {
		context: context.to_vec(),
		msg: intent_msg.to_vec(),
		alias: people_alias,
	};
	let info = crate::extension::AsCoinageInfo::AsUnloadTokenPeople {
		proof: people_proof,
		period,
		counter,
		alias_proofs,
	};

	let extension =
		(AuthorizeCall::<Test>::new(), crate::extension::AsCoinage::<Test>::new(Some(info)));
	Extrinsic::new_signed(call, 0, UintAuthorityId(0), extension)
}

fn create_alias_and_proof(
	secret: &Secret,
	value: Denomination,
	index: RingIndex,
	proven_msg: &[u8; 32],
) -> (Alias, ProofOf<Test>) {
	let ring_members = Coinage::get_recycler_members(TEST_INSTANCE_ID, value, index);
	let member = CryptoOf::<Test>::member_from_secret(secret);
	let commitment =
		CryptoOf::<Test>::open(recycler_ring_size(), &member, ring_members.into_iter()).unwrap();
	let (proof, alias) = CryptoOf::<Test>::create(
		commitment,
		secret,
		UNLOADING_RECYCLER_CONTEXT.as_ref(),
		proven_msg,
	)
	.unwrap();

	(alias, proof)
}

#[test]
fn mixed_output_prepaid_success() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_asset();
		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [11u8; 32];
		let (alias, proof) = create_alias_and_proof(secret, 0, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		let loaded_coin_secret = CryptoOf::<Test>::new_secret([200u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);
		let pallet_acc = Coinage::pallet_account();

		let post = Coinage::unload_recycler_into_external_asset_and_loaded_coins(
			RuntimeOrigin::from(origin),
			TEST_INSTANCE_ID,
			bounded_vec![alias],
			0,
			index,
			revision,
			42,
			500,
			bounded_vec![(-1, loaded_coin_member)],
			0,
		)
		.expect("unload should succeed");

		// Prepaid mode: refunded down to the `Prepaid` benchmarked weight (1 alias, 1 loaded_coin)
		// plus the instance reads of the settlement and charge branches, never above the charged
		// worst case `max(prepaid, from_output)`.
		assert_eq!(
			post.actual_weight,
			Some(
				Coinage::unload_recycler_into_external_asset_and_loaded_coins_prepaid_weight(
					1, 1, 0
				)
				.saturating_add(<Test as Config>::WeightInfo::read_instance().saturating_mul(2))
			),
		);
		assert!(post.actual_weight.unwrap().all_lte(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins_max_weight(1, 1, 0)
				.saturating_add(<Test as Config>::WeightInfo::read_instance().saturating_mul(2))
		));

		assert_eq!(
			RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, 0, index, alias)),
			Some(AliasState::Unloaded)
		);
		assert_eq!(
			RecyclersCoinToRecycler::<Test>::get(loaded_coin_member),
			Some((TEST_INSTANCE_ID, -1))
		);
		assert_eq!(AssetsWithHolder::total_balance(TEST_ASSET_ID, &42,), 500);
		assert_eq!(AssetsWithHolder::total_balance_on_hold(TEST_ASSET_ID, &pallet_acc,), 500);
		System::assert_has_event(
			crate::Event::<Test>::RecyclerUnloadedIntoExternalAssetAndLoadedCoins {
				instance_id: TEST_INSTANCE_ID,
				to: 42,
				value: 0,
				input_count: 1,
				external_asset_amount: 500,
				loaded_coin_count: 1,
			}
			.into(),
		);
	});
}

#[test]
fn mixed_output_empty_aliases_rejected() {
	new_test_ext().execute_with(|| {
		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [12u8; 32];
		let (_alias, proof) = create_alias_and_proof(secret, 0, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		assert_err_ignore_postinfo!(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins(
				RuntimeOrigin::from(origin),
				TEST_INSTANCE_ID,
				bounded_vec![],
				0,
				index,
				revision,
				42,
				0,
				bounded_vec![(
					-2,
					CryptoOf::<Test>::member_from_secret(&CryptoOf::<Test>::new_secret([12u8; 32]))
				)],
				0,
			),
			Error::<Test>::EmptyInputs
		);
	});
}

#[test]
fn mixed_output_empty_loaded_coins_rejected() {
	new_test_ext().execute_with(|| {
		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [13u8; 32];
		let (alias, proof) = create_alias_and_proof(secret, 0, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		assert_err_ignore_postinfo!(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins(
				RuntimeOrigin::from(origin),
				TEST_INSTANCE_ID,
				bounded_vec![alias],
				0,
				index,
				revision,
				42,
				1000,
				bounded_vec![],
				0,
			),
			Error::<Test>::InvalidSplit
		);
	});
}

#[test]
fn mixed_output_from_output_success() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_balances();

		let value: Denomination = 0;
		let dest = CHARLIE;
		let (secrets, index, revision) = setup_recycler(value, 2, 0);
		let secret = &secrets[0];
		let fee_amount = Coinage::get_paid_unload_token_fee_in_asset(TEST_INSTANCE_ID).unwrap();
		let external_asset_amount = 500u64;
		let proven_msg = blake2_256(&(value, index, revision, &dest).encode());
		let (alias, proof) = create_alias_and_proof(secret, value, index, &proven_msg);

		RecyclerManager::<Test>::mark_alias_unloaded(TEST_INSTANCE_ID, value, index, alias);

		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: pallet::UnloadFee::FromOutput {
				fee_recycler_value: value,
				fee_recycler_index: index,
			},
		};

		let loaded_coin_secret = CryptoOf::<Test>::new_secret([201u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);
		let market_before = AssetsWithHolder::total_balance(TEST_ASSET_ID, &MOCK_MARKET);
		let dest_before = AssetsWithHolder::total_balance(TEST_ASSET_ID, &dest);
		let pallet_acc = Coinage::pallet_account();
		let pallet_hold_before =
			AssetsWithHolder::total_balance_on_hold(TEST_ASSET_ID, &pallet_acc);

		let post = Coinage::unload_recycler_into_external_asset_and_loaded_coins(
			RuntimeOrigin::from(origin),
			TEST_INSTANCE_ID,
			bounded_vec![alias],
			value,
			index,
			revision,
			dest,
			external_asset_amount,
			bounded_vec![(-1, loaded_coin_member)],
			unload_token_fee_in_asset(),
		)
		.expect("unload should succeed");

		// FromOutput mode: refunded down to the `FromOutput` benchmarked weight (1 alias, 1
		// loaded_coin) plus the instance reads of the settlement and charge branches, never above
		// the charged worst case `max(prepaid, from_output)`.
		assert_eq!(
			post.actual_weight,
			Some(
				Coinage::unload_recycler_into_external_asset_and_loaded_coins_from_output_weight(
					1, 1, 0
				)
				.saturating_add(<Test as Config>::WeightInfo::read_instance().saturating_mul(2))
			),
		);
		assert!(post.actual_weight.unwrap().all_lte(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins_max_weight(1, 1, 0)
				.saturating_add(<Test as Config>::WeightInfo::read_instance().saturating_mul(2))
		));

		assert_eq!(
			RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, value, index, alias)),
			Some(AliasState::Unloaded),
		);
		assert_eq!(
			RecyclersCoinToRecycler::<Test>::get(loaded_coin_member),
			Some((TEST_INSTANCE_ID, -1))
		);
		assert_eq!(
			AssetsWithHolder::total_balance(TEST_ASSET_ID, &MOCK_MARKET,) - market_before,
			fee_amount
		);
		assert_eq!(
			AssetsWithHolder::total_balance(TEST_ASSET_ID, &dest,) - dest_before,
			external_asset_amount - fee_amount
		);
		assert_eq!(
			pallet_hold_before -
				AssetsWithHolder::total_balance_on_hold(TEST_ASSET_ID, &pallet_acc,),
			external_asset_amount
		);
		System::assert_has_event(
			crate::Event::<Test>::RecyclerUnloadedIntoExternalAssetAndLoadedCoins {
				instance_id: TEST_INSTANCE_ID,
				to: dest,
				value,
				input_count: 1,
				external_asset_amount: external_asset_amount - fee_amount,
				loaded_coin_count: 1,
			}
			.into(),
		);
	});
}

#[test]
fn mixed_output_loaded_coin_only_success() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_asset();
		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [14u8; 32];
		let (alias, proof) = create_alias_and_proof(secret, 0, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		let loaded_coin_secret = CryptoOf::<Test>::new_secret([14u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);

		assert_ok!(Coinage::unload_recycler_into_external_asset_and_loaded_coins(
			RuntimeOrigin::from(origin),
			TEST_INSTANCE_ID,
			bounded_vec![alias],
			0,
			index,
			revision,
			42,
			0,
			bounded_vec![(0, loaded_coin_member)],
			0,
		));

		assert_eq!(
			RecyclersCoinToRecycler::<Test>::get(loaded_coin_member),
			Some((TEST_INSTANCE_ID, 0))
		);
		assert_eq!(AssetsWithHolder::total_balance(TEST_ASSET_ID, &42,), 0);
		System::assert_has_event(
			crate::Event::<Test>::RecyclerUnloadedIntoExternalAssetAndLoadedCoins {
				instance_id: TEST_INSTANCE_ID,
				to: 42,
				value: 0,
				input_count: 1,
				external_asset_amount: 0,
				loaded_coin_count: 1,
			}
			.into(),
		);
	});
}

#[test]
fn mixed_output_exact_balance_mismatch_rejected() {
	new_test_ext().execute_with(|| {
		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [22u8; 32];
		let (alias, proof) = create_alias_and_proof(secret, 0, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		let loaded_coin_secret = CryptoOf::<Test>::new_secret([202u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);

		assert_err_ignore_postinfo!(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins(
				RuntimeOrigin::from(origin),
				TEST_INSTANCE_ID,
				bounded_vec![alias],
				0,
				index,
				revision,
				42,
				400,
				bounded_vec![(-1, loaded_coin_member)],
				0,
			),
			Error::<Test>::InvalidSplit
		);
	});
}

#[test]
fn mixed_output_duplicate_loaded_coin_member_key_rejected() {
	new_test_ext().execute_with(|| {
		let (secrets, index, revision) = setup_recycler(1, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [33u8; 32];
		let (alias, proof) = create_alias_and_proof(secret, 1, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		let loaded_coin_secret = CryptoOf::<Test>::new_secret([203u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);

		assert_err_ignore_postinfo!(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins(
				RuntimeOrigin::from(origin),
				TEST_INSTANCE_ID,
				bounded_vec![alias],
				1,
				index,
				revision,
				42,
				4000,
				bounded_vec![(0, loaded_coin_member), (0, loaded_coin_member)],
				0,
			),
			Error::<Test>::MemberKeyAlreadyUsed
		);
	});
}

#[test]
fn mixed_output_used_loaded_coin_member_key_rejected() {
	new_test_ext().execute_with(|| {
		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [34u8; 32];
		let (alias, proof) = create_alias_and_proof(secret, 0, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		let loaded_coin_secret = CryptoOf::<Test>::new_secret([204u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);
		assert_ok!(RecyclerManager::<Test>::load_batch_grouped(
			TEST_INSTANCE_ID,
			&[(1, loaded_coin_member)]
		));

		assert_err_ignore_postinfo!(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins(
				RuntimeOrigin::from(origin),
				TEST_INSTANCE_ID,
				bounded_vec![alias],
				0,
				index,
				revision,
				42,
				500,
				bounded_vec![(-1, loaded_coin_member)],
				0,
			),
			Error::<Test>::MemberKeyAlreadyUsed
		);
	});
}

#[test]
fn mixed_output_invalid_loaded_coin_value_rejected() {
	new_test_ext().execute_with(|| {
		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let proven_msg = [44u8; 32];
		let (alias, proof) = create_alias_and_proof(secret, 0, index, &proven_msg);
		let origin = pallet::Origin::<Test>::UnloadToken {
			alias_proofs: bounded_vec![proof],
			proven_msg,
			fee: UnloadFee::Prepaid,
		};

		let loaded_coin_secret = CryptoOf::<Test>::new_secret([204u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);
		let invalid_value = <Test as Config>::MaximumExponent::get() + 1;

		assert_err_ignore_postinfo!(
			Coinage::unload_recycler_into_external_asset_and_loaded_coins(
				RuntimeOrigin::from(origin),
				TEST_INSTANCE_ID,
				bounded_vec![alias],
				0,
				index,
				revision,
				42,
				0,
				bounded_vec![(invalid_value, loaded_coin_member)],
				0,
			),
			Error::<Test>::DenominationOutOfBound
		);
	});
}

#[test]
fn mixed_output_invalid_split_fails_in_extension_before_consuming_free_token() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_asset();

		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let alias: Alias =
			CryptoOf::<Test>::alias_in_context(secret, UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let loaded_coin_secret = CryptoOf::<Test>::new_secret([207u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);
		let people_alias = [77u8; 32];

		let call = RuntimeCall::Coinage(
			crate::Call::unload_recycler_into_external_asset_and_loaded_coins {
				instance_id: TEST_INSTANCE_ID,
				aliases: bounded_vec![alias],
				value: 0,
				index,
				revision,
				to: 42,
				external_asset_amount: 400,
				loaded_coins: bounded_vec![(-1, loaded_coin_member)],
				max_fee: 0,
			},
		);

		let ext = build_unload_ext(call, 0, 0, &secrets, 0, index, Some(people_alias));
		assert_invalid(ext, CustomInvalidity::InvalidSplit);
		assert!(!Coinage::is_free_token_alias_consumed(0, people_alias));
		assert_ne!(
			RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, 0, index, alias)),
			Some(AliasState::Unloaded)
		);
	});
}

#[test]
fn mixed_output_invalid_split_fails_in_extension_before_premarking_alias() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_balances();

		let value: Denomination = 0;
		let dest = CHARLIE;
		let (secrets, index, revision) = setup_recycler(value, 1, 0);
		let alias =
			CryptoOf::<Test>::alias_in_context(&secrets[0], UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let loaded_coin_secret = CryptoOf::<Test>::new_secret([208u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);

		let call = crate::Call::unload_recycler_into_external_asset_and_loaded_coins {
			instance_id: TEST_INSTANCE_ID,
			aliases: bounded_vec![alias],
			value,
			index,
			revision,
			to: dest,
			external_asset_amount: 501,
			loaded_coins: bounded_vec![(-1, loaded_coin_member)],
			max_fee: unload_token_fee_in_asset(),
		};

		let ext = build_unload_from_output_ext(call, value, index, revision, &secrets[0..1]);
		assert_invalid(ext, CustomInvalidity::InvalidSplit);
		assert_ne!(
			RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, value, index, alias)),
			Some(AliasState::Unloaded),
		);
	});
}

#[test]
fn mixed_output_from_output_rejects_asset_portion_below_fee() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_balances();

		let value: Denomination = 0;
		let dest = CHARLIE;
		let (secrets, index, revision) = setup_recycler(value, 1, 0);
		let alias =
			CryptoOf::<Test>::alias_in_context(&secrets[0], UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let loaded_coin_secret = CryptoOf::<Test>::new_secret([211u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);

		let call = crate::Call::unload_recycler_into_external_asset_and_loaded_coins {
			instance_id: TEST_INSTANCE_ID,
			aliases: bounded_vec![alias],
			value,
			index,
			revision,
			to: dest,
			external_asset_amount: 0,
			loaded_coins: bounded_vec![(0, loaded_coin_member)],
			max_fee: unload_token_fee_in_asset(),
		};

		let ext = build_unload_from_output_ext(call, value, index, revision, &secrets[0..1]);
		assert_invalid(ext, CustomInvalidity::UnloadedValueBelowFee);
		assert_ne!(
			RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, value, index, alias)),
			Some(AliasState::Unloaded),
		);
	});
}

/// The asset portion covers the fee, but the caller's own bound does not: `max_fee` is checked
/// while validating here as it is for the other `FromOutput` calls, so the transaction never
/// reaches a block.
#[test]
fn mixed_output_from_output_max_fee_below_the_quote_is_invalid() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_balances();

		let value: Denomination = 0;
		let dest = CHARLIE;
		let (secrets, index, revision) = setup_recycler(value, 1, 0);
		let alias =
			CryptoOf::<Test>::alias_in_context(&secrets[0], UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let loaded_coin_secret = CryptoOf::<Test>::new_secret([212u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);

		// The asset portion is worth far more than the fee: the bound is what fails.
		let external_asset_amount = 500u64;
		assert!(external_asset_amount > unload_token_fee_in_asset());

		let call = crate::Call::unload_recycler_into_external_asset_and_loaded_coins {
			instance_id: TEST_INSTANCE_ID,
			aliases: bounded_vec![alias],
			value,
			index,
			revision,
			to: dest,
			external_asset_amount,
			loaded_coins: bounded_vec![(-1, loaded_coin_member)],
			max_fee: unload_token_fee_in_asset() - 1,
		};

		let ext = build_unload_from_output_ext(call, value, index, revision, &secrets[0..1]);
		assert_invalid(ext, CustomInvalidity::MaxFeeInsufficientForUnload);
		assert_ne!(
			RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, value, index, alias)),
			Some(AliasState::Unloaded),
		);
	});
}

#[test]
fn mixed_output_from_output_failed_dispatch_locks_fee_alias() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_balances();

		let value: Denomination = 0;
		let dest = CHARLIE;
		let (secrets, index, revision) = setup_recycler(value, 3, 0);
		let alias0 =
			CryptoOf::<Test>::alias_in_context(&secrets[0], UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let alias1 =
			CryptoOf::<Test>::alias_in_context(&secrets[1], UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let loaded_coin_secret = CryptoOf::<Test>::new_secret([212u8; 32]);
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret);

		// Pre-consume the second alias so the mixed-output call passes extension validation but
		// fails once dispatch reaches the actual unload logic.
		RecyclerManager::<Test>::mark_alias_unloaded(TEST_INSTANCE_ID, value, index, alias1);

		let call = crate::Call::unload_recycler_into_external_asset_and_loaded_coins {
			instance_id: TEST_INSTANCE_ID,
			aliases: bounded_vec![alias0, alias1],
			value,
			index,
			revision,
			to: dest,
			external_asset_amount: 1500,
			loaded_coins: bounded_vec![(-1, loaded_coin_member)],
			max_fee: unload_token_fee_in_asset(),
		};

		let ext = build_unload_from_output_ext(call, value, index, revision, &secrets[0..2]);
		let result = Executive::apply_extrinsic(ext);
		assert!(matches!(result, Ok(Err(_))), "Dispatch should fail: {result:?}");
		assert_eq!(TotalValueOfDestroyedCoins::<Test>::get(TEST_INSTANCE_ID), 0);
		// The fee alias should not remain permanently marked as unloaded after the failed call.
		assert_ne!(
			RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, value, index, alias0)),
			Some(AliasState::Unloaded),
		);
		// From the outside, the observable behavior is that the alias is temporarily locked for a
		// retry rather than burnt or lost.
		assert!(
			super::get_recycler_alias_lock_until(value, index, alias0).is_some(),
			"failed dispatch should lock the fee alias for retry"
		);
	});
}

/// Sponsored-instance deposit flow of the mixed-output call through the given unload-token
/// extension flavor: the call is invalid while the pot cannot collateralize the fresh
/// loaded-coin keys, and once it can, dispatch settles the unloaded key's deposit and charges
/// one deposit per fresh key.
///
/// `max_fee` only bounds the `FromOutput` fee, which is paid out of the external asset portion,
/// so the bound follows the flavor.
fn sponsored_mixed_output_deposit_flow(make_variant: impl FnOnce() -> UnloadTokenVariant) {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		let variant = make_variant();
		let (instance_id, secrets, index, revision) = setup_sponsored_recycler(10, 100, 1, 0);
		drain_pot(instance_id, NATIVE_DEPOSIT_ID);
		assert!(matches!(
			Coinage::ensure_can_charge_load_deposit(instance_id, 2),
			Err(CustomInvalidity::PotCannotCoverLoadDeposit)
		));

		let max_fee = match variant {
			UnloadTokenVariant::FromOutput =>
				Coinage::get_paid_unload_token_fee_in_asset(instance_id)
					.expect("the mock fee conversion is available"),
			_ => 0,
		};
		let member_1 = CryptoOf::<Test>::member_from_secret(&get_unique_secret());
		let member_2 = CryptoOf::<Test>::member_from_secret(&get_unique_secret());
		let call = crate::Call::<Test>::unload_recycler_into_external_asset_and_loaded_coins {
			instance_id,
			aliases: bounded_vec![recycler_alias(&secrets[0])],
			value: 0,
			index,
			revision,
			to: 9_001,
			external_asset_amount: 500,
			loaded_coins: bounded_vec![(-2, member_1), (-2, member_2)],
			max_fee,
		};

		// The transaction is invalid while the two fresh keys cannot be collateralized.
		let ext = variant.build_ext(instance_id, call.clone(), &secrets, 0, index, revision, 0);
		assert_invalid(ext, CustomInvalidity::PotCannotCoverLoadDeposit);

		// Once the pot covers them, dispatch settles the unloaded key's deposit and charges
		// one deposit per fresh loaded coin.
		fund_pot(instance_id, NATIVE_DEPOSIT_ID, 100);
		let free_before = pot_free(instance_id, NATIVE_DEPOSIT_ID);
		let ext = variant.build_ext(instance_id, call, &secrets, 0, index, revision, 0);
		assert_eq!(Executive::apply_extrinsic(ext), Ok(Ok(())));
		assert_eq!(pot_held(instance_id, NATIVE_DEPOSIT_ID), 20);
		assert_eq!(free_before - pot_free(instance_id, NATIVE_DEPOSIT_ID), 10);
		check_load_deposit_invariant(instance_id, 2);
	});
}

#[test]
fn sponsored_mixed_output_people_token_deposit_flow() {
	sponsored_mixed_output_deposit_flow(|| UnloadTokenVariant::People);
}

#[test]
fn sponsored_mixed_output_lite_people_token_deposit_flow() {
	sponsored_mixed_output_deposit_flow(|| UnloadTokenVariant::LitePeople);
}

#[test]
fn sponsored_mixed_output_paid_token_deposit_flow() {
	sponsored_mixed_output_deposit_flow(|| paid_unload_token_variant(1));
}

#[test]
fn sponsored_mixed_output_from_output_deposit_flow() {
	sponsored_mixed_output_deposit_flow(|| UnloadTokenVariant::FromOutput);
}

#[test]
fn mixed_output_prepaid_ignores_nonzero_max_fee() {
	// A prepaid unload token takes no fee out of the external asset portion, so `max_fee` bounds
	// nothing: any value passes validation and dispatch, and the whole portion goes to `to`.
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		setup_asset();

		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let alias: Alias =
			CryptoOf::<Test>::alias_in_context(&secrets[0], UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let loaded_coin_member = CryptoOf::<Test>::member_from_secret(&get_unique_secret());

		let call = RuntimeCall::Coinage(
			crate::Call::unload_recycler_into_external_asset_and_loaded_coins {
				instance_id: TEST_INSTANCE_ID,
				aliases: bounded_vec![alias],
				value: 0,
				index,
				revision,
				to: 42,
				external_asset_amount: 500,
				loaded_coins: bounded_vec![(-1, loaded_coin_member)],
				max_fee: 499,
			},
		);

		let ext = build_unload_ext(call, 0, 0, &secrets, 0, index, None);
		assert_eq!(Executive::apply_extrinsic(ext), Ok(Ok(())));

		// Nothing was withheld for a fee.
		assert_eq!(AssetsWithHolder::total_balance(TEST_ASSET_ID, &42), 500);
		assert_eq!(
			RecyclersCoinToRecycler::<Test>::get(loaded_coin_member),
			Some((TEST_INSTANCE_ID, -1))
		);
	});
}

#[test]
fn mixed_output_multi_value_loaded_coins_work_via_unload_token_extension() {
	new_test_ext().execute_with(|| {
		// This is the one end-to-end test that still proves grouped loaded_coin
		// reminting across multiple values through the unload-token extension.
		System::set_block_number(1);
		setup_asset();

		let (secrets, index, revision) = setup_recycler(0, 1, 0);
		let secret = &secrets[0];
		let alias: Alias =
			CryptoOf::<Test>::alias_in_context(secret, UNLOADING_RECYCLER_CONTEXT.as_ref())
				.unwrap();
		let loaded_coin_secret_1 = CryptoOf::<Test>::new_secret([209u8; 32]);
		let loaded_coin_secret_2 = CryptoOf::<Test>::new_secret([210u8; 32]);
		let loaded_coin_member_1 = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret_1);
		let loaded_coin_member_2 = CryptoOf::<Test>::member_from_secret(&loaded_coin_secret_2);

		let call = RuntimeCall::Coinage(
			crate::Call::unload_recycler_into_external_asset_and_loaded_coins {
				instance_id: TEST_INSTANCE_ID,
				aliases: bounded_vec![alias],
				value: 0,
				index,
				revision,
				to: 42,
				external_asset_amount: 250,
				loaded_coins: bounded_vec![(-2, loaded_coin_member_1), (-1, loaded_coin_member_2)],
				max_fee: 0,
			},
		);

		let ext = build_unload_ext(call, 0, 0, &secrets, 0, index, None);
		let result = Executive::apply_extrinsic(ext);
		assert!(result.is_ok(), "Extrinsic should succeed: {result:?}");
		assert_eq!(
			RecyclersCoinToRecycler::<Test>::get(loaded_coin_member_1),
			Some((TEST_INSTANCE_ID, -2))
		);
		assert_eq!(
			RecyclersCoinToRecycler::<Test>::get(loaded_coin_member_2),
			Some((TEST_INSTANCE_ID, -1))
		);
		assert_eq!(AssetsWithHolder::total_balance(TEST_ASSET_ID, &42,), 250);
	});
}

fn check_mixed_output_diversity_weight(fee: UnloadFee) {
	for (exponents, expected_groups, expected_extras, external_asset_amount) in
		[([-2, -1, 0], 3, 0, 250), ([-2, -1, -2], 2, 1, 1_000)]
	{
		new_test_ext().execute_with(|| {
			System::set_block_number(1);
			setup_balances();
			let cheap = Weight::from_parts(1_000_000, 100_000);
			let expensive = Weight::from_parts(2_000_000, 200_000);
			let bases = match fee {
				UnloadFee::Prepaid => (cheap, expensive),
				UnloadFee::FromOutput { .. } => (expensive, cheap),
			};
			MockMixedOutputWeightBases::set(&Some(bases));
			// One denomination-1 input provides eight minimum-denomination units. Three
			// distinct outputs cost seven units, so three is the maximum diversity at d = 3.
			let value = 1;
			let (secrets, index, revision) = setup_recycler(value, 1, 0);
			let proven_msg = [91u8; 32];
			let (alias, proof) = create_alias_and_proof(&secrets[0], value, index, &proven_msg);
			let fee = match fee {
				UnloadFee::Prepaid => UnloadFee::Prepaid,
				UnloadFee::FromOutput { .. } => {
					RecyclerManager::<Test>::mark_alias_unloaded(
						TEST_INSTANCE_ID,
						value,
						index,
						alias,
					);
					UnloadFee::FromOutput { fee_recycler_value: value, fee_recycler_index: index }
				},
			};
			let loaded_coins: BoundedVec<_, <Test as Config>::MaxSplitOutputs> = exponents
				.into_iter()
				.enumerate()
				.map(|(i, denomination)| {
					let secret = CryptoOf::<Test>::new_secret([220 + i as u8; 32]);
					(denomination, CryptoOf::<Test>::member_from_secret(&secret))
				})
				.collect::<Vec<_>>()
				.try_into()
				.unwrap();
			let aliases: BoundedVec<_, <Test as Config>::MaxConsolidation> = bounded_vec![alias];
			let max_fee = unload_token_fee_in_asset();
			let call = Call::<Test>::unload_recycler_into_external_asset_and_loaded_coins {
				instance_id: TEST_INSTANCE_ID,
				aliases: aliases.clone(),
				value,
				index,
				revision,
				to: CHARLIE,
				external_asset_amount,
				loaded_coins: loaded_coins.clone(),
				max_fee,
			};
			let mut repeated = loaded_coins.clone();
			for (denomination, _) in &mut repeated {
				*denomination = -2;
			}
			let repeated_call =
				Call::<Test>::unload_recycler_into_external_asset_and_loaded_coins {
					instance_id: TEST_INSTANCE_ID,
					aliases: aliases.clone(),
					value,
					index,
					revision,
					to: CHARLIE,
					external_asset_amount: 1_250,
					loaded_coins: repeated,
					max_fee,
				};
			let declared = call.get_dispatch_info().call_weight;
			assert!(declared.all_gt(repeated_call.get_dispatch_info().call_weight));
			let recipient_before = AssetsWithHolder::total_balance(TEST_ASSET_ID, &CHARLIE);
			let market_before = AssetsWithHolder::total_balance(TEST_ASSET_ID, &MOCK_MARKET);
			let fee_amount = match fee {
				UnloadFee::Prepaid => 0,
				UnloadFee::FromOutput { .. } => max_fee,
			};
			let post = Coinage::unload_recycler_into_external_asset_and_loaded_coins(
				RuntimeOrigin::from(pallet::Origin::<Test>::UnloadToken {
					alias_proofs: bounded_vec![proof],
					proven_msg,
					fee,
				}),
				TEST_INSTANCE_ID,
				aliases,
				value,
				index,
				revision,
				CHARLIE,
				external_asset_amount,
				loaded_coins.clone(),
				max_fee,
			)
			.expect("diverse mixed-output unload should succeed");
			let expected = cheap
				.saturating_add(Weight::from_parts(10_000, 1_000).saturating_mul(expected_groups))
				.saturating_add(Weight::from_parts(100, 10).saturating_mul(expected_extras))
				.saturating_add(<Test as Config>::WeightInfo::read_instance().saturating_mul(2));
			assert_eq!(post.actual_weight, Some(expected));
			assert!(expected.all_lt(declared));
			assert_eq!(
				AssetsWithHolder::total_balance(TEST_ASSET_ID, &CHARLIE) - recipient_before,
				external_asset_amount - fee_amount
			);
			assert_eq!(
				AssetsWithHolder::total_balance(TEST_ASSET_ID, &MOCK_MARKET) - market_before,
				fee_amount
			);
			assert_eq!(
				RecyclerAliasStates::<Test>::get((TEST_INSTANCE_ID, value, index, alias)),
				Some(AliasState::Unloaded)
			);
			for (denomination, member) in loaded_coins {
				assert_eq!(
					RecyclersCoinToRecycler::<Test>::get(member),
					Some((TEST_INSTANCE_ID, denomination))
				);
			}
		});
	}
}

#[test]
fn mixed_output_prepaid_refund_counts_distinct_denominations_and_repeats() {
	check_mixed_output_diversity_weight(UnloadFee::Prepaid);
}

#[test]
fn mixed_output_from_output_refund_counts_distinct_denominations_and_repeats() {
	check_mixed_output_diversity_weight(UnloadFee::FromOutput {
		fee_recycler_value: 1,
		fee_recycler_index: 0,
	});
}

#[test]
fn mixed_output_counts_use_denominations_independently_of_order_and_member() {
	let member = CryptoOf::<Test>::member_from_secret(&CryptoOf::<Test>::new_secret([240; 32]));
	assert_eq!(Coinage::mixed_output_counts(&[]), (0, 0));
	for (denominations, expected) in [
		(vec![-2], (1, 0)),
		(vec![-2, -2, -2], (1, 2)),
		(vec![-2, 0, -2, 7, 0], (3, 2)),
		(vec![i8::MIN, i8::MAX, i8::MIN, 0], (3, 1)),
	] {
		let loaded = denominations.into_iter().map(|d| (d, member)).collect::<Vec<_>>();
		assert_eq!(Coinage::mixed_output_counts(&loaded), expected);
	}
}
