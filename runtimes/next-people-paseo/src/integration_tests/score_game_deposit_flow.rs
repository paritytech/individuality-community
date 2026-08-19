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

#![allow(clippy::needless_range_loop)]

use super::*;

#[test]
fn configurable_play_deposit_is_held_refunded_and_future_only() {
	new_test_ext().execute_with(|| {
		let bob_pair = Sr25519Keyring::Bob.pair();
		let bob = Sr25519Keyring::Bob.to_account_id();
		let charlie_pair = Sr25519Keyring::Charlie.pair();
		let charlie = Sr25519Keyring::Charlie.to_account_id();
		let hold_reason = RuntimeHoldReason::Game(indiv_pallet_game::HoldReason::PlayDeposit);

		let initial_deposit = PlayDepositDefault::get();
		let updated_deposit = initial_deposit.saturating_mul(3);

		reduce_game_phase_durations();
		let now = (pallet_timestamp::Now::<Runtime>::get() / 1000) as u32;
		let schedule = indiv_pallet_game::GameSchedule {
			game_play_time: now + IN_BETWEEN_GAMES,
			rounds: 1,
			max_group_size: 2,
			airdrops: Default::default(),
		};
		Game::schedule_games(RuntimeOrigin::root(), vec![schedule.clone()]).unwrap();

		let min_funding = updated_deposit
			.saturating_mul(2)
			.saturating_add(Balances::minimum_balance().saturating_mul(2));
		Balances::mint_into(&bob, min_funding).unwrap();
		Balances::mint_into(&charlie, min_funding).unwrap();

		advance_block();
		let signup_bob = indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
			identifier_key: [10u8; 65],
			airdrops: None,
		};
		exec_signed(&bob_pair, signup_bob.into());

		assert_eq!(Balances::balance_on_hold(&hold_reason, &bob), initial_deposit);

		assert_eq!(indiv_pallet_game::PlayDepositAmount::<Runtime>::get(), initial_deposit);
		Game::set_play_deposit(RuntimeOrigin::root(), updated_deposit).unwrap();
		assert_eq!(indiv_pallet_game::PlayDepositAmount::<Runtime>::get(), updated_deposit);

		let signup_charlie = indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
			identifier_key: [11u8; 65],
			airdrops: None,
		};
		exec_signed(&charlie_pair, signup_charlie.into());

		assert_eq!(
			Balances::balance_on_hold(&hold_reason, &bob),
			initial_deposit,
			"existing hold keeps original amount"
		);
		assert_eq!(
			Balances::balance_on_hold(&hold_reason, &charlie),
			updated_deposit,
			"future signup uses updated deposit"
		);

		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedule));
		let full_report: indiv_pallet_game::FullReport<Runtime> =
			vec![vec![indiv_pallet_game::Report::Person].try_into().unwrap()]
				.try_into()
				.unwrap();
		let report =
			indiv_pallet_game::Call::<Runtime>::report { full_report: full_report.clone() };
		exec_signed_as_score_participant(&bob_pair, report.into());
		let report = indiv_pallet_game::Call::<Runtime>::report { full_report };
		exec_signed_as_score_participant(&charlie_pair, report.into());

		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedule));
		let bob_hold_before_offboard = Balances::balance_on_hold(&hold_reason, &bob);
		let bob_free_before_offboard = Balances::free_balance(bob.clone());
		let personhood_threshold = indiv_pallet_score::PersonhoodThreshold::<Runtime>::get();
		let expected_hold_before_offboard =
			if personhood_threshold <= 1 { 0 } else { initial_deposit };
		assert_eq!(
			bob_hold_before_offboard, expected_hold_before_offboard,
			"hold after game depends on personhood threshold"
		);

		let offboard = RuntimeCall::Game(indiv_pallet_game::Call::<Runtime>::offboard {});
		exec_signed_as_score_participant(&bob_pair, offboard);

		assert_eq!(
			Balances::balance_on_hold(&hold_reason, &bob),
			0,
			"offboard must fully release held deposit"
		);
		assert_eq!(
			Balances::free_balance(bob),
			bob_free_before_offboard + bob_hold_before_offboard,
			"released hold should be returned to free balance"
		);
	});
}

