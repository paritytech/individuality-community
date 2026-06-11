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

#![cfg(test)]

mod mock;
#[cfg(test)]
mod value_transfer_auth_tests;
#[cfg(test)]
mod value_transfer_xcm_filter_tests;

use codec::{Decode, Encode};
use frame_support::{assert_err, assert_noop, assert_ok};
use frame_system::pallet_prelude::BlockNumberFor;
use indiv_pallet_members::RingRoot;
use indiv_pallet_mob_rule::MOB_CONTEXT;
use indiv_pallet_people::pallet::PEOPLE_MEMBER_IDENTIFIER;
use indiv_pallet_proof_of_ink::{Callbacks, DesignIndex, FamilyKind, InkChoice};
use indiv_support::traits::{
	AddOnlyPeopleTrait, ContextualAlias, InkSpec, Judgement, JudgementContext, PersonalId,
	RevisedContextualAlias, RingPosition, Statement, StatementOracle, Truth, RI_ZERO,
};
use mock::*;
use sp_runtime::{
	testing::{TestSignature as TicketSignature, TestSignature},
	transaction_validity::InvalidTransaction,
};
use std::collections::BTreeSet;
use verifiable::{ring::bandersnatch::BandersnatchVrfVerifiable, GenerateVerifiable};

type EncodedPublicKey = <BandersnatchVrfVerifiable as GenerateVerifiable>::Member;
type SecretKey = <BandersnatchVrfVerifiable as GenerateVerifiable>::Secret;

