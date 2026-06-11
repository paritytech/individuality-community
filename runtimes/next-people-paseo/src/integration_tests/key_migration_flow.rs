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

use super::*;
use codec::Encode;
use frame_support::assert_ok;
use sp_runtime::{
	transaction_validity::{InvalidTransaction, TransactionValidityError},
	DispatchError, ModuleError,
};

#[test]
fn key_migration_onboarding_queue() {
	new_test_ext().execute_with(|| {
		// ═══════════════════════════════════════════════════════════════════════════
		// Setup - Creates People in Onboarding State
		// ═══════════════════════════════════════════════════════════════════════════

		// Person A
		let secret_a = Crypto::new_secret([10u8; 32]);
		let key_a = Crypto::member_from_secret(&secret_a);

		// Person B
		let secret_b = Crypto::new_secret([20u8; 32]);
		let key_b = Crypto::member_from_secret(&secret_b);

		// Person A and B have their personhood recognized
		DummyDim::reserve_ids(RuntimeOrigin::root(), 2).unwrap();
		DummyDim::recognize_personhood(
			RuntimeOrigin::root(),
			vec![(0, key_a.clone()), (1, key_b.clone())].try_into().unwrap(),
		)
		.unwrap();

		let person_a_id = 0u64;
		let person_b_id = 1u64;

		// Set up sr25519 accounts for both people
		let account_a = sr25519::Pair::from_seed(&[10u8; 32]);
		let account_b = sr25519::Pair::from_seed(&[20u8; 32]);

		// Link accounts to personal IDs
		exec_set_personal_id_account(&secret_a, person_a_id, &account_a);
		exec_set_personal_id_account(&secret_b, person_b_id, &account_b);

		// Fund accounts
		let account_a_id = pair_to_account_id(&account_a);
		let account_b_id = pair_to_account_id(&account_b);
		Balances::set_balance(&account_a_id, 1_000_000_000_000);
		Balances::set_balance(&account_b_id, 1_000_000_000_000);

		// People are still in onboarding state (no blocks advanced)
		let record_a = indiv_pallet_people::People::<Runtime>::get(person_a_id).unwrap();
		assert!(matches!(record_a.position, indiv_pallet_people::RingPosition::Onboarding { .. }));
		let record_b = indiv_pallet_people::People::<Runtime>::get(person_b_id).unwrap();
		assert!(matches!(record_b.position, indiv_pallet_people::RingPosition::Onboarding { .. }));

		// ═══════════════════════════════════════════════════════════════════════════
		// Key Migration - success case
		// ═══════════════════════════════════════════════════════════════════════════

		// New key of Person A
		let secret_a_new = Crypto::new_secret([11u8; 32]);
		let key_a_new = Crypto::member_from_secret(&secret_a_new);

		// Person A migrates their key
		let migrate_call = RuntimeCall::People(indiv_pallet_people::Call::migrate_onboarding_key {
			new_key: key_a_new.clone(),
		});
		exec_signed_as_personal_id(&account_a, migrate_call);

		// Migration completes immediately
		let record_a_after = indiv_pallet_people::People::<Runtime>::get(person_a_id).unwrap();
		assert_eq!(record_a_after.key, key_a_new);

		// ═══════════════════════════════════════════════════════════════════════════
		// Key Migrations - restrictions
		// ═══════════════════════════════════════════════════════════════════════════

		// Cannot migrate to another known key
		let migrate_to_b_call =
			RuntimeCall::People(indiv_pallet_people::Call::migrate_onboarding_key {
				new_key: key_b.clone(),
			});
		let uxt = build_signed_as_personal_id_ext(&account_a, migrate_to_b_call);
		let result = Executive::apply_extrinsic(uxt);
		assert!(matches!(
			result,
			Ok(Err(DispatchError::Module(ModuleError { error, .. })))
			if error[0] == indiv_pallet_people::Error::<Runtime>::KeyAlreadyInUse.encode()[0]
		));

		// Cannot migrate to the same key
		let migrate_to_same_call =
			RuntimeCall::People(indiv_pallet_people::Call::migrate_onboarding_key {
				new_key: key_a_new.clone(),
			});
		let uxt = build_signed_as_personal_id_ext(&account_a, migrate_to_same_call);
		let result = Executive::apply_extrinsic(uxt);
		assert!(matches!(
			result,
			Ok(Err(DispatchError::Module(ModuleError { error, .. })))
			if error[0] == indiv_pallet_people::Error::<Runtime>::KeyAlreadyInUse.encode()[0]
		));

		// Wrong migration function invoked
		let key_a_another = Crypto::member_from_secret(&Crypto::new_secret([12u8; 32]));
		let wrong_migrate_call =
			RuntimeCall::People(indiv_pallet_people::Call::migrate_included_key {
				new_key: key_a_another,
			});
		let uxt = build_signed_as_personal_id_ext(&account_a, wrong_migrate_call);
		let result = Executive::apply_extrinsic(uxt);
		assert!(matches!(
			result,
			Ok(Err(DispatchError::Module(ModuleError { error, .. })))
			if error[0] == indiv_pallet_people::Error::<Runtime>::InvalidKeyMigration.encode()[0]
		));
	});
}

