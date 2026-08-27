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

//! A lite person plays the game for free with an account of their choosing: they bind an account to
//! their alias in the score context, so that it can act as that alias, and it signs itself up with
//! `sign_up_with_account_lite_invite`, without holding any funds.

use super::*;
use crate::people::PlayDepositReason;
use frame_support::assert_ok;
use indiv_support::traits::ContextualAlias;

/// Build a transaction dispatching `call` from the lite alias the account `who` is bound to,
/// authenticated by `who`'s signature and nonce.
fn build_as_lite_alias_with_account_ext(
	who: &sr25519::Pair,
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	build_people_lite_auth_ext(
		who,
		indiv_pallet_people_lite::PeopleLiteAuthData::AsLiteAliasWithAccount,
		call,
	)
}

/// The whole flow: an attested lite person binds a product key to their alias in the score
/// context, that key signs itself up for a game and plays it, and neither account ever holds or
/// spends any funds.
#[test]
fn lite_person_plays_the_game_for_free_with_a_product_key() {
	new_test_ext().execute_with(|| {
		// The lite person, attested by an authority, and the product key they play with. Neither
		// holds any funds.
		let lite_pair = sr25519::Pair::from_seed(&[77u8; 32]);
		let lite_account = pair_to_account_id(&lite_pair);
		let lite_secret = register_lite_person_for_integration(&lite_pair);
		let product_pair = sr25519::Pair::from_seed(&[78u8; 32]);
		let product_key = pair_to_account_id(&product_pair);
		let product_player = AccountOrPerson::Account(product_key.clone());
		assert_eq!(Balances::free_balance(lite_account.clone()), 0);
		assert_eq!(Balances::free_balance(product_key.clone()), 0);

		// ─────────────────────────────────────
		// The lite person binds the product key to their alias in the score context, using their
		// ring VRF key, so that the key can act as that alias. Nothing links it to their own
		// account.
		// ─────────────────────────────────────
		let set_alias = indiv_pallet_people_lite::Call::<Runtime>::set_alias_account {
			account: product_key.clone(),
			valid_at_block: frame_system::Pallet::<Runtime>::block_number(),
		};
		let uxt =
			build_as_lite_alias_with_proof_ext(&lite_secret, score_context(), set_alias.into());
		Executive::apply_extrinsic(uxt)
			.expect("the alias setup transaction is valid")
			.expect("the alias setup dispatch succeeds");
		let score_alias = Crypto::alias_in_context(&lite_secret, &score_context()[..]).unwrap();
		assert_eq!(
			indiv_pallet_people_lite::AliasToAccount::<Runtime>::get(ContextualAlias {
				alias: score_alias,
				context: score_context()
			}),
			Some(product_key.clone())
		);

		// ─────────────────────────────────────
		// A game is scheduled and its registration phase is open, which the lite invite requires.
		// ─────────────────────────────────────
		setup_people(5001);
		reduce_game_phase_durations();
		let now = (pallet_timestamp::Now::<Runtime>::get() / 1000) as u32;
		let schedule = indiv_pallet_game::GameSchedule {
			game_play_time: now + IN_BETWEEN_GAMES,
			rounds: 1,
			max_group_size: 2,
			airdrops: Default::default(),
			private_claims: None,
		};
		assert_ok!(Game::schedule_games(RuntimeOrigin::root(), vec![schedule.clone()]));
		advance_until_time(GameTimes::<Runtime>::registration_start(&schedule));

		// ─────────────────────────────────────
		// The product key, acting as the lite alias, names itself as the playing account. The
		// transaction is free: the lite alias origin holds an allowance in
		// pallet-origin-restriction, and the sign-up takes no deposit.
		// ─────────────────────────────────────
		let lite_invite = indiv_pallet_game::Call::<Runtime>::sign_up_with_account_lite_invite {
			account: product_key.clone(),
			identifier_key: [0u8; 65],
			airdrops: None,
		};
		let uxt = build_as_lite_alias_with_account_ext(&product_pair, lite_invite.into());
		Executive::apply_extrinsic(uxt)
			.expect("the lite invite transaction is valid")
			.expect("the lite invite dispatch succeeds");
		assert_eq!(Balances::free_balance(product_key.clone()), 0);
		assert!(Balances::balance_on_hold(&PlayDepositReason::get(), &product_key).is_zero());
		assert_eq!(
			indiv_pallet_game::LiteInvites::<Runtime>::get(score_alias),
			Some(product_key.clone())
		);
		let player = indiv_pallet_game::Players::<Runtime>::get(&product_player)
			.expect("the product key is a player");
		assert!(matches!(player.credibility, indiv_pallet_game::PlayerCredibility::Invited));
		assert!(player.registered, "the product key is registered for the ongoing game");

		// The player attends the game: it reports about its group, alone here, and is processed as
		// having attended.
		advance_until_time(GameTimes::<Runtime>::game_play_time(&schedule));
		let report = indiv_pallet_game::Call::<Runtime>::report {
			full_report: vec![BoundedVec::new()].try_into().unwrap(),
		};
		exec_signed_as_score_participant(&product_pair, report.into());
		advance_until_time(GameTimes::<Runtime>::reporting_end(&schedule) + 1);
		for _ in 0..8 {
			advance_block();
		}

		let score = indiv_pallet_score::Participants::<Runtime>::get(&product_player)
			.expect("the product key is a participant")
			.score;
		assert!(
			score > 0,
			"the lite person's playing account attended the game it played for free"
		);
		assert_eq!(Balances::free_balance(product_key), 0);
		assert_eq!(Balances::free_balance(lite_account), 0);
	});
}
