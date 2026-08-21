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

#[test]
fn alias_player_flow() {
	new_test_ext().execute_with(|| {
		// Yields a personhood threshold of 3.
		setup_people(5001);

		// Create a person (alias-based player): Amy
		let amy_secret = Crypto::new_secret([10u8; 32]);
		let amy_member = Crypto::member_from_secret(&amy_secret);

		// Alias account used to sign as the personal alias in SCORE context
		let amy_alias_acc_pair = sr25519::Pair::from_seed(&[10u8; 32]);
		let amy_alias_acc = pair_to_account_id(&amy_alias_acc_pair);
		assert_eq!(Balances::free_balance(amy_alias_acc.clone()), 0);

		// Statement account (used to talk to the statement store during the game)
		let amy_stmt_acc_pair = sr25519::Pair::from_seed(&[11u8; 32]);
		let amy_stmt_acc = pair_to_account_id(&amy_stmt_acc_pair);

		// Create the alias in SCORE context and the required proof-of-ownership for the game
		let amy_score_alias = Crypto::alias_in_context(&amy_secret, &score_context()[..]).unwrap();
		let amy_stmt_acc_proof_of_ownership: MultiSignature = amy_stmt_acc_pair
			.sign(
				&(b"pop:game:stmt_account_for_alias:", amy_score_alias)
					.using_encoded(sp_io::hashing::blake2_256)[..],
			)
			.into();

		// ─────────────────────────────────────
		// Recognize Amy's personhood (Dummy DIM)
		// ─────────────────────────────────────
		let amy_id = indiv_pallet_people::NextPersonalId::<Runtime>::get();
		DummyDim::reserve_ids(RuntimeOrigin::root(), 1).unwrap();
		DummyDim::recognize_personhood(
			RuntimeOrigin::root(),
			vec![(amy_id, amy_member)].try_into().unwrap(),
		)
		.unwrap();
		// Onboarding and ring building happen in separate blocks.
		advance_block();
		advance_block();

		// ─────────────────────────────────────
		// Amy sets alias account for SCORE context
		// ─────────────────────────────────────
		let set_alias = indiv_pallet_people::Call::<Runtime>::set_alias_account {
			account: amy_alias_acc_pair.public().into(),
			call_valid_at: frame_system::Pallet::<Runtime>::block_number(),
		};
		exec_as_alias_with_proof(&amy_secret, score_context(), set_alias.into());

		// ─────────────────────────────────────
		// Schedule 5 games and 12 payouts. Game #4 (= schedules[3]) carries two airdrop
		// events — one drawn at the game play time, one drawn half a game-interval later —
		// so Amy can exercise the alias-VRF path across several draws end-to-end. Game #4 is
		// also Amy's last attended game in the segment below, so `last_attended_game ==
		// Some(4)` lines up with the claim targets.
		// ─────────────────────────────────────
		reduce_game_phase_durations();
		const AIRDROP_MAX_WINNERS: u32 = 4;
		// Two events, each with its own `max_winners` prize allocation.
		setup_airdrop_prize_asset(2 * AIRDROP_MAX_WINNERS);
		let now = (pallet_timestamp::Now::<Runtime>::get() / 1000) as u32; // seconds
		let in_between_games = IN_BETWEEN_GAMES;
		let second_draw_offset = in_between_games / 2;
		let schedules = (1..=5)
			.map(|i| indiv_pallet_game::GameSchedule {
				game_play_time: now + i * in_between_games,
				rounds: 1,
				max_group_size: 3,
				airdrops: if i == 4 {
					game_airdrops(&[0, second_draw_offset], AIRDROP_MAX_WINNERS)
				} else {
					Default::default()
				},
			})
			.collect::<Vec<_>>();
		Game::schedule_games(RuntimeOrigin::root(), schedules.clone()).unwrap();
		let airdrop_game_index = 4u32;
		let airdrop_event_ids = [
			airdrop_event_id_for(airdrop_game_index, 0),
			airdrop_event_id_for(airdrop_game_index, 1),
		];
		FungibleExternalAsset::mint_into(
			&Score::score_pot_id(),
			10 * UNITS * 12 + FungibleExternalAsset::minimum_balance(),
		)
		.unwrap();
		Score::schedule_payout_rounds(RuntimeOrigin::root(), 10 * UNITS, 12, PAYOUT_DURATION)
			.unwrap();

		// ─────────────────────────────────────
		// Game #1: sign up with alias but DO NOT report (counts as absent). The airdrop
		// for this game is not the one Amy is targeting. Advance time so game #1's
		// registration phase is open.
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[0]));
		let sign_up_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			identifier_key: [0u8; 65],
			statement_account: amy_stmt_acc.clone(),
			sig: amy_stmt_acc_proof_of_ownership.clone(),
			airdrops: None,
		};
		exec_signed_as_alias_with_account(&amy_alias_acc_pair, sign_up_alias.into());
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[0]));

		// ─────────────────────────────────────
		// Game #1: sign up with alias, report and win
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[1]));
		let sign_up_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			identifier_key: [1u8; 65],
			statement_account: amy_stmt_acc.clone(),
			sig: amy_stmt_acc_proof_of_ownership.clone(),
			airdrops: None,
		};
		exec_signed_as_alias_with_account(&amy_alias_acc_pair, sign_up_alias.into());

		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedules[1]));
		let full_report = vec![BoundedVec::new()].try_into().unwrap();
		let report = indiv_pallet_game::Call::<Runtime>::report { full_report };
		exec_signed_as_alias_with_account(&amy_alias_acc_pair, report.into());
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[1]));

		// ─────────────────────────────────────
		// Game #2: sign up with alias but DO NOT report (counts as absent)
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[2]));
		let sign_up_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			identifier_key: [2u8; 65],
			statement_account: amy_stmt_acc.clone(),
			sig: amy_stmt_acc_proof_of_ownership.clone(),
			airdrops: None,
		};
		exec_signed_as_alias_with_account(&amy_alias_acc_pair, sign_up_alias.into());
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[2]));

		// ─────────────────────────────────────
		// Playing games as a recognized person no longer accrues credit.
		// ─────────────────────────────────────
		assert_eq!(
			indiv_pallet_score::Participants::<Runtime>::get(AccountOrPerson::Person(
				amy_score_alias,
			))
			.unwrap()
			.credit,
			0
		);

		// ─────────────────────────────────────
		// Game #4: sign up with alias and one Alias VRF per airdrop event — each proof is
		// bound to its own event id. After this game `last_attended_game` is `Some(4)` — the
		// index of the airdrops' game.
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[3]));
		for event_id in airdrop_event_ids {
			drive_airdrop_to_registering(event_id);
		}
		let sign_up_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			identifier_key: [3u8; 65],
			statement_account: amy_stmt_acc.clone(),
			sig: amy_stmt_acc_proof_of_ownership.clone(),
			airdrops: Some(build_alias_airdrop_vrfs(
				&amy_secret,
				&airdrop_event_ids,
				RegistrationEntry::Alias { alias: amy_score_alias },
			)),
		};
		exec_signed_as_alias_with_account(&amy_alias_acc_pair, sign_up_alias.into());
		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedules[3]));
		let full_report = vec![BoundedVec::new()].try_into().unwrap();
		let report = indiv_pallet_game::Call::<Runtime>::report { full_report };
		exec_signed_as_alias_with_account(&amy_alias_acc_pair, report.into());
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[3]));

		// ─────────────────────────────────────
		// Credit stays at 0 across further game wins.
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::player_process_end(&schedules[4]));
		assert_eq!(
			indiv_pallet_score::Participants::<Runtime>::get(AccountOrPerson::Person(
				amy_score_alias,
			))
			.unwrap()
			.credit,
			0
		);

		// ─────────────────────────────────────
		// Amy claims both airdrops scheduled on game #4 to a fresh beneficiary, each one
		// separately once its own draw has run. She is recognized, so she signs the claims
		// as her personal alias. She never attends another game in between, so
		// `last_attended_game` stays at `Some(4)` for the later draw too.
		// ─────────────────────────────────────
		let beneficiary = pair_to_account_id(&sr25519::Pair::from_seed(&[200u8; 32]));
		let beneficiary_before = FungibleExternalAsset::balance(&beneficiary);
		for (airdrop_index, event_id) in airdrop_event_ids.into_iter().enumerate() {
			drive_airdrop_to_claiming(event_id);
			let claim = indiv_pallet_game::Call::<Runtime>::claim_airdrop {
				game_index: airdrop_game_index,
				airdrop_index: airdrop_index as u8,
				beneficiary: beneficiary.clone(),
			};
			exec_signed_as_alias_with_account(&amy_alias_acc_pair, claim.into());
			assert_eq!(
				FungibleExternalAsset::balance(&beneficiary),
				beneficiary_before + (airdrop_index as Balance + 1) * AIRDROP_PRIZE_PER_WINNER,
				"airdrop prize must arrive at the beneficiary",
			);
		}
	});
}
