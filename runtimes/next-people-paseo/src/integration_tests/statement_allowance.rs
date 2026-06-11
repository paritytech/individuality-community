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

//! Tests that pallet flows write the allowance storage that `sc_statement_store`
//! reads. Signature verification and rate limiting live in polkadot-sdk and are
//! exercised there; these tests only cover the runtime-side wiring.

use super::*;
use frame_support::traits::Authorize;
use sp_runtime::transaction_validity::{InvalidTransaction, TransactionSource};
use sp_statement_store::{get_allowance, StatementAllowance};

#[test]
fn lite_person_registration_grants_lite_allowance() {
	new_test_ext().execute_with(|| {
		let pair = sr25519::Pair::from_seed(&[10u8; 32]);
		let account = pair_to_account_id(&pair);

		Resources::register_lite_person(
			OriginCaller::PeopleLite(indiv_pallet_people_lite::Origin::LitePerson(account.clone()))
				.into(),
			[1u8; 65],
			b"mynameisme.12".to_vec().try_into().unwrap(),
			None,
		)
		.unwrap();

		let allowance = get_allowance(&account);
		assert_eq!(allowance, crate::people::LitePersonStatementLimit::get());
	});
}

#[test]
fn friend_request_registration_allowance_lifecycle() {
	new_test_ext().execute_with(|| {
		let person_secret = create_unique_secret();
		let person_member = Crypto::member_from_secret(&person_secret);
		let stmt_pair = sr25519::Pair::from_seed(&[77u8; 32]);
		let stmt_account = pair_to_account_id(&stmt_pair);

		DummyDim::reserve_ids(RuntimeOrigin::root(), 1).unwrap();
		DummyDim::recognize_personhood(
			RuntimeOrigin::root(),
			vec![(0, person_member)].try_into().unwrap(),
		)
		.unwrap();
		advance_block();
		advance_block();

		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::friend_request_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::FriendRequestReference { period, seq };
		let context = Resources::friend_request_context(reference);

		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_friend_request_statement_account_for_sequence {
				reference,
				account_id: stmt_account.clone(),
			},
		);
		let as_person_uxt =
			build_as_alias_with_proof_ext(&person_secret, context, register_call.clone());
		assert!(
			Executive::apply_extrinsic(as_person_uxt).is_err(),
			"friend request registration should not validate via AsPerson anymore"
		);
		exec_friend_request_registration_with_proof(&person_secret, period, seq, register_call);

		let active_allowance = get_allowance(&stmt_account);
		assert_eq!(active_allowance, crate::people::FriendRequestAllowance::get());

		let cleanup_call =
			indiv_pallet_resources::Call::<Runtime>::clear_expired_friend_request_sequence {
				account: stmt_account.clone(),
				seq,
			};
		let cleanup_result = cleanup_call.authorize(TransactionSource::InBlock);
		assert_eq!(
			cleanup_result,
			Some(Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::InvalidExpiredFriendRequestCleanup
					as u8
			)
			.into()))
		);

		let period_duration = crate::people::FriendRequestPeriodDuration::get();
		let grace = crate::people::FriendRequestGraceWindow::get();
		let period_rollover_time = (period + 1)
			.saturating_mul(period_duration)
			.saturating_add(grace)
			.saturating_add(1);
		set_time(period_rollover_time as u64);
		let still_fresh_result = cleanup_call.authorize(TransactionSource::InBlock);
		assert_eq!(
			still_fresh_result,
			Some(Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::InvalidExpiredFriendRequestCleanup
					as u8
			)
			.into()))
		);

		let cleanup_time = Resources::friend_request_expiration_time(period).saturating_add(1);
		set_time(cleanup_time);
		Resources::clear_expired_friend_request_sequence(
			frame_system::RawOrigin::Authorized.into(),
			stmt_account.clone(),
			seq,
		)
		.unwrap();

		let removed_allowance = get_allowance(&stmt_account);
		assert_eq!(removed_allowance, StatementAllowance::default());
	});
}

