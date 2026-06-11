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
		let amy_score_alias = Crypto::alias_in_context(&amy_secret, &SCORE_CONTEXT[..]).unwrap();
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
		exec_as_alias_with_proof(&amy_secret, SCORE_CONTEXT, set_alias.into());

		// ─────────────────────────────────────
		// Schedule 5 games and 12 payouts. Game #4 (= schedules[3]) carries an airdrop
		// prize so Amy can exercise the alias-VRF path end-to-end. Game #4 is also Amy's
		// last attended game in the segment below, so `last_attended_game == Some(4)` lines
		// up with the claim target.
		// ─────────────────────────────────────
		reduce_game_phase_durations();
		const AIRDROP_MAX_WINNERS: u32 = 4;
		setup_airdrop_prize_asset(AIRDROP_MAX_WINNERS);
		let now = (pallet_timestamp::Now::<Runtime>::get() / 1000) as u32; // seconds
		let in_between_games = IN_BETWEEN_GAMES;
		let schedules = (1..=5)
			.map(|i| indiv_pallet_game::GameSchedule {
				game_play_time: now + i * in_between_games,
				rounds: 1,
				max_group_size: 3,
				airdrop_prize: (i == 4).then(|| airdrop_prize_for(AIRDROP_MAX_WINNERS)),
			})
			.collect::<Vec<_>>();
		Game::schedule_games(RuntimeOrigin::root(), schedules.clone()).unwrap();
		let airdrop_game_index = 4u32;
		let airdrop_event_id = airdrop_event_id_for(airdrop_game_index);
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
			airdrop: None,
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
			airdrop: None,
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
			airdrop: None,
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
		// Game #4: sign up with alias and the Alias VRF for the airdrop, report and win.
		// After this game `last_attended_game` is `Some(4)` — the index of the airdrop's game.
		// ─────────────────────────────────────
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedules[3]));
		drive_airdrop_to_registering(airdrop_event_id);
		let sign_up_alias = indiv_pallet_game::Call::<Runtime>::sign_up_with_alias {
			identifier_key: [3u8; 65],
			statement_account: amy_stmt_acc.clone(),
			sig: amy_stmt_acc_proof_of_ownership.clone(),
			airdrop: Some(build_alias_airdrop_vrf(
				&amy_secret,
				airdrop_event_id,
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
		// Amy claims the airdrop scheduled on game #4 to a fresh beneficiary. She is
		// recognized, so she signs the claim as her personal alias.
		// ─────────────────────────────────────
		drive_airdrop_to_claiming(airdrop_event_id);
		let beneficiary = pair_to_account_id(&sr25519::Pair::from_seed(&[200u8; 32]));
		let beneficiary_before = FungibleExternalAsset::balance(&beneficiary);
		let claim = indiv_pallet_game::Call::<Runtime>::claim_airdrop {
			game_index: airdrop_game_index,
			beneficiary: beneficiary.clone(),
		};
		exec_signed_as_alias_with_account(&amy_alias_acc_pair, claim.into());
		assert_eq!(
			FungibleExternalAsset::balance(&beneficiary),
			beneficiary_before + AIRDROP_PRIZE_PER_WINNER,
			"airdrop prize must arrive at the beneficiary",
		);
	});
}