#[test]
fn everything_works() {
	new_test_ext().execute_with(|| {
		// Setup - create the people collection and initialize chunks first
		initialize_chunks();
		create_people_collection();
		set_people_onboarding_size(1);
		// Add a design family for tattoos. This is just a simple elective one with 10 designs.
		assert_ok!(ProofOfInk::add_design_family(
			root(),
			0,
			FamilyKind::Designed { count: 10 },
			[0u8; 32]
		));

		let reward_value = 1000;
		set_up_reimbursements_and_pot(reward_value);

		// First participant applies, commits to a design and submits evidence...
		frame_system::Pallet::<Test>::inc_providers(&1);
		assert_ok!(exec_signed_tx(1, ProofOfInkCall::apply {}));
		assert_ok!(exec_signed_tx(
			1,
			ProofOfInkCall::commit { choice: InkChoice::DesignedElective(0, 0), require_id: None }
		));

		// Evidence is uploaded to Bulletin chain here; hash of evidence compilation is [1; 32].

		assert_ok!(exec_signed_tx(1, ProofOfInkCall::submit_evidence { evidence: [1; 32] }));

		// We can see that Mob Rule has been triggered and a case has been created.
		assert_eq!(indiv_pallet_mob_rule::CaseCount::<Test>::get(), 1);
		assert!(indiv_pallet_mob_rule::OpenCases::<Test>::get(0).is_some());

		// We force the outcome by governance.
		assert_ok!(MobRule::intervene(root(), 0, Judgement::Truth(Truth::True)));

		// First person is now a proven - they register a special key and we bake the first root.
		let secret_key = BandersnatchVrfVerifiable::new_secret([0u8; 32]);
		let pub_key = BandersnatchVrfVerifiable::member_from_secret(&secret_key);
		let signer = 1u64;
		let proof_of_ownership = {
			let mut m = b"pop register using".to_vec();
			m.extend_from_slice(&signer.encode());
			BandersnatchVrfVerifiable::sign(&secret_key, &m[..]).unwrap()
		};
		assert_ok!(exec_signed_tx(
			signer,
			ProofOfInkCall::register_non_referred {
				key: pub_key,
				destination: 10,
				proof_of_ownership,
			}
		));
		build_rings();

		// Registered person should receive direct rewards.
		assert_eq!(Balances::free_balance(10), reward_value.saturating_mul(2));
		assert_eq!(Balances::free_balance(11), 0);

		let init_proof = |member, ring_index| {
			// For this we need to know all the members. This proof generation would be done
			// off-chain (but on some device connected to the chain) so we need not be too concerned
			// about the iteration.
			// This code will need to be rewritten to work in JS (or whatever language the UI uses)
			// and a light-client.
			use indiv_pallet_members::{RingKeys, RingKeysStatus};
			use verifiable::ring::RingDomainSize;
			let all_keys = RingKeys::<Test>::get((PEOPLE_MEMBER_IDENTIFIER, ring_index, 0u32));
			let all_keys_status = RingKeysStatus::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, ring_index);
			let keys = all_keys.into_iter().take(all_keys_status.included as usize);
			BandersnatchVrfVerifiable::open(RingDomainSize::Domain11, member, keys)
				.expect("expected key to be included in the built ring")
		};

		// Look up the member's position to get the ring index.
		let position =
			indiv_pallet_members::Members::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, pub_key)
				.expect("member not found");
		let RingPosition::Included { ring_index, .. } = position else {
			panic!("member isn't included in a ring")
		};

		// The thing we want to do as an *anonymous* member under the Mob Rules context is set an
		// associated context-sensible account. This is a bit like a proxy account, except it
		// proxies to a personal alias, not an account and it will only work in this specific
		// context.
		let call: RuntimeCall =
			indiv_pallet_people::Call::<Test>::set_alias_account { account: 20, call_valid_at: 0 }
				.into();

		// NOTE: The implicit are null so we can ignore them to generate the proof.
		let tx_ext_part = (
			indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
			frame_system::CheckNonce::<Test>::from(0),
		);

		// We now make a proof that we're a member. This is a two-part affair - the first part
		// (called the "commitment") collects together all the keys which made the latest root and
		// thus generally needs access to the chain's state to know what these keys are. The second
		// part combines the output of this with the secret key and the message we want to sign in
		// order to create the final proof. This part can happen on an offline device as it doesn't
		// require much external data or chain-state. In this case our message is just the call we
		// want to execute.
		let commitment = init_proof(&pub_key, ring_index);
		let (proof, mob_1) = BandersnatchVrfVerifiable::create(
			commitment,
			&secret_key,
			&MOB_CONTEXT,
			&(&(EXTENSION_VERSION, &call), tx_ext_part).using_encoded(sp_io::hashing::blake2_256),
		)
		.unwrap();

		assert_ok!(exec_proof_as_alias_tx(proof, ring_index, MOB_CONTEXT, call));
		assert_eq!(
			indiv_pallet_people::AccountToAlias::<Test>::get(20),
			Some(RevisedContextualAlias {
				revision: 0,
				ring: ring_index,
				ca: ContextualAlias { context: MOB_CONTEXT, alias: mob_1 },
			}),
		);

		// Since it is now associated, we can use `under_alias` to execute any further calls for us
		// in the Mob Rules context. We start by altering the proxy account to be 30.
		let call =
			indiv_pallet_people::Call::<Test>::set_alias_account { account: 30, call_valid_at: 0 };
		assert_ok!(exec_as_alias_tx(20, call));
		assert_eq!(
			indiv_pallet_people::AccountToAlias::<Test>::get(30),
			Some(RevisedContextualAlias {
				revision: 0,
				ring: ring_index,
				ca: ContextualAlias { context: MOB_CONTEXT, alias: mob_1 },
			}),
		);
		let call =
			indiv_pallet_people::Call::<Test>::set_alias_account { account: 30, call_valid_at: 0 };
		assert_noop!(exec_as_alias_tx(20, call), InvalidTransaction::BadSigner);
		// And then back again to 20...
		let call =
			indiv_pallet_people::Call::<Test>::set_alias_account { account: 20, call_valid_at: 0 };
		assert_ok!(exec_as_alias_tx(30, call));

		// Setup personal id account.
		let personal_id_account = 11111;
		setup_personal_id_account(0, &secret_key, personal_id_account);

		// Register referral pub key `101` for the recently registered person.
		let call: RuntimeCall =
			indiv_pallet_proof_of_ink::Call::<Test>::set_referral_ticket { ticket: 101 }.into();
		assert_ok!(exec_as_personal_id(personal_id_account, call));

		// 50 is not funded.
		assert_eq!(frame_system::Account::<Test>::get(50).providers, 0);
		assert_eq!(frame_system::Account::<Test>::get(50).sufficients, 0);

		// Mock giving someone the secret key associated with pub key `101`.
		// Use that to apply for personhood with account 50.
		let referral_sig = TicketSignature(101, 50u64.encode());
		assert_ok!(exec_as_apply_with_sig(
			50,
			ProofOfInkCall::apply_with_signature {
				referrer: 0,
				signature: referral_sig,
				ticket: 101
			}
		));

		// 50 is funded.
		assert_eq!(frame_system::Account::<Test>::get(50).providers, 0);
		assert_eq!(frame_system::Account::<Test>::get(50).sufficients, 1);

		// Person 0 referred account 50.
		assert!(matches!(
			indiv_pallet_proof_of_ink::Candidates::<Test>::get(50),
			Some(indiv_pallet_proof_of_ink::CandidateOf::<Test>::Applied {
				cred: indiv_pallet_proof_of_ink::Credibility::Referred(0),
				..
			})
		));

		// Candidate 50 should be able to complete the process without funded account.
		assert_ok!(exec_as_referred_candidate(
			50,
			ProofOfInkCall::commit { choice: InkChoice::DesignedElective(0, 1), require_id: None }
		));
		assert_ok!(exec_as_referred_candidate(
			50,
			ProofOfInkCall::submit_evidence { evidence: [1; 32] }
		));

		// We can see that Mob Rule has been triggered and a case has been created.
		assert_eq!(indiv_pallet_mob_rule::CaseCount::<Test>::get(), 2);
		assert!(indiv_pallet_mob_rule::OpenCases::<Test>::get(1).is_some());

		// We force the outcome by governance.
		assert_ok!(MobRule::intervene(root(), 1, Judgement::Truth(Truth::True)));

		// 50 is still funded until it registers.
		assert_eq!(frame_system::Account::<Test>::get(50).providers, 0);
		assert_eq!(frame_system::Account::<Test>::get(50).sufficients, 1);

		// Referrer can register pending referral reward.
		let call: RuntimeCall =
			indiv_pallet_proof_of_ink::Call::<Test>::register_successful_referral_reward {
				destination: 12,
			}
			.into();
		assert_ok!(exec_as_personal_id(personal_id_account, call));
		assert_eq!(Balances::free_balance(12), reward_value);

		// Second person is now proven and registers with a destination.
		let secret_key_2 = BandersnatchVrfVerifiable::new_secret([4u8; 32]);
		let pub_key_2 = BandersnatchVrfVerifiable::member_from_secret(&secret_key_2);
		let signer = 50u64;
		let proof_of_ownership = {
			let mut m = b"pop register using".to_vec();
			m.extend_from_slice(&signer.encode());
			BandersnatchVrfVerifiable::sign(&secret_key_2, &m[..]).unwrap()
		};
		assert_ok!(exec_as_referred_candidate(
			signer,
			ProofOfInkCall::register_referred {
				key: pub_key_2,
				destination: 13,
				proof_of_ownership,
			}
		));

		// 50 is back to not funded.
		assert_eq!(frame_system::Account::<Test>::get(50).providers, 0);
		assert_eq!(frame_system::Account::<Test>::get(50).sufficients, 0);
		assert_eq!(Balances::free_balance(13), reward_value);
	});
}

