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
use crate::{
	parameters::{
		AccountsApiAllowance, LitePersonStatementLimit, NotificationAllowance,
		NotificationPeriodDuration, PlayerStatementLimit, StmtStoreGraceWindow,
	},
	OperationAllowedOneTimeExcess, RestrictedEntity,
};
use codec::Decode;
use frame_support::traits::{Authorize, ContainsPair};
use indiv_pallet_origin_restriction::{RestrictedEntity as _, Usages};
#[cfg(feature = "runtime-benchmarks")]
use indiv_pallet_resources::benchmarking::BenchmarkHelper;
use indiv_pallet_resources::StatementStoreAllowances;
#[cfg(feature = "runtime-benchmarks")]
use sp_runtime::traits::Verify;
use sp_runtime::{
	generic::Preamble,
	traits::ExtensionVariant,
	transaction_validity::{InvalidTransaction, TransactionSource},
};
use sp_statement_store::{get_allowance, StatementAllowance};
use xcm::latest::{Junction, Location, NetworkId};

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
		assert_eq!(allowance, LitePersonStatementLimit::get());
	});
}

#[test]
fn notification_registration_allowance_lifecycle() {
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
		let period = Resources::notification_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::NotificationReference { period, seq };
		let context = Resources::notification_context(reference);

		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_notification_statement_account_for_sequence {
				reference,
				account_id: stmt_account.clone(),
			},
		);
		let as_person_uxt =
			build_as_alias_with_proof_ext(&person_secret, context, register_call.clone());
		assert!(
			Executive::apply_extrinsic(as_person_uxt).is_err(),
			"notification registration should not validate via AsPerson anymore"
		);
		exec_notification_registration_with_proof(&person_secret, period, seq, register_call);

		let active_allowance = get_allowance(&stmt_account);
		assert_eq!(active_allowance, NotificationAllowance::get());

		let cleanup_call =
			indiv_pallet_resources::Call::<Runtime>::clear_expired_notification_sequence {
				account: stmt_account.clone(),
				seq,
			};
		let cleanup_result = cleanup_call.authorize(TransactionSource::InBlock);
		assert_eq!(
			cleanup_result,
			Some(Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::InvalidExpiredNotificationCleanup
					as u8
			)
			.into()))
		);

		let period_duration = NotificationPeriodDuration::get();
		let grace = StmtStoreGraceWindow::get();
		let period_rollover_time =
			(period + 1).saturating_mul(period_duration).saturating_add(grace);
		set_time(period_rollover_time as u64);
		let still_fresh_result = cleanup_call.authorize(TransactionSource::InBlock);
		assert_eq!(
			still_fresh_result,
			Some(Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::InvalidExpiredNotificationCleanup
					as u8
			)
			.into()))
		);

		let cleanup_time = Resources::notification_expiration_time(period).saturating_add(1);
		set_time(cleanup_time);
		Resources::clear_expired_notification_sequence(
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
fn validate_collection_based_notification_registration() {
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
		let period = Resources::notification_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::NotificationReference { period, seq };

		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_notification_statement_account_for_sequence {
				reference,
				account_id: stmt_account.clone(),
			},
		);

		let uxt = build_notification_for_collection_ext(
			&person_secret,
			period,
			seq,
			register_call,
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			indiv_pallet_resources::types::MembershipCollection::People,
		);
		Executive::apply_extrinsic(uxt)
			.expect("collection-based notification transaction is valid")
			.expect("collection-based notification dispatch succeeds");

		let active_allowance = get_allowance(&stmt_account);
		assert_eq!(active_allowance, NotificationAllowance::get());
	});
}