#[test]
fn deposit_player_flow() {
	new_test_ext().execute_with(|| {
		// Yields a personhood threshold of 3.
		setup_people(5001);
		let schedule: indiv_pallet_score::AbsenceGraceTiers =
			vec![indiv_pallet_score::AbsenceGraceTier {
				population_size_threshold: u32::MAX,
				window: 4,
				allowed_misses: 3,
			}]
			.try_into()
			.unwrap();
		indiv_pallet_score::AbsenceGraceSchedule::<Runtime>::put(schedule);

		let bob_pair = Sr25519Keyring::Bob.pair();
		let bob = Sr25519Keyring::Bob.to_account_id();

		// ─────────────────────────────────────
		// Recognize personhood for some person zoe #1
		// ─────────────────────────────────────
		let zoe_s = Crypto::new_secret([1u8; 32]);
		let zoe_p = Crypto::member_from_secret(&zoe_s);
		let zoe_score_alias_account = sr25519::Pair::from_seed(&[1u8; 32]);
		let zoe_score_alias = Crypto::alias_in_context(&zoe_s, &score_context()[..]).unwrap();
		let zoe_stmt_acc_pair = sr25519::Pair::from_seed(&[2u8; 32]);
		let zoe_stmt_acc = pair_to_account_id(&zoe_stmt_acc_pair);
		let zoe_stmt_acc_proof_of_ownership: MultiSignature = zoe_stmt_acc_pair
			.sign(
				&(b"pop:game:stmt_account_for_alias:", zoe_score_alias)
					.using_encoded(sp_io::hashing::blake2_256)[..],
			)
			.into();
		let zoe_id = indiv_pallet_people::NextPersonalId::<Runtime>::get();
		DummyDim::reserve_ids(RuntimeOrigin::root(), 1).unwrap();
		DummyDim::recognize_personhood(
			RuntimeOrigin::root(),
			vec![(zoe_id, zoe_p)].try_into().unwrap(),
		)
		.unwrap();
		// Onboarding and ring building happen in separate blocks.
		advance_block();
		advance_block();
		let set_alias = indiv_pallet_people::Call::<Runtime>::set_alias_account {
			account: zoe_score_alias_account.public().into(),
			call_valid_at: frame_system::Pallet::<Runtime>::block_number(),
		};
		exec_as_alias_with_proof(&zoe_s, score_context(), set_alias.into());

		// ─────────────────────────────────────
		// Schedule 12 games and 26 payouts. Game #7 (= schedules[6]) carries an airdrop
		// prize — it is Bob's last attended game in this pre-register segment, so the new
		// `last_attended_game == Some(game_index)` eligibility check lines up for his claim.
		// ─────────────────────────────────────
		reduce_game_phase_durations();
		const AIRDROP_MAX_WINNERS: u32 = 4;
		setup_airdrop_prize_asset(AIRDROP_MAX_WINNERS);
		let now = (pallet_timestamp::Now::<Runtime>::get() / 1000) as u32; // seconds
		let in_between_games = IN_BETWEEN_GAMES;
		let schedules = (1..=12)
			.map(|i| indiv_pallet_game::GameSchedule {
				game_play_time: now + i * in_between_games,
				rounds: 1,
				max_group_size: 3,
				airdrops: if i == 7 {
					game_airdrops(&[0], AIRDROP_MAX_WINNERS)
				} else {
					Default::default()
				},
			})
			.collect::<Vec<_>>();
		Game::schedule_games(RuntimeOrigin::root(), schedules.clone()).unwrap();
		let airdrop_game_index = 7u32;
		let airdrop_event_id = airdrop_event_id_for(airdrop_game_index, 0);
		FungibleExternalAsset::mint_into(
			&Score::score_pot_id(),
			10 * UNITS * 26 + FungibleExternalAsset::minimum_balance(),
		)
		.unwrap();
		Score::schedule_payout_rounds(RuntimeOrigin::root(), 10 * UNITS, 26, PAYOUT_DURATION)
			.unwrap();

		// ─────────────────────────────────────
		// Mint funds to Bob
		// ─────────────────────────────────────
		Balances::mint_into(&bob.clone(), 10 * UNITS).unwrap();

		// ─────────────────────────────────────
		// Bob sign-up for game #1. No airdrop entry here — Game #7 is the one carrying
		// the airdrop prize, and the new `last_attended_game == Some(game_index)` rule means
		// Bob must enter the draw on his last attended game in this segment. Advance
		// blocks until game #1's registration phase is open.
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[0]));
		let signup = indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
			identifier_key: [0u8; 65],
			airdrops: None,
		};
		exec_signed(&bob_pair, signup.into());

		// ─────────────────────────────────────
		// Bob transfer all to Alice to test feeless capability
		// ─────────────────────────────────────
		let transfer_all = pallet_balances::Call::<Runtime>::transfer_all {
			dest: Address::Id(Sr25519Keyring::Alice.to_account_id()),
			keep_alive: false,
		};
		exec_signed(&bob_pair, transfer_all.into());
		let bob_no_fee_used_anymore = Balances::free_balance(bob.clone());

		// ─────────────────────────────────────
		// Bob plays and wins Game #0
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedules[0]));
		let report = indiv_pallet_game::Call::<Runtime>::report {
			full_report: vec![BoundedVec::new()].try_into().unwrap(),
		};
		exec_signed_as_score_participant(&bob_pair, report.into());
		assert_eq!(Balances::free_balance(bob.clone()), bob_no_fee_used_anymore);

		// ─────────────────────────────────────
		// Bob cashes out
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[0]));
		let cash_out = indiv_pallet_score::Call::<Runtime>::cash_out {};
		exec_signed_as_score_participant(&bob_pair, cash_out.into());
		assert_eq!(Balances::free_balance(bob.clone()), bob_no_fee_used_anymore);

		// ─────────────────────────────────────
		// Bob loses Game #1 (miss report); redeem credit while archived
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[1]));
		// signup with person #1 to not have the game canceled.
		let sign_up_with_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			identifier_key: [1u8; 65],
			statement_account: zoe_stmt_acc.clone(),
			sig: zoe_stmt_acc_proof_of_ownership.clone(),
			airdrops: None,
		};
		exec_signed_as_alias_with_account(&zoe_score_alias_account, sign_up_with_alias.into());

		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[1]));
		let first_credit =
			indiv_pallet_score::Participants::<Runtime>::get(AccountOrPerson::Account(bob.clone()))
				.unwrap()
				.credit;
		assert!(first_credit > 0);
		let first_payout_pair = sr25519::Pair::from_seed(&[3u8; 32]);
		let first_payout = pair_to_account_id(&first_payout_pair);
		let redeem = indiv_pallet_score::Call::<Runtime>::redeem_credit {
			destination: first_payout.clone(),
		};
		exec_signed_as_score_participant(&bob_pair, redeem.into());
		assert_eq!(Balances::free_balance(bob.clone()), bob_no_fee_used_anymore);
		assert_eq!(FungibleExternalAsset::balance(&first_payout), first_credit);
		assert_eq!(
			indiv_pallet_score::Participants::<Runtime>::get(AccountOrPerson::Account(bob.clone()))
				.unwrap()
				.credit,
			0
		);

		// ─────────────────────────────────────
		// Alice funds Bob again exactly for a new deposit
		// ─────────────────────────────────────
		let transfer = pallet_balances::Call::<Runtime>::transfer_keep_alive {
			dest: Address::Id(bob.clone()),
			value: PlayDepositDefault::get(),
		};
		exec_signed(&Sr25519Keyring::Alice.pair(), transfer.into());

		// ─────────────────────────────────────
		// Bob sign-up for game #2
		// ─────────────────────────────────────
		let signup = RuntimeCall::Game(indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
			identifier_key: [2u8; 65],
			airdrops: None,
		});
		exec_signed_as_score_participant(&bob_pair, signup);
		assert_eq!(Balances::free_balance(bob.clone()), bob_no_fee_used_anymore);

		// ─────────────────────────────────────
		// Bob wins games #3-6 to build score buffer for grace period
		// Score after each: 1, 3, 6, 10
		// (personhood threshold = 3, grace period = 4 for 5001 people,
		//  so Bob needs score > 10 to survive 4 absences: 1+2+3+4 = 10)
		// ─────────────────────────────────────
		for game_i in 2..=5 {
			// (Game #3 is already signed-up.)
			if game_i > 2 {
				advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[game_i]));
				let signup = indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
					identifier_key: [3u8; 65],
					airdrops: None,
				};
				exec_signed_as_score_participant(&bob_pair, signup.into());
			}

			advance_until_time(GameTimes::<Runtime>::game_play_time(&schedules[game_i]));
			let full_report = vec![BoundedVec::new()].try_into().unwrap();
			let report =
				RuntimeCall::Game(indiv_pallet_game::Call::<Runtime>::report { full_report });
			exec_signed_as_score_participant(&bob_pair, report);
		}

		// ─────────────────────────────────────
		// Bob wins game #7 — the game carrying the airdrop prize. Drive the airdrop event
		// into `Registering` before signing up so the Account VRF entry is accepted. Bob's
		// score climbs to 15 and `last_attended_game` becomes `Some(7)`, matching the claim
		// target below.
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[6]));
		drive_airdrop_to_registering(airdrop_event_id);
		let signup = indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
			identifier_key: [4u8; 65],
			airdrops: Some(build_account_airdrop_vrfs(&bob_pair, &[airdrop_event_id])),
		};
		exec_signed_as_score_participant(&bob_pair, signup.into());
		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedules[6]));
		let full_report = vec![BoundedVec::new()].try_into().unwrap();
		let report = RuntimeCall::Game(indiv_pallet_game::Call::<Runtime>::report { full_report });
		exec_signed_as_score_participant(&bob_pair, report);
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[6]));

		// ─────────────────────────────────────
		// Bob claims the airdrop scheduled on game #7 to a fresh beneficiary BEFORE
		// calling `score::register`. He registered with the Account VRF when he was
		// not yet recognized, and is now eligible to claim because his attended games
		// lifted his score past the personhood threshold (`reached_personhood`). The
		// airdrop is attached to Bob's most recently attended game, so `last_attended_game`
		// matches the `game_index` of the claim — `score::register` is not required.
		// He authenticates as a Score participant (so the call stays feeless even though
		// Bob's free balance is zero).
		// ─────────────────────────────────────
		drive_airdrop_to_claiming(airdrop_event_id);
		let beneficiary = pair_to_account_id(&sr25519::Pair::from_seed(&[200u8; 32]));
		let beneficiary_before = FungibleExternalAsset::balance(&beneficiary);
		let claim = indiv_pallet_game::Call::<Runtime>::claim_airdrop {
			game_index: airdrop_game_index,
			airdrop_index: 0,
			beneficiary: beneficiary.clone(),
		};
		exec_signed_as_score_participant(&bob_pair, claim.into());
		assert_eq!(
			FungibleExternalAsset::balance(&beneficiary),
			beneficiary_before + AIRDROP_PRIZE_PER_WINNER,
			"airdrop prize must arrive at the beneficiary",
		);

		// ─────────────────────────────────────
		// Bob registers personhood
		// ─────────────────────────────────────
		let s = Crypto::new_secret([2u8; 32]);
		let p = Crypto::member_from_secret(&s);
		let msg = (b"pop register using", &bob).encode();
		let sig = Crypto::sign(&s, &msg[..]).unwrap();
		let register = indiv_pallet_score::Call::<Runtime>::register { key: Some((p, sig)) };
		exec_signed_as_score_participant(&bob_pair, register.into());

		// ─────────────────────────────────────
		// Bob vote on some cases with personhood
		// ─────────────────────────────────────
		// Onboarding and ring building happen in separate blocks.
		advance_block();
		advance_block();
		// fake case
		let case_idx = indiv_pallet_mob_rule::Pallet::<Runtime>::judge_statement(
			Statement::UsernameValid { username: b"hello".to_vec().try_into().unwrap() },
			JudgementContext::truncate_from([0u8; 32].encode()),
			Callback::from_parts(0, 0),
		)
		.unwrap();
		let bob_mob_alias_acc = sr25519::Pair::from_seed(&[41u8; 32]);
		let set_alias = indiv_pallet_people::Call::<Runtime>::set_alias_account {
			account: bob_mob_alias_acc.public().into(),
			call_valid_at: frame_system::Pallet::<Runtime>::block_number(),
		};
		exec_as_alias_with_proof(&s, indiv_pallet_mob_rule::MOB_CONTEXT, set_alias.into());
		let vote_call = indiv_pallet_mob_rule::Call::<Runtime>::vote {
			case_index: case_idx,
			opinion: Judgement::Truth(Truth::True),
		};
		exec_signed_as_alias_with_account(&bob_mob_alias_acc, vote_call.into());

		// ─────────────────────────────────────
		// Bob loses games #7-10 (grace period = 4 for 5001 people)
		// Score after each: 14, 12, 9, 5 — suspended at absence 4, score > 0
		// ─────────────────────────────────────
		for game_i in 7..=10 {
			advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[game_i]));
			// signup with zoe to not have the game canceled.
			let sign_up_with_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
				identifier_key: [4u8; 65],
				statement_account: zoe_stmt_acc.clone(),
				sig: zoe_stmt_acc_proof_of_ownership.clone(),
				airdrops: None,
			};
			exec_signed_as_alias_with_account_revised(
				&zoe_score_alias_account,
				&zoe_s,
				sign_up_with_alias.into(),
			);
			advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[game_i]));
		}

		// ─────────────────────────────────────
		// Bob wins game #11 to recover score above threshold
		// Score: 5 + 1 = 6 >= threshold (3)
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[11]));
		let signup = RuntimeCall::Game(indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
			identifier_key: [5u8; 65],
			airdrops: None,
		});
		exec_signed_as_score_participant(&bob_pair, signup);
		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedules[11]));
		let full_report = vec![BoundedVec::new()].try_into().unwrap();
		let report = RuntimeCall::Game(indiv_pallet_game::Call::<Runtime>::report { full_report });
		exec_signed_as_score_participant(&bob_pair, report);
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[11]));

		// ─────────────────────────────────────
		// Bob resume personhood
		// ─────────────────────────────────────
		let register =
			RuntimeCall::Score(indiv_pallet_score::Call::<Runtime>::register { key: None });
		exec_signed_as_score_participant(&bob_pair, register);

		// ─────────────────────────────────────
		// Bob vote on some cases with personhood
		// ─────────────────────────────────────
		// Onboarding and ring building happen in separate blocks.
		advance_block();
		advance_block();
		// fake case
		let case_idx = indiv_pallet_mob_rule::Pallet::<Runtime>::judge_statement(
			Statement::UsernameValid { username: b"hello".to_vec().try_into().unwrap() },
			JudgementContext::truncate_from([0u8; 32].encode()),
			Callback::from_parts(0, 0),
		)
		.unwrap();
		let vote_call = indiv_pallet_mob_rule::Call::<Runtime>::vote {
			case_index: case_idx,
			opinion: Judgement::Truth(Truth::True),
		};
		exec_signed_as_alias_with_account_revised(&bob_mob_alias_acc, &s, vote_call.into());

		// ─────────────────────────────────────
		// Bob earns no additional credit from games played as a recognized person; his
		// remaining credit stays at 0 (the first cash-out was already redeemed above).
		// ─────────────────────────────────────
		assert_eq!(
			indiv_pallet_score::Participants::<Runtime>::get(AccountOrPerson::Account(bob.clone()))
				.unwrap()
				.credit,
			0
		);

		// Schedule one attended game (with an airdrop prize so Bob can register via the
		// Alias VRF) followed by enough absence games to drive Bob's score back to 0.
		// ─────────────────────────────────────
		let now_extra = (pallet_timestamp::Now::<Runtime>::get() / 1000) as u32;
		let extra_schedules = (1..=6)
			.map(|i| indiv_pallet_game::GameSchedule {
				game_play_time: now_extra + i * in_between_games,
				rounds: 1,
				max_group_size: 3,
				// The first extra game carries the alias-VRF airdrop prize.
				airdrops: if i == 1 {
					game_airdrops(&[0], AIRDROP_MAX_WINNERS)
				} else {
					Default::default()
				},
			})
			.collect::<Vec<_>>();
		// Top up the airdrop source for the second event's prize allocation.
		FungibleExternalAsset::mint_into(
			&GameAirdropSource::get(),
			(AIRDROP_MAX_WINNERS as Balance).saturating_mul(AIRDROP_PRIZE_PER_WINNER),
		)
		.unwrap();
		Game::schedule_games(RuntimeOrigin::root(), extra_schedules.clone()).unwrap();
		// Twelve games were already created earlier, so the first extra is game #13.
		let alias_airdrop_game_index = 13u32;
		let alias_airdrop_event_id = airdrop_event_id_for(alias_airdrop_game_index, 0);

		// ─────────────────────────────────────
		// Game #13: Bob is recognized after resume; he signs up with the Alias VRF and
		// reports, pinning `last_attended_game` to `Some(13)`, which matches the claim target
		// below. Afterwards Bob loses games #14-18 to reach score 0.
		// ─────────────────────────────────────
		let attended_sched = &extra_schedules[0];
		advance_until_time(GameTimes::<Runtime>::registration_start(attended_sched));
		drive_airdrop_to_registering(alias_airdrop_event_id);
		let signup_bob = indiv_pallet_game::Call::<Runtime>::sign_up_with_account {
			identifier_key: [8u8; 65],
			airdrops: Some(build_alias_airdrop_vrfs(
				&s,
				&[alias_airdrop_event_id],
				RegistrationEntry::Account { account_id: bob.clone() },
			)),
		};
		exec_signed_as_score_participant(&bob_pair, signup_bob.into());
		advance_until_time(GameTimes::<Runtime>::game_play_time(attended_sched));
		let full_report = vec![BoundedVec::new()].try_into().unwrap();
		let report = RuntimeCall::Game(indiv_pallet_game::Call::<Runtime>::report { full_report });
		exec_signed_as_score_participant(&bob_pair, report);
		advance_until_time(GameTimes::<Runtime>::player_process_end(attended_sched));

		// Drive the alias-VRF airdrop event to `Claiming` and claim it to a fresh beneficiary.
		drive_airdrop_to_claiming(alias_airdrop_event_id);
		let beneficiary_alias = pair_to_account_id(&sr25519::Pair::from_seed(&[201u8; 32]));
		let beneficiary_alias_before = FungibleExternalAsset::balance(&beneficiary_alias);
		let claim_alias = indiv_pallet_game::Call::<Runtime>::claim_airdrop {
			game_index: alias_airdrop_game_index,
			airdrop_index: 0,
			beneficiary: beneficiary_alias.clone(),
		};
		exec_signed_as_score_participant(&bob_pair, claim_alias.into());
		assert_eq!(
			FungibleExternalAsset::balance(&beneficiary_alias),
			beneficiary_alias_before + AIRDROP_PRIZE_PER_WINNER,
			"alias-VRF airdrop prize must arrive at the beneficiary",
		);

		for idx in 1..6 {
			let sched = &extra_schedules[idx];
			advance_until_time(GameTimes::<Runtime>::registration_start(sched));
			// signup with person #1 to not have the game canceled.
			let sign_up_with_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
				identifier_key: [7u8; 65],
				statement_account: zoe_stmt_acc.clone(),
				sig: zoe_stmt_acc_proof_of_ownership.clone(),
				airdrops: None,
			};
			exec_signed_as_alias_with_account_revised(
				&zoe_score_alias_account,
				&zoe_s,
				sign_up_with_alias.into(),
			);
			advance_until_time(GameTimes::<Runtime>::player_process_end(sched));
		}

		// ─────────────────────────────────────
		// Alice kickout bob
		// ─────────────────────────────────────
		let now = frame_system::Pallet::<Runtime>::block_number();
		let kickout_duration: u32 =
			<Runtime as indiv_pallet_game::Config>::NonPlayingKickoutTime::get();
		frame_system::Pallet::<Runtime>::set_block_number(now + kickout_duration);
		advance_block();
		let kickout = indiv_pallet_game::Call::<Runtime>::kickout { player: bob.clone() };
		exec_signed(&Sr25519Keyring::Alice.pair(), kickout.into());

		// ─────────────────────────────────────
		// Assert all bob's transactions were free
		// ─────────────────────────────────────
		assert_eq!(
			Balances::free_balance(bob.clone()),
			bob_no_fee_used_anymore + PlayDepositDefault::get(),
			"all feeless"
		);
	});
}