#[test]
fn validate_collection_based_friend_request_registration() {
	new_test_ext().execute_with(|| {
		let person_secret = create_unique_secret();
		let person_member = Crypto::member_from_secret(&person_secret);
		let stmt_pair = sr25519::Pair::from_seed(&[88u8; 32]);
		let stmt_account = pair_to_account_id(&stmt_pair);

		DummyDim::reserve_ids(RuntimeOrigin::root(), 1).unwrap();
		DummyDim::recognize_personhood(
			RuntimeOrigin::root(),
			vec![(0, person_member)].try_into().unwrap(),
		)
		.unwrap();
		advance_block();
		advance_block();

		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::friend_request_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::FriendRequestReference { period, seq };

		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_friend_request_statement_account_for_sequence {
				reference,
				account_id: stmt_account.clone(),
			},
		);

		let uxt = build_friend_request_for_collection_ext(
			&person_secret,
			period,
			seq,
			register_call,
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			indiv_pallet_resources::types::MembershipCollection::People,
		);
		Executive::apply_extrinsic(uxt)
			.expect("collection-based friend request transaction is valid")
			.expect("collection-based friend request dispatch succeeds");

		let active_allowance = get_allowance(&stmt_account);
		assert_eq!(active_allowance, crate::people::FriendRequestAllowance::get());
	});
}

#[test]
fn validate_collection_based_lite_friend_request_registration_lifecycle() {
	new_test_ext().execute_with(|| {
		let lite_pair = sr25519::Pair::from_seed(&[89u8; 32]);
		let lite_secret = register_lite_person_for_integration(&lite_pair);
		let stmt_pair = sr25519::Pair::from_seed(&[90u8; 32]);
		let stmt_account = pair_to_account_id(&stmt_pair);

		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::friend_request_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::FriendRequestReference { period, seq };
		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_friend_request_statement_account_for_sequence {
				reference,
				account_id: stmt_account.clone(),
			},
		);

		let uxt =
			build_lite_friend_request_registration_ext(&lite_secret, period, seq, register_call);
		Executive::apply_extrinsic(uxt)
			.expect("lite collection-based friend request transaction is valid")
			.expect("lite collection-based friend request dispatch succeeds");

		let active_allowance = get_allowance(&stmt_account);
		assert_eq!(active_allowance, crate::people::FriendRequestAllowance::get());

		let cleanup_call =
			indiv_pallet_resources::Call::<Runtime>::clear_expired_friend_request_sequence {
				account: stmt_account.clone(),
				seq,
			};
		let cleanup_result = cleanup_call.authorize(TransactionSource::InBlock);
		assert_eq!(
			cleanup_result,
			Some(Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::InvalidExpiredFriendRequestCleanup
					as u8
			)
			.into()))
		);

		let cleanup_time = Resources::friend_request_expiration_time(period).saturating_add(1);
		set_time(cleanup_time);
		Resources::clear_expired_friend_request_sequence(
			frame_system::RawOrigin::Authorized.into(),
			stmt_account.clone(),
			seq,
		)
		.unwrap();

		let removed_allowance = get_allowance(&stmt_account);
		assert_eq!(removed_allowance, StatementAllowance::default());
	});
}

#[test]
fn validate_collection_based_friend_request_rejects_lite_proof_with_people_variant() {
	new_test_ext().execute_with(|| {
		let lite_pair = sr25519::Pair::from_seed(&[91u8; 32]);
		let lite_secret = register_lite_person_for_integration(&lite_pair);
		let stmt_pair = sr25519::Pair::from_seed(&[92u8; 32]);
		let stmt_account = pair_to_account_id(&stmt_pair);

		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::friend_request_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::FriendRequestReference { period, seq };
		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_friend_request_statement_account_for_sequence {
				reference,
				account_id: stmt_account,
			},
		);

		let uxt = build_friend_request_for_collection_ext(
			&lite_secret,
			period,
			seq,
			register_call,
			indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER,
			indiv_pallet_resources::types::MembershipCollection::People,
		);

		assert!(matches!(
			Executive::apply_extrinsic(uxt),
			Err(sp_runtime::transaction_validity::TransactionValidityError::Invalid(
				InvalidTransaction::BadProof
			))
		));
	});
}