#[test]
fn voting_works() {
	new_test_ext().execute_with(|| {
		// Setup - create the people collection and initialize chunks first
		initialize_chunks();
		create_people_collection();
		set_people_onboarding_size(1);

		let (alice_account_id, alice_alias_account_id, alice_personal_id): (_, _, PersonalId) =
			(1u64, 11u64, People::reserve_new_id());
		let (alice_b_pub, alice_b_secret) = account_id_to_bandersnatch_key_pair(alice_account_id);

		indiv_pallet_people::pallet::Pallet::<Test>::recognize_personhood(
			alice_personal_id,
			Some(alice_b_pub),
		)
		.unwrap();

		// Refresh root push alice's key to make it usable for her
		build_rings();

		// Register alias for account 10 in MOBRULE_CONTEXT
		assert_ok!(register_alias(
			(&alice_b_pub, &alice_b_secret),
			(alice_account_id, alice_alias_account_id),
			MOB_CONTEXT,
		));

		// Create a statement which can be voted on via mob rule
		let mob_rule_case_index = indiv_pallet_mob_rule::Pallet::<Test>::judge_statement(
			Statement::ProofOfInk {
				design: InkSpec::ProceduralPersonal(0, 0),
				evidence: [0; 32],
				probable_acceptable: false,
			},
			JudgementContext::truncate_from([0u8; 32].encode()),
			indiv_pallet_proof_of_ink::Call::<Test>::judged(),
		)
		.unwrap();

		let vote: RuntimeCall = indiv_pallet_mob_rule::Call::<Test>::vote {
			case_index: mob_rule_case_index,
			opinion: Judgement::Truth(Truth::True),
		}
		.into();

		// Voting with alias account should work.
		assert_ok!(exec_as_alias_tx(alice_alias_account_id, vote.clone()));

		// Voting with non-alias account should fail.
		assert_noop!(
			exec_as_alias_tx(alice_account_id, vote.clone()),
			InvalidTransaction::BadSigner,
		);

		// Registering same alias for another account should fail
		assert_noop!(
			register_alias(
				(&alice_b_pub, &alice_b_secret),
				(alice_account_id, alice_alias_account_id),
				MOCK_CONTEXT,
			)
			.map_err(|e| e.unwrap_dispatch().error),
			indiv_pallet_people::Error::<Test>::AccountInUse,
		);

		let alias_alias_account_id_two = 99u64;
		assert_ok!(register_alias(
			(&alice_b_pub, &alice_b_secret),
			(alice_account_id, alias_alias_account_id_two),
			MOCK_CONTEXT,
		));

		// Voting with valid alias but wrong Context for Mob Rule should fail
		// Not no-op: the nonce is handled
		assert_eq!(
			exec_as_alias_tx(alias_alias_account_id_two, vote.clone())
				.unwrap_err()
				.unwrap_dispatch()
				.error,
			sp_runtime::DispatchError::BadOrigin,
		);
	});
}