#[test]
fn validate_collection_based_lite_notification_registration_lifecycle() {
	new_test_ext().execute_with(|| {
		let lite_pair = sr25519::Pair::from_seed(&[89u8; 32]);
		let lite_secret = register_lite_person_for_integration(&lite_pair);
		let stmt_pair = sr25519::Pair::from_seed(&[90u8; 32]);
		let stmt_account = pair_to_account_id(&stmt_pair);

		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::notification_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::NotificationReference { period, seq };
		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_notification_statement_account_for_sequence {
				reference,
				account_id: stmt_account.clone(),
			},
		);

		let uxt =
			build_lite_notification_registration_ext(&lite_secret, period, seq, register_call);
		Executive::apply_extrinsic(uxt)
			.expect("lite collection-based notification transaction is valid")
			.expect("lite collection-based notification dispatch succeeds");

		let active_allowance = get_allowance(&stmt_account);
		assert_eq!(active_allowance, NotificationAllowance::get());

		let cleanup_call =
			indiv_pallet_resources::Call::<Runtime>::clear_expired_notification_sequence {
				account: stmt_account.clone(),
				seq,
			};
		let cleanup_result = cleanup_call.authorize(TransactionSource::InBlock);
		assert_eq!(
			cleanup_result,
			Some(Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::InvalidExpiredNotificationCleanup
					as u8
			)
			.into()))
		);

		let cleanup_time = Resources::notification_expiration_time(period).saturating_add(1);
		set_time(cleanup_time);
		Resources::clear_expired_notification_sequence(
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
fn validate_collection_based_notification_rejects_lite_proof_with_people_variant() {
	new_test_ext().execute_with(|| {
		let lite_pair = sr25519::Pair::from_seed(&[91u8; 32]);
		let lite_secret = register_lite_person_for_integration(&lite_pair);
		let stmt_pair = sr25519::Pair::from_seed(&[92u8; 32]);
		let stmt_account = pair_to_account_id(&stmt_pair);

		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::notification_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::NotificationReference { period, seq };
		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_notification_statement_account_for_sequence {
				reference,
				account_id: stmt_account,
			},
		);

		let uxt = build_notification_for_collection_ext(
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
fn validate_collection_based_notification_rejects_people_proof_with_lite_variant() {
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
		let period = Resources::notification_period_from_timestamp(now_secs);
		let seq = 0u8;
		let reference = indiv_pallet_resources::types::NotificationReference { period, seq };
		let register_call = RuntimeCall::Resources(
			indiv_pallet_resources::Call::set_notification_statement_account_for_sequence {
				reference,
				account_id: stmt_account,
			},
		);

		let uxt = build_notification_for_collection_ext(
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
		exec_as_alias_with_proof(&person_secret, score_context(), set_alias.into());

		// Proof of ownership for statement account
		let score_alias = Crypto::alias_in_context(&person_secret, &score_context()[..]).unwrap();
		let stmt_acc_proof: MultiSignature = stmt_acc_pair
			.sign(&indiv_pallet_game::Pallet::<Runtime>::account_signing_payload(
				&(b"pop:game:stmt_account_for_alias:", score_alias)
					.using_encoded(sp_io::hashing::blake2_256),
			))
			.into();

		// There is a game scheduled
		let game_play_time = 2_000u32;
		let schedule = indiv_pallet_game::GameSchedule {
			game_play_time,
			rounds: 1,
			max_group_size: 3,
			airdrops: Default::default(),
			tenure_claim_window: 0,
		};
		Game::schedule_games(RuntimeOrigin::root(), vec![schedule.clone()]).unwrap();

		// and it's in registration phase
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedule));

		// The account signs up for the game with alias
		let sign_up = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			statement_account: stmt_acc.clone(),
			sig: stmt_acc_proof,
			identifier_key: [1u8; 65],
			airdrops: None,
		};
		exec_signed_as_alias_with_account(&alias_pair, sign_up.into());

		// The game moves on to reporting phase
		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedule));
		let game_info = indiv_pallet_game::Game::<Runtime>::get();
		assert!(game_info.is_some());
		assert!(matches!(game_info.unwrap().state, indiv_pallet_game::GameState::Reporting { .. }));

		let allowance = get_allowance(&stmt_acc);
		assert_eq!(allowance, PlayerStatementLimit::get());
	})
}

#[cfg(feature = "runtime-benchmarks")]
#[test]
fn benchmark_helper_signs_with_sr25519() {
	let mut ext = new_test_ext();
	ext.register_extension(sp_keystore::KeystoreExt::new(
		sp_keystore::testing::MemoryKeystore::new(),
	));
	ext.execute_with(|| {
		let message = b"benchmark message";
		let (account, signature) =
			crate::people::benchmark_utils::ResourcesBenchHelper::sign_message(message);
		assert!(matches!(signature, MultiSignature::Sr25519(_)));
		assert!(signature.verify(&message[..], &account));
		assert!(!signature.verify(&b"another message"[..], &account));
		// The benchmarks look the account up with one call and sign with another.
		let (same_account, _) =
			crate::people::benchmark_utils::ResourcesBenchHelper::sign_message(b"other");
		assert_eq!(same_account, account);
	});
}