#[test]
fn validate_collection_based_friend_request_rejects_people_proof_with_lite_variant() {
	new_test_ext().execute_with(|| {
		let person_secret = create_unique_secret();
		let person_member = Crypto::member_from_secret(&person_secret);
		let stmt_pair = sr25519::Pair::from_seed(&[93u8; 32]);
		let stmt_account = pair_to_account_id(&stmt_pair);

		DummyDim::reserve_ids(RuntimeOrigin::root(), 1).unwrap();
		DummyDim::recognize_personhood(
			RuntimeOrigin::root(),
			vec![(0, person_member)].try_into().unwrap(),
		)
		.unwrap();
		advance_block();
		advance_block();

		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::friend_request_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::FriendRequestReference { period, seq };
		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_friend_request_statement_account_for_sequence {
				reference,
				account_id: stmt_account,
			},
		);

		let uxt = build_friend_request_for_collection_ext(
			&person_secret,
			period,
			seq,
			register_call,
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			indiv_pallet_resources::types::MembershipCollection::LitePeople,
		);

		assert!(matches!(
			Executive::apply_extrinsic(uxt),
			Err(sp_runtime::transaction_validity::TransactionValidityError::Invalid(
				InvalidTransaction::BadProof
			))
		));
	});
}

#[test]
fn game_sign_up_grants_player_allowance() {
	new_test_ext().execute_with(|| {
		let person_secret = Crypto::new_secret([10u8; 32]);
		let person_member = Crypto::member_from_secret(&person_secret);

		let alias_pair = sr25519::Pair::from_seed(&[10u8; 32]);

		let stmt_acc_pair = sr25519::Pair::from_seed(&[11u8; 32]);
		let stmt_acc = pair_to_account_id(&stmt_acc_pair);

		// Account becomes a person
		DummyDim::reserve_ids(RuntimeOrigin::root(), 1).unwrap();
		DummyDim::recognize_personhood(
			RuntimeOrigin::root(),
			vec![(0, person_member)].try_into().unwrap(),
		)
		.unwrap();
		// Onboarding and ring building happen in separate blocks
		advance_block();
		advance_block();

		// To set alias account
		let set_alias = indiv_pallet_people::Call::<Runtime>::set_alias_account {
			account: alias_pair.public().into(),
			call_valid_at: frame_system::Pallet::<Runtime>::block_number(),
		};
		exec_as_alias_with_proof(&person_secret, SCORE_CONTEXT, set_alias.into());

		// Proof of ownership for statement account
		let score_alias = Crypto::alias_in_context(&person_secret, &SCORE_CONTEXT[..]).unwrap();
		let stmt_acc_proof: MultiSignature = stmt_acc_pair
			.sign(
				&(b"pop:game:stmt_account_for_alias:", score_alias)
					.using_encoded(sp_io::hashing::blake2_256)[..],
			)
			.into();

		// There is a game scheduled
		let game_play_time = 2_000u32;
		let schedule = indiv_pallet_game::GameSchedule {
			game_play_time,
			rounds: 1,
			max_group_size: 3,
			airdrop_prize: None,
		};
		Game::schedule_games(RuntimeOrigin::root(), vec![schedule.clone()]).unwrap();

		// and it's in registration phase
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedule));

		// The account signs up for the game with alias
		let sign_up = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			statement_account: stmt_acc.clone(),
			sig: stmt_acc_proof,
			identifier_key: [1u8; 65],
			airdrop: None,
		};
		exec_signed_as_alias_with_account(&alias_pair, sign_up.into());

		// The game moves on to reporting phase
		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedule));
		let game_info = indiv_pallet_game::Game::<Runtime>::get();
		assert!(game_info.is_some());
		assert!(matches!(game_info.unwrap().state, indiv_pallet_game::GameState::Reporting { .. }));

		let allowance = get_allowance(&stmt_acc);
		assert_eq!(allowance, crate::people::PlayerStatementLimit::get());
	})
}