#[test]
fn rebuilding_ring_works() {
	new_test_ext().execute_with(|| {
		// Setup - create the people collection and initialize chunks first
		initialize_chunks();
		create_people_collection();
		set_people_onboarding_size(1);
		// Check if Root is not initialized
		assert_eq!(
			indiv_pallet_members::Root::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, RI_ZERO),
			None
		);

		let onboard_size = 10;

		// Prepare 20 people
		let account_ids = 1u64..=20u64;

		// Reserve some id to enforce account_id != personal_id
		for _ in 0..*account_ids.end() {
			People::reserve_new_id();
		}

		let personal_ids: Vec<u64> =
			account_ids.clone().map(|_| People::reserve_new_id()).collect();
		let alias_account_ids: Vec<u64> = account_ids.clone().map(|id| id * 100u64).collect();

		let key_pairs: Vec<(EncodedPublicKey, SecretKey)> =
			account_ids.clone().map(account_id_to_bandersnatch_key_pair).collect();

		// recognize_personhood onboard_size of them
		for (personal_id, (pub_key, ..)) in
			personal_ids.iter().zip(key_pairs.iter()).take(onboard_size)
		{
			indiv_pallet_people::pallet::Pallet::<Test>::recognize_personhood(
				*personal_id,
				Some(*pub_key),
			)
			.unwrap();
			assert_eq!(indiv_pallet_people::Keys::<Test>::get(pub_key).unwrap(), *personal_id);
		}

		// Baking should succeed.
		build_rings();

		// Check Root revision number
		let RingRoot { revision, .. } =
			indiv_pallet_members::Root::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, RI_ZERO).unwrap();
		assert_eq!(revision, 0);

		// Check if all keys are present in the root
		let all_keys =
			indiv_pallet_members::RingKeys::<Test>::get((PEOPLE_MEMBER_IDENTIFIER, RI_ZERO, 0u32));
		let all_keys_status =
			indiv_pallet_members::RingKeysStatus::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, RI_ZERO);
		let set_of_encoded_keys: BTreeSet<_> = all_keys
			.into_iter()
			.take(all_keys_status.included as usize)
			.map(|pub_key| Encode::encode(&pub_key))
			.collect();

		let all_in_set = key_pairs
			.iter()
			.take(onboard_size)
			.map(|(pub_key, ..)| Encode::encode(&pub_key))
			.all(|encoded_pub_key| set_of_encoded_keys.contains(&encoded_pub_key));

		assert!(all_in_set);

		// recognize_personhood for remaining prepared accounts
		for (personal_id, (pub_key, ..)) in
			personal_ids.iter().zip(key_pairs.iter()).skip(onboard_size)
		{
			indiv_pallet_people::pallet::Pallet::<Test>::recognize_personhood(
				*personal_id,
				Some(*pub_key),
			)
			.unwrap();
			assert_eq!(indiv_pallet_people::Keys::<Test>::get(pub_key).unwrap(), *personal_id);
		}

		// Baking new revision
		build_rings();

		// Check Root pages amount and revision number
		let RingRoot { revision, .. } =
			indiv_pallet_members::Root::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, RI_ZERO).unwrap();
		assert_eq!(revision, 1);

		// Check if all keys are present in the root again
		let all_keys =
			indiv_pallet_members::RingKeys::<Test>::get((PEOPLE_MEMBER_IDENTIFIER, RI_ZERO, 0u32));
		let all_keys_status =
			indiv_pallet_members::RingKeysStatus::<Test>::get(PEOPLE_MEMBER_IDENTIFIER, RI_ZERO);
		let set_of_encoded_keys: BTreeSet<_> = all_keys
			.into_iter()
			.take(all_keys_status.included as usize)
			.map(|pub_key| Encode::encode(&pub_key))
			.collect();

		let all_in_set = key_pairs
			.iter()
			.map(|(pub_key, ..)| Encode::encode(&pub_key))
			.all(|encoded_pub_key| set_of_encoded_keys.contains(&encoded_pub_key));

		assert!(all_in_set);
		assert_eq!(set_of_encoded_keys.len(), personal_ids.len());

		// all keys can register an alias account
		key_pairs.iter().zip(alias_account_ids.iter()).for_each(
			|((pub_key, secret_key), alias_account_id)| {
				assert_ok!(register_alias(
					(pub_key, secret_key),
					(*alias_account_id, *alias_account_id),
					MOB_CONTEXT,
				));
			},
		);

		// Create a statement which can be voted on via mob rule
		let case_index = indiv_pallet_mob_rule::Pallet::<Test>::judge_statement(
			Statement::ProofOfInk {
				design: InkSpec::ProceduralPersonal(0, 0),
				evidence: [0; 32],
				probable_acceptable: false,
			},
			JudgementContext::truncate_from([0u8; 32].encode()),
			indiv_pallet_proof_of_ink::Call::<Test>::judged(),
		)
		.unwrap();

		let vote: RuntimeCall = indiv_pallet_mob_rule::Call::<Test>::vote {
			case_index,
			opinion: Judgement::Truth(Truth::True),
		}
		.into();

		// alias account ids can be used `under_alias`
		key_pairs.iter().for_each(|(public_key, secret_key)| {
			let account = u64::decode(&mut &public_key.encode()[..]).unwrap();
			setup_alias_account(public_key, secret_key, MOB_CONTEXT, account);

			assert_ok!(exec_as_alias_tx(account, vote.clone()));
		});
	});
}