#[test]
fn key_migration_included_in_ring() {
	new_test_ext().execute_with(|| {
		// ═══════════════════════════════════════════════════════════════════════════
		// Setup - Creates people included in a ring
		// ═══════════════════════════════════════════════════════════════════════════

		// Person A
		let secret_a = Crypto::new_secret([10u8; 32]);
		let key_a = Crypto::member_from_secret(&secret_a);

		// Person B
		let secret_b = Crypto::new_secret([20u8; 32]);
		let key_b = Crypto::member_from_secret(&secret_b);

		// Person C
		let secret_c = Crypto::new_secret([30u8; 32]);
		let key_c = Crypto::member_from_secret(&secret_c);

		// More people to ensure a ring gets built
		let mut people_vec = vec![(0, key_a.clone()), (1, key_b.clone()), (2, key_c.clone())];
		let mut next_id = 3u64;
		for i in 0..10 {
			let secret = Crypto::new_secret([40u8 + i as u8; 32]);
			let key = Crypto::member_from_secret(&secret);
			people_vec.push((next_id, key));
			next_id += 1;
		}

		// Recognize personhood of all these people
		DummyDim::reserve_ids(RuntimeOrigin::root(), people_vec.len() as u32).unwrap();
		DummyDim::recognize_personhood(RuntimeOrigin::root(), people_vec.try_into().unwrap())
			.unwrap();

		let person_a_id = 0u64;
		let person_b_id = 1u64;
		let person_c_id = 2u64;

		// Set personal ID for these people
		let account_a = sr25519::Pair::from_seed(&[10u8; 32]);
		let account_b = sr25519::Pair::from_seed(&[20u8; 32]);
		let account_c = sr25519::Pair::from_seed(&[30u8; 32]);

		// Link accounts to personal IDs
		exec_set_personal_id_account(&secret_a, person_a_id, &account_a);
		exec_set_personal_id_account(&secret_b, person_b_id, &account_b);
		exec_set_personal_id_account(&secret_c, person_c_id, &account_c);

		// Fund accounts
		Balances::set_balance(&pair_to_account_id(&account_a), 1_000_000_000_000);
		Balances::set_balance(&pair_to_account_id(&account_b), 1_000_000_000_000);
		Balances::set_balance(&pair_to_account_id(&account_c), 1_000_000_000_000);

		// Advancing blocks to trigger the hook that runs ring building
		advance_block();

		// All people are included in rings now
		let record_a = indiv_pallet_people::People::<Runtime>::get(person_a_id).unwrap();
		assert!(matches!(record_a.position, indiv_pallet_people::RingPosition::Included { .. }));

		// Person C gets suspended
		assert_ok!(DummyDim::start_mutation_session(RuntimeOrigin::root()));
		DummyDim::suspend_personhood(RuntimeOrigin::root(), vec![person_c_id].try_into().unwrap())
			.unwrap();
		assert_ok!(DummyDim::end_mutation_session(RuntimeOrigin::root()));

		// Advance blocks to process suspension
		advance_block();

		let record_c = indiv_pallet_people::People::<Runtime>::get(person_c_id).unwrap();
		assert!(matches!(record_c.position, indiv_pallet_people::RingPosition::Suspended));

		// ═══════════════════════════════════════════════════════════════════════════
		// Key Migration - success case
		// ═══════════════════════════════════════════════════════════════════════════

		// Mutation session starts
		assert_ok!(DummyDim::start_mutation_session(RuntimeOrigin::root()));

		// Person A migrates their included in a ring key
		let secret_a_new = Crypto::new_secret([11u8; 32]);
		let key_a_new = Crypto::member_from_secret(&secret_a_new);

		let migrate_call = RuntimeCall::People(indiv_pallet_people::Call::migrate_included_key {
			new_key: key_a_new.clone(),
		});
		exec_signed_as_personal_id(&account_a, migrate_call);

		// Queued but not yet migrated
		assert_eq!(
			indiv_pallet_people::KeyMigrationQueue::<Runtime>::get(person_a_id).unwrap(),
			key_a_new,
		);

		// Mutation Session ends
		assert_ok!(DummyDim::end_mutation_session(RuntimeOrigin::root()));

		// Advancing blocks to trigger a hook which executes the migration
		advance_block();

		// Person's A record now contains the new key
		let record_a_migrated = indiv_pallet_people::People::<Runtime>::get(person_a_id).unwrap();
		assert_eq!(record_a_migrated.key, key_a_new);

		// Advancing blocks to reinclude Person A in a ring
		advance_block();

		let record_a_final = indiv_pallet_people::People::<Runtime>::get(person_a_id).unwrap();
		assert!(matches!(
			record_a_final.position,
			indiv_pallet_people::RingPosition::Included { .. }
		));

		// ═══════════════════════════════════════════════════════════════════════════
		// Key Migration - Restrictions
		// ═══════════════════════════════════════════════════════════════════════════

		// Cannot migrate to a known key
		assert_ok!(DummyDim::start_mutation_session(RuntimeOrigin::root()));

		let migrate_to_b_call =
			RuntimeCall::People(indiv_pallet_people::Call::migrate_included_key {
				new_key: key_b.clone(),
			});
		let uxt = build_signed_as_personal_id_ext(&account_a, migrate_to_b_call);
		let result = Executive::apply_extrinsic(uxt);
		assert!(matches!(
			result,
			Ok(Err(DispatchError::Module(ModuleError { error, .. })))
			if error[0] == indiv_pallet_people::Error::<Runtime>::KeyAlreadyInUse.encode()[0]
		));

		assert_ok!(DummyDim::end_mutation_session(RuntimeOrigin::root()));
		advance_block();

		// Cannot migrate to the same key
		assert_ok!(DummyDim::start_mutation_session(RuntimeOrigin::root()));

		let migrate_to_same_call =
			RuntimeCall::People(indiv_pallet_people::Call::migrate_included_key {
				new_key: key_a_new.clone(),
			});
		let uxt = build_signed_as_personal_id_ext(&account_a, migrate_to_same_call);
		let result = Executive::apply_extrinsic(uxt);
		assert!(matches!(
			result,
			Ok(Err(DispatchError::Module(ModuleError { error, .. })))
			if error[0] == indiv_pallet_people::Error::<Runtime>::KeyAlreadyInUse.encode()[0]
		));

		assert_ok!(DummyDim::end_mutation_session(RuntimeOrigin::root()));
		advance_block();

		// Wrong migration type
		let key_a_new_2 = Crypto::member_from_secret(&Crypto::new_secret([51u8; 32]));
		let wrong_migrate_call =
			RuntimeCall::People(indiv_pallet_people::Call::migrate_onboarding_key {
				new_key: key_a_new_2,
			});
		let uxt = build_signed_as_personal_id_ext(&account_a, wrong_migrate_call);
		let result = Executive::apply_extrinsic(uxt);
		assert!(matches!(
			result,
			Ok(Err(DispatchError::Module(ModuleError { error, .. })))
			if error[0] == indiv_pallet_people::Error::<Runtime>::InvalidKeyMigration.encode()[0]
		));

		// Suspended person cannot migrate
		assert_ok!(DummyDim::start_mutation_session(RuntimeOrigin::root()));

		let key_c_new = Crypto::member_from_secret(&Crypto::new_secret([31u8; 32]));
		let suspended_migrate_call =
			RuntimeCall::People(indiv_pallet_people::Call::migrate_included_key {
				new_key: key_c_new,
			});
		let uxt = build_signed_as_personal_id_ext(&account_c, suspended_migrate_call);
		let result = Executive::apply_extrinsic(uxt);
		assert_eq!(result, Err(TransactionValidityError::Invalid(InvalidTransaction::BadSigner)));

		assert_ok!(DummyDim::end_mutation_session(RuntimeOrigin::root()));
		advance_block();
	});
}