/// Returns the implication the `AsResources` extension inherits in `tx_ext` for `call`. It holds
/// the extension version and call, then the explicit and implicit data of the later extensions.
fn resources_inherited_implication(call: &RuntimeCall, tx_ext: &TxExtensionV1) -> impl Encode {
	let following = (
		(tx_ext.0 .0 .10.clone(), tx_ext.0 .0 .11.clone()),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
		tx_ext.0 .10.clone(),
	);
	let implicit = following.implicit().expect("the following extensions have implicit data");
	((INDIVIDUALITY_EXTENSION_VERSION, call.clone()), following, implicit)
}

/// Builds the general transaction that moves the statement store slot of `alias` in `period`
/// to `new_account` with the witness `since`. The `AsResources` extension carries `holder`'s
/// sr25519 signature over the blake2 hash of the inherited implication.
fn build_signed_replacement_ext(
	holder: &sr25519::Pair,
	period: u32,
	alias: indiv_support::traits::Alias,
	new_account: AccountId32,
	since: u64,
) -> UncheckedExtrinsic {
	let call =
		RuntimeCall::Resources(indiv_pallet_resources::Call::replace_statement_store_account {
			period,
			alias,
			new_account,
			since,
		});
	let mut tx_ext = base_tx_ext(call.clone());
	let msg =
		resources_inherited_implication(&call, &tx_ext).using_encoded(sp_io::hashing::blake2_256);
	let signature: MultiSignature = holder.sign(&msg).into();
	tx_ext.0 .0 .9 = indiv_pallet_resources::extension::AsResources::new(Some(
		indiv_pallet_resources::extension::AsResourcesInfo::ReplaceStatementStoreAccount(signature),
	));
	finalize_uxt(call, tx_ext)
}

/// Returns a copy of `broadcast` with the nonce, tip and asset id set to their longest
/// encodings, as a relaying node could forward it. The call and the holder's signature are
/// unchanged.
fn pad_relayed_replacement(broadcast: &[u8]) -> UncheckedExtrinsic {
	let mut uxt = UncheckedExtrinsic::decode(&mut &broadcast[..]).expect("relayed bytes decode");
	let Preamble::General(ExtensionVariant::Other(pipeline)) = &mut uxt.preamble else {
		panic!("a signed replacement is a general transaction");
	};
	// An account on a forked network is the longest junction, 75 bytes. A location holds at most
	// 8 junctions.
	let junction = Junction::AccountId32 {
		network: Some(NetworkId::ByFork { block_number: u64::MAX, block_hash: [0xff; 32] }),
		id: [0xff; 32],
	};
	let asset_id = Location::new(0, [junction; 8]);
	pipeline.extension.0 .7 = frame_system::CheckNonce::<Runtime>::from(u32::MAX);
	pipeline.extension.0 .9 =
		pallet_asset_tx_payment::ChargeAssetTxPayment::<Runtime>::from(u128::MAX, Some(asset_id));
	uxt
}

#[test]
fn signed_replacement_moves_the_slot_allowance() {
	new_test_ext().execute_with(|| {
		let holder_pair = sr25519::Pair::from_seed(&[90u8; 32]);
		let holder = pair_to_account_id(&holder_pair);
		let new_account = pair_to_account_id(&sr25519::Pair::from_seed(&[91u8; 32]));
		let alias = [7u8; 32];
		advance_block();
		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::stmt_store_period_from_timestamp(now_secs);

		Resources::set_statement_store_account(
			OriginCaller::Resources(indiv_pallet_resources::Origin::StmtStoreAlias(alias)).into(),
			period,
			0,
			holder.clone(),
		)
		.unwrap();
		let since = StatementStoreAllowances::<Runtime>::get(
			indiv_support::utils::BigEndianU32::from(period),
			alias,
		)
		.expect("slot is claimed")
		.since;
		set_time(since + 1);

		// A signature by another key is refused before dispatch.
		let intruder = sr25519::Pair::from_seed(&[92u8; 32]);
		let uxt =
			build_signed_replacement_ext(&intruder, period, alias, new_account.clone(), since);
		assert_eq!(
			Executive::apply_extrinsic(uxt),
			Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::StmtStoreReplacementBadSignature
					as u8,
			)
			.into())
		);

		let uxt =
			build_signed_replacement_ext(&holder_pair, period, alias, new_account.clone(), since);
		Executive::apply_extrinsic(uxt)
			.expect("signed replacement transaction is valid")
			.expect("signed replacement dispatch succeeds");

		assert_eq!(get_allowance(&holder), StatementAllowance::default());
		assert_eq!(get_allowance(&new_account), AccountsApiAllowance::get());
	});
}