#[test]
fn proof_of_ink_with_timeout() {
	new_test_ext().execute_with(|| {
		// Setup - create the people collection and initialize chunks first
		initialize_chunks();
		create_people_collection();
		set_people_onboarding_size(1);
		indiv_pallet_proof_of_ink::Configuration::<Test>::set(
			indiv_pallet_proof_of_ink::ConfigRecord::<BlockNumberFor<Test>> {
				timeout: 1,
				..Default::default()
			},
		);

		assert_ok!(ProofOfInk::add_design_family(
			root(),
			0,
			FamilyKind::Designed { count: 10 },
			[0u8; 32]
		));

		set_up_reimbursements_and_pot(1000);

		// User 1 applies and commits to a design
		frame_system::Pallet::<Test>::inc_providers(&1);
		assert_ok!(exec_signed_tx(1, ProofOfInkCall::apply {}));
		assert_ok!(exec_signed_tx(
			1,
			ProofOfInkCall::commit { choice: InkChoice::DesignedElective(0, 0), require_id: None }
		));

		// At this point the candidate's status is 'selected'
		// and timeout can only be called on 'selected' candidates

		assert_ok!(exec_signed_tx(1, ProofOfInkCall::submit_evidence { evidence: [1; 32] }));

		// Once the prover submitted evidence, his status remains 'selected'.
		// The prover is not yet approved by the mob.

		// Timeout called earlier than config.timeout and called by a different user
		frame_system::Pallet::<Test>::inc_providers(&2);

		assert_err!(
			exec_signed_tx(2, ProofOfInkCall::timeout { account: 1 })
				.map_err(|e| e.unwrap_dispatch().error),
			indiv_pallet_proof_of_ink::Error::<Test>::TooEarly
		);

		// Timeout called later than config.timeout and called by a different user
		mock::advance_to(2);
		assert_ok!(exec_signed_tx(2, ProofOfInkCall::timeout { account: 1 }));

		// User 2 onboards
		let keys = onboard_a_person(2);

		// User 2 set its personal id account
		let user_2_personal_id_account = 22222;
		setup_personal_id_account(1, &keys.secret_key, user_2_personal_id_account);

		// User 2 creates a referral ticket
		let referral_ticket = 101u64;
		let call: RuntimeCall = indiv_pallet_proof_of_ink::Call::<Test>::set_referral_ticket {
			ticket: referral_ticket,
		}
		.into();

		// id == 1 because he's the 2nd person to apply
		assert!(indiv_pallet_proof_of_ink::People::<Test>::contains_key(1));
		assert_ok!(exec_as_personal_id(user_2_personal_id_account, call));

		// User 3 applies with a referral and commits to a design
		assert_ok!(exec_as_apply_with_sig(
			3,
			ProofOfInkCall::apply_with_signature {
				referrer: 1,
				signature: TestSignature(referral_ticket, 3u64.encode()),
				ticket: referral_ticket
			}
		));
		assert_ok!(exec_signed_tx(
			3,
			ProofOfInkCall::commit { choice: InkChoice::DesignedElective(0, 5), require_id: None }
		));
		assert!(indiv_pallet_proof_of_ink::Candidates::<Test>::contains_key(3));

		// User 1, who referred user 3 should have no bad referrals at this point
		assert_eq!(indiv_pallet_proof_of_ink::People::<Test>::get(1).unwrap().bad_referrals, 0);

		// Timeout called on user 3
		mock::advance_to(4);
		assert_ok!(exec_signed_tx(2, ProofOfInkCall::timeout { account: 3 }));
		assert!(!indiv_pallet_proof_of_ink::Candidates::<Test>::contains_key(3));

		// Bad referral counter of the referrer should be incremented
		assert_eq!(indiv_pallet_proof_of_ink::People::<Test>::get(1).unwrap().bad_referrals, 1);
	})
}