#[test]
fn key_migration_with_proof_generation() {
	new_test_ext().execute_with(|| {
		// ═══════════════════════════════════════════════════════════════════════════
		// Setup - Creates a person included in a ring
		// ═══════════════════════════════════════════════════════════════════════════

		let secret = Crypto::new_secret([10u8; 32]);
		let key = Crypto::member_from_secret(&secret);

		// More people to ensure a ring gets built
		let mut people_vec = vec![(0, key.clone())];
		for i in 1..10 {
			let mut seed = [0u8; 32];
			seed[0] = 100 + i as u8;
			let secret = Crypto::new_secret(seed);
			let key = Crypto::member_from_secret(&secret);
			people_vec.push((i as u64, key));
		}

		// Recognize personhood of all these people
		DummyDim::reserve_ids(RuntimeOrigin::root(), people_vec.len() as u32).unwrap();
		DummyDim::recognize_personhood(RuntimeOrigin::root(), people_vec.try_into().unwrap())
			.unwrap();

		let person_id = 0u64;

		// Setting personal ID
		let account = sr25519::Pair::from_seed(&[10u8; 32]);
		exec_set_personal_id_account(&secret, person_id, &account);
		Balances::set_balance(&pair_to_account_id(&account), 1_000_000_000_000);

		// Advancing blocks to trigger the hook that runs ring building
		advance_block();

		let record = indiv_pallet_people::People::<Runtime>::get(person_id).unwrap();
		let ring_index = match record.position {
			indiv_pallet_people::RingPosition::Included { ring_index, .. } => ring_index,
			_ => panic!("Person should be included in ring"),
		};

		// The key is in the ring
		let ring_members = indiv_pallet_people::RingKeys::<Runtime>::get(ring_index);
		assert!(ring_members.contains(&key));

		// ═══════════════════════════════════════════════════════════════════════════
		// Setting up alias account
		// ═══════════════════════════════════════════════════════════════════════════

		// Set up an alias account
		let context = SCORE_CONTEXT;
		let alias_account = sr25519::Pair::from_seed(&[99u8; 32]);
		let set_alias_call = RuntimeCall::People(indiv_pallet_people::Call::set_alias_account {
			account: pair_to_account_id(&alias_account),
			call_valid_at: frame_system::Pallet::<Runtime>::block_number(),
		});
		exec_as_alias_with_proof(&secret, context, set_alias_call);

		// Alias is successfully linked to the account
		assert!(indiv_pallet_people::AccountToAlias::<Runtime>::get(pair_to_account_id(
			&alias_account
		))
		.is_some());

		// ═══════════════════════════════════════════════════════════════════════════
		// Key migration
		// ═══════════════════════════════════════════════════════════════════════════

		let secret_new = Crypto::new_secret([11u8; 32]);
		let key_new = Crypto::member_from_secret(&secret_new);

		assert_ok!(DummyDim::start_mutation_session(RuntimeOrigin::root()));

		let migrate_call = RuntimeCall::People(indiv_pallet_people::Call::migrate_included_key {
			new_key: key_new.clone(),
		});
		exec_signed_as_personal_id(&account, migrate_call);

		assert_ok!(DummyDim::end_mutation_session(RuntimeOrigin::root()));
		advance_block();

		// ═══════════════════════════════════════════════════════════════════════════
		// Verifying state after key migration
		// ═══════════════════════════════════════════════════════════════════════════

		let record_after = indiv_pallet_people::People::<Runtime>::get(person_id).unwrap();
		assert_eq!(record_after.key, key_new);

		// Old key is removed from ring members
		let ring_members_after = indiv_pallet_people::RingKeys::<Runtime>::get(ring_index);
		assert!(!ring_members_after.contains(&key));

		// Old key can no longer open a commitment in the updated ring
		let result_after = Crypto::open(&key, ring_members_after.iter().cloned());
		assert!(result_after.is_err());

		// ═══════════════════════════════════════════════════════════════════════════
		// Re-include person and verify new key works
		// ═══════════════════════════════════════════════════════════════════════════

		// Advancing blocks to have a ring built with the migrated key
		advance_block();

		let record_final = indiv_pallet_people::People::<Runtime>::get(person_id).unwrap();
		let new_ring_index = match record_final.position {
			indiv_pallet_people::RingPosition::Included { ring_index, .. } => ring_index,
			_ => panic!("Person should be re-included"),
		};

		let new_ring_members = indiv_pallet_people::RingKeys::<Runtime>::get(new_ring_index);
		assert!(new_ring_members.contains(&key_new));

		// Set up new alias account with the new key
		let new_alias_account = sr25519::Pair::from_seed(&[98u8; 32]);
		let set_new_alias_call =
			RuntimeCall::People(indiv_pallet_people::Call::set_alias_account {
				account: pair_to_account_id(&new_alias_account),
				call_valid_at: frame_system::Pallet::<Runtime>::block_number(),
			});
		exec_as_alias_with_proof(&secret_new, context, set_new_alias_call);

		// New alias is successfully linked to the account
		assert!(indiv_pallet_people::AccountToAlias::<Runtime>::get(pair_to_account_id(
			&new_alias_account
		))
		.is_some());
	});
}