/// The origin restriction charges the slot by the transaction length. The holder's signature
/// covers the extension fields a relaying node could pad.
#[test]
fn signed_replacement_rejects_a_padded_relayed_copy() {
	new_test_ext().execute_with(|| {
		let holder_pair = sr25519::Pair::from_seed(&[95u8; 32]);
		let holder = pair_to_account_id(&holder_pair);
		let new_account = pair_to_account_id(&sr25519::Pair::from_seed(&[96u8; 32]));
		let alias = [8u8; 32];
		advance_block();
		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::stmt_store_period_from_timestamp(now_secs);

		Resources::set_statement_store_account(
			OriginCaller::Resources(indiv_pallet_resources::Origin::StmtStoreAlias(alias)).into(),
			period,
			0,
			holder.clone(),
		)
		.unwrap();
		let since = StatementStoreAllowances::<Runtime>::get(
			indiv_support::utils::BigEndianU32::from(period),
			alias,
		)
		.expect("slot is claimed")
		.since;
		set_time(since + 1);

		let broadcast =
			build_signed_replacement_ext(&holder_pair, period, alias, new_account.clone(), since)
				.encode();
		let padded = pad_relayed_replacement(&broadcast);
		assert!(padded.encoded_size() > 4 * broadcast.len());

		assert_eq!(
			Executive::apply_extrinsic(padded),
			Err(InvalidTransaction::Custom(
				indiv_pallet_resources::extension::CustomValidity::StmtStoreReplacementBadSignature
					as u8,
			)
			.into())
		);

		// The copy the holder broadcast is still valid.
		let uxt = UncheckedExtrinsic::decode(&mut &broadcast[..]).expect("broadcast bytes decode");
		Executive::apply_extrinsic(uxt)
			.expect("signed replacement transaction is valid")
			.expect("signed replacement dispatch succeeds");
		assert_eq!(get_allowance(&new_account), AccountsApiAllowance::get());
	});
}

#[test]
fn signed_replacement_is_limited_to_one_per_fifteen_minutes() {
	new_test_ext().execute_with(|| {
		let holder_pair = sr25519::Pair::from_seed(&[93u8; 32]);
		let holder = pair_to_account_id(&holder_pair);
		let second_pair = sr25519::Pair::from_seed(&[94u8; 32]);
		let second = pair_to_account_id(&second_pair);
		let third = pair_to_account_id(&sr25519::Pair::from_seed(&[95u8; 32]));
		let alias = [8u8; 32];

		// The first replacement lands on a relay chain block boundary.
		advance_to_block(3);
		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::stmt_store_period_from_timestamp(now_secs);
		let period_key = indiv_support::utils::BigEndianU32::from(period);
		Resources::set_statement_store_account(
			OriginCaller::Resources(indiv_pallet_resources::Origin::StmtStoreAlias(alias)).into(),
			period,
			0,
			holder.clone(),
		)
		.unwrap();
		let since = || StatementStoreAllowances::<Runtime>::get(period_key, alias).unwrap().since;
		set_time(since() + 1);
		let uxt =
			build_signed_replacement_ext(&holder_pair, period, alias, second.clone(), since());
		Executive::apply_extrinsic(uxt)
			.expect("first replacement is valid")
			.expect("first replacement dispatch succeeds");

		// The next replacement is refused until the fee of one replacement has recovered.
		let attempt = |block: BlockNumber| {
			advance_to_block(block);
			let uxt =
				build_signed_replacement_ext(&second_pair, period, alias, third.clone(), since());
			Executive::apply_extrinsic(uxt)
		};
		assert_eq!(attempt(4), Err(InvalidTransaction::Payment.into()));
		assert_eq!(attempt(3 + 60), Err(InvalidTransaction::Payment.into()));
		assert_eq!(attempt(3 + 449), Err(InvalidTransaction::Payment.into()));
		attempt(3 + 450)
			.expect("replacement is valid after 15 minutes")
			.expect("replacement dispatch succeeds");
		assert_eq!(get_allowance(&second), StatementAllowance::default());
		assert_eq!(get_allowance(&third), AccountsApiAllowance::get());
	});
}