struct PersonalKeys {
	secret_key: SecretKey,
	_reward_secret_key: SecretKey,
	_self_ref_reward_secret_key: SecretKey,
	_pub_key: EncodedPublicKey,
	_reward_pub_key: EncodedPublicKey,
	_self_ref_reward_pub_key: EncodedPublicKey,
}

fn onboard_a_person(who: u64) -> PersonalKeys {
	frame_system::Pallet::<Test>::inc_providers(&who);

	// application without a referral
	assert_ok!(exec_signed_tx(who, ProofOfInkCall::apply {}));

	assert_ok!(exec_signed_tx(
		who,
		ProofOfInkCall::commit {
			choice: InkChoice::DesignedElective(0, who as DesignIndex),
			require_id: None
		}
	));

	assert_ok!(exec_signed_tx(who, ProofOfInkCall::submit_evidence { evidence: [1; 32] }));

	// approval of the onboarding case of the person
	assert_ok!(MobRule::intervene(
		root(),
		indiv_pallet_mob_rule::CaseCount::<Test>::get() - 1,
		Judgement::Truth(Truth::True)
	));

	// final onboarding steps - keys generation and registration with them
	let mut generator = mock::BandersnatchKeyPairGenerator::new();
	let (secret_key, pub_key) = generator.generate_key_pair();
	let (reward_secret_key, reward_pub_key) = generator.generate_key_pair();
	let (self_ref_reward_secret_key, self_ref_reward_pub_key) = generator.generate_key_pair();
	let proof_of_ownership = {
		let mut m = b"pop register using".to_vec();
		m.extend_from_slice(&who.encode());
		BandersnatchVrfVerifiable::sign(&secret_key, &m[..]).unwrap()
	};
	assert_ok!(exec_signed_tx(
		who,
		ProofOfInkCall::register_non_referred {
			key: pub_key,
			destination: who,
			proof_of_ownership,
		}
	));

	// then ring baking
	build_rings();

	PersonalKeys {
		secret_key,
		_reward_secret_key: reward_secret_key,
		_self_ref_reward_secret_key: self_ref_reward_secret_key,
		_pub_key: pub_key,
		_reward_pub_key: reward_pub_key,
		_self_ref_reward_pub_key: self_ref_reward_pub_key,
	}
}

#[test]
fn replay_protection_for_identity() {
	new_test_ext().execute_with(|| {
		// Setup - create the people collection first
		create_people_collection();

		// Setup alice as a member.
		let alice_sec = BandersnatchVrfVerifiable::new_secret([1u8; 32]);
		let alice_pub = BandersnatchVrfVerifiable::member_from_secret(&alice_sec);
		let alice_index = People::reserve_new_id();
		People::recognize_personhood(alice_index, Some(alice_pub)).unwrap();

		let generate_setup_account_tx_ext_for_call = |call: RuntimeCall| {
			let other_tx_ext = (
				indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
				frame_system::CheckNonce::<Test>::from(0),
			);
			// Here we simply ignore implicit as they are null.
			let msg = (&EXTENSION_VERSION, &call, &other_tx_ext)
				.using_encoded(sp_io::hashing::blake2_256);
			let signature = BandersnatchVrfVerifiable::sign(&alice_sec, &msg).unwrap();
			(
				frame_system::AuthorizeCall::new(),
				indiv_pallet_people::extension::AsPerson::<Test>::new(Some(
					indiv_pallet_people::extension::AsPersonInfo::AsPersonalIdentityWithProof(
						signature,
						alice_index,
					),
				)),
				other_tx_ext.0,
				other_tx_ext.1,
			)
		};

		// Transaction 1: Alice sets its personal id account to 10.
		let call = RuntimeCall::People(PeopleCall::set_personal_id_account {
			account: 10,
			call_valid_at: System::block_number(),
		});
		let tx_ext = generate_setup_account_tx_ext_for_call(call.clone());
		assert_ok!(exec_tx(None, tx_ext.clone(), call.clone()));
		assert_eq!(
			indiv_pallet_people::People::<Test>::get(alice_index).unwrap().account,
			Some(10)
		);
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(10), Some(alice_index));

		// Somebody tries to replay the transaction, it must fail, replay is protected.
		assert_noop!(exec_tx(None, tx_ext.clone(), call.clone()), InvalidTransaction::Stale);

		// Transaction 2: Alice sets its personal id account to 11, with call valid only in the
		// future.
		let call_2 = RuntimeCall::People(PeopleCall::set_personal_id_account {
			account: 11,
			call_valid_at: System::block_number() + 1,
		});
		let tx_ext_2 = generate_setup_account_tx_ext_for_call(call_2.clone());

		// Transaction 2 is not valid yet.
		assert_noop!(exec_tx(None, tx_ext_2.clone(), call_2.clone()), InvalidTransaction::Future);
		assert_eq!(
			indiv_pallet_people::People::<Test>::get(alice_index).unwrap().account,
			Some(10)
		);
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(10), Some(alice_index));
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(11), None);

		// Advance some time. Transaction 1 is still valid, transaction 2 becomes valid.
		mock::advance_by(People::account_setup_time_tolerance());

		// Transaction 2 is now valid.
		assert_ok!(exec_tx(None, tx_ext_2.clone(), call_2.clone()));
		assert_eq!(
			indiv_pallet_people::People::<Test>::get(alice_index).unwrap().account,
			Some(11)
		);
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(11), Some(alice_index));
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(10), None);

		// Somebody replays the transaction 1, it must succeed. It is within time tolerance.
		assert_ok!(exec_tx(None, tx_ext.clone(), call.clone()));
		assert_eq!(
			indiv_pallet_people::People::<Test>::get(alice_index).unwrap().account,
			Some(10)
		);
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(10), Some(alice_index));
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(11), None);

		// Replay the transaction 2.
		assert_ok!(exec_tx(None, tx_ext_2.clone(), call_2.clone()));
		assert_eq!(
			indiv_pallet_people::People::<Test>::get(alice_index).unwrap().account,
			Some(11)
		);
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(11), Some(alice_index));
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(10), None);

		// Advance some time, Now time tolerance is exceeded for transaction 1.
		mock::advance_by(1);

		// Somebody replays the first transaction, it is invalid.
		assert_noop!(exec_tx(None, tx_ext, call), InvalidTransaction::Stale);
		assert_eq!(
			indiv_pallet_people::People::<Test>::get(alice_index).unwrap().account,
			Some(11)
		);
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(11), Some(alice_index));
		assert_eq!(indiv_pallet_people::AccountToPersonalId::<Test>::get(10), None);
	});
}