#[test]
fn cleanup_drops_the_holder_usage_of_a_replaced_slot() {
	new_test_ext().execute_with(|| {
		let holder_pair = sr25519::Pair::from_seed(&[96u8; 32]);
		let holder = pair_to_account_id(&holder_pair);
		let new_account = pair_to_account_id(&sr25519::Pair::from_seed(&[97u8; 32]));
		let alias = [9u8; 32];
		advance_block();
		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::stmt_store_period_from_timestamp(now_secs);
		let period_key = indiv_support::utils::BigEndianU32::from(period);
		Resources::set_statement_store_account(
			OriginCaller::Resources(indiv_pallet_resources::Origin::StmtStoreAlias(alias)).into(),
			period,
			0,
			holder.clone(),
		)
		.unwrap();
		let since = StatementStoreAllowances::<Runtime>::get(period_key, alias).unwrap().since;
		set_time(since + 1);
		let uxt = build_signed_replacement_ext(&holder_pair, period, alias, new_account, since);
		Executive::apply_extrinsic(uxt).unwrap().unwrap();
		let entity = crate::RestrictedEntity::StmtStoreHolder(alias);
		let usage = indiv_pallet_origin_restriction::Usages::<Runtime>::get(&entity).unwrap();
		let other_entity = crate::RestrictedEntity::StmtStoreHolder([10u8; 32]);
		indiv_pallet_origin_restriction::Usages::<Runtime>::insert(&other_entity, usage.clone());

		let grace = u64::from(StmtStoreGraceWindow::get());
		set_time(u64::from(period + 1) * 24 * 60 * 60 + grace + 1);
		Resources::clear_expired_stmt_store_allowances(
			frame_system::RawOrigin::Authorized.into(),
			period,
			alias,
		)
		.unwrap();
		assert!(indiv_pallet_origin_restriction::Usages::<Runtime>::get(&entity).is_none());
		assert_eq!(
			indiv_pallet_origin_restriction::Usages::<Runtime>::get(&other_entity),
			Some(usage)
		);
		assert_eq!(StatementStoreAllowances::<Runtime>::get(period_key, alias), None);
	});
}

#[test]
fn replacement_fee_estimate_covers_a_signed_replacement() {
	new_test_ext().execute_with(|| {
		let holder_pair = sr25519::Pair::from_seed(&[98u8; 32]);
		let holder = pair_to_account_id(&holder_pair);
		let new_account = pair_to_account_id(&sr25519::Pair::from_seed(&[99u8; 32]));
		let alias = [11u8; 32];
		advance_block();
		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::stmt_store_period_from_timestamp(now_secs);
		let period_key = indiv_support::utils::BigEndianU32::from(period);
		Resources::set_statement_store_account(
			OriginCaller::Resources(indiv_pallet_resources::Origin::StmtStoreAlias(alias)).into(),
			period,
			0,
			holder,
		)
		.unwrap();
		let since = StatementStoreAllowances::<Runtime>::get(period_key, alias).unwrap().since;
		set_time(since + 1);
		let uxt = build_signed_replacement_ext(&holder_pair, period, alias, new_account, since);
		Executive::apply_extrinsic(uxt).unwrap().unwrap();

		let used = Usages::<Runtime>::get(RestrictedEntity::StmtStoreHolder(alias)).unwrap().used;
		let estimate = crate::stmt_store_replacement_fee();
		assert!(used > 0);
		assert!(estimate >= used, "estimate {estimate} below the usage {used} of a replacement");
	});
}

#[test]
fn stmt_store_holder_entity_allows_only_the_replacement_call() {
	let alias = [3u8; 32];
	let account = AccountId32::new([0u8; 32]);
	let entity = RestrictedEntity::StmtStoreHolder(alias);
	let replace =
		RuntimeCall::Resources(indiv_pallet_resources::Call::replace_statement_store_account {
			period: 0,
			alias,
			new_account: account.clone(),
			since: 0,
		});
	let claim = RuntimeCall::Resources(indiv_pallet_resources::Call::set_statement_store_account {
		period: 0,
		seq: 0,
		target_account: account.clone(),
	});

	assert!(OperationAllowedOneTimeExcess::contains(&entity, &replace));
	assert!(!OperationAllowedOneTimeExcess::contains(&entity, &claim));
	assert!(!OperationAllowedOneTimeExcess::contains(
		&RestrictedEntity::LitePerson(account),
		&replace
	));
	assert_eq!(
		RestrictedEntity::restricted_entity(&OriginCaller::Resources(
			indiv_pallet_resources::Origin::StmtStoreHolder(alias)
		)),
		Some(entity)
	);
}