#[test]
fn replay_protection_for_alias() {
	new_test_ext().execute_with(|| {
		// Setup - create the people collection and initialize chunks first
		initialize_chunks();
		create_people_collection();
		set_people_onboarding_size(1);
		// Setup Alice as a member.
		let alice_sec = BandersnatchVrfVerifiable::new_secret([1u8; 32]);
		let alice_pub = BandersnatchVrfVerifiable::member_from_secret(&alice_sec);
		let alice_index = People::reserve_new_id();
		People::recognize_personhood(alice_index, Some(alice_pub)).unwrap();
		build_rings();

		// Helper to generate a transaction extension (tx_ext) for an alias call.
		// It signs the call (using a VRF commitment/proof) and returns both the tx_ext and the
		// computed alias.
		let generate_alias_tx_ext_for_call = |call: RuntimeCall| {
			use verifiable::ring::RingDomainSize;
			let other_tx_ext = (
				indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Test>::new(None),
				frame_system::CheckNonce::<Test>::from(0),
			);
			// The message is the hash over the extension version, call, and other extensions.
			let msg = (&EXTENSION_VERSION, &call, &other_tx_ext)
				.using_encoded(sp_io::hashing::blake2_256);
			// Open a commitment (using Alice's public key and public data)
			let commitment = BandersnatchVrfVerifiable::open(
				RingDomainSize::Domain11,
				&alice_pub,
				Some(alice_pub).into_iter(),
			)
			.unwrap();
			// Create a VRF proof and compute the alias output from the call message.
			let (proof, alias_value) =
				BandersnatchVrfVerifiable::create(commitment, &alice_sec, &MOB_CONTEXT, &msg)
					.unwrap();
			let alias = ContextualAlias { context: MOB_CONTEXT, alias: alias_value };
			let tx_ext = (
				frame_system::AuthorizeCall::new(),
				indiv_pallet_people::extension::AsPerson::<Test>::new(Some(
					indiv_pallet_people::extension::AsPersonInfo::AsPersonalAliasWithProof(
						proof,
						0,
						MOB_CONTEXT,
					),
				)),
				other_tx_ext.0,
				other_tx_ext.1,
			);
			(tx_ext, alias)
		};

		// --- Transaction 1: set alias account to 10 ---
		// Use the current block number as the valid time.
		let call1 = RuntimeCall::People(PeopleCall::set_alias_account {
			account: 10,
			call_valid_at: System::block_number(),
		});
		let (tx_ext1, alias) = generate_alias_tx_ext_for_call(call1.clone());
		let rev_alias = RevisedContextualAlias { revision: 0, ring: 0, ca: alias.clone() };
		// Execute transaction 1. It should succeed.
		assert_ok!(exec_tx(None, tx_ext1.clone(), call1.clone()));
		assert_eq!(indiv_pallet_people::AliasToAccount::<Test>::get(&alias), Some(10));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(10), Some(rev_alias.clone()));

		// Replay transaction 1 immediately: it must fail (replay protected).
		assert_noop!(exec_tx(None, tx_ext1.clone(), call1.clone()), InvalidTransaction::Stale);

		// --- Transaction 2: set alias account to 11 ---
		// Set its valid time to the future: current block number plus the allowed tolerance + 1.
		let call2 = RuntimeCall::People(PeopleCall::set_alias_account {
			account: 11,
			call_valid_at: System::block_number() + 1,
		});
		let (tx_ext2, _) = generate_alias_tx_ext_for_call(call2.clone());

		// Transaction 2 is too early: it should be rejected as "Future".
		assert_noop!(exec_tx(None, tx_ext2.clone(), call2.clone()), InvalidTransaction::Future);
		// The mapping still reflects transaction 1.
		assert_eq!(indiv_pallet_people::AliasToAccount::<Test>::get(&alias), Some(10));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(10), Some(rev_alias.clone()));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(11), None);

		// Advance time by the allowed tolerance. Now transaction 2 becomes valid.
		mock::advance_by(People::account_setup_time_tolerance());

		// Execute transaction 2. It now succeeds.
		assert_ok!(exec_tx(None, tx_ext2.clone(), call2.clone()));
		assert_eq!(indiv_pallet_people::AliasToAccount::<Test>::get(&alias), Some(11));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(11), Some(rev_alias.clone()));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(10), None);

		// --- Replaying old transactions within tolerance ---
		// Replay transaction 1. Within the tolerance window its replay is allowed.
		assert_ok!(exec_tx(None, tx_ext1.clone(), call1.clone()));
		assert_eq!(indiv_pallet_people::AliasToAccount::<Test>::get(&alias), Some(10));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(10), Some(rev_alias.clone()));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(11), None);

		// Replay transaction 2 to set it back to 11.
		assert_ok!(exec_tx(None, tx_ext2.clone(), call2.clone()));
		assert_eq!(indiv_pallet_people::AliasToAccount::<Test>::get(&alias), Some(11));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(11), Some(rev_alias.clone()));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(10), None);

		// --- Advance time beyond tolerance ---
		// After advancing time a bit more, the time tolerance for transaction 1 is exceeded.
		mock::advance_by(1);

		// Now replaying transaction 1 must be rejected as stale.
		assert_noop!(exec_tx(None, tx_ext1, call1), InvalidTransaction::Stale);
		assert_eq!(indiv_pallet_people::AliasToAccount::<Test>::get(&alias), Some(11));
		assert_eq!(indiv_pallet_people::AccountToAlias::<Test>::get(11), Some(rev_alias));
	});
}
