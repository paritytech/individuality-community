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

//! The shuffle and the player process, the phases the game hooks run.

use super::*;
use indiv_pallet_game::{WeightInfo as _, MAX_GAME_AIRDROPS, PLAYER_PROCESS_STEP2_CHUNK};

/// The work of a phase the game hooks run, one loop iteration per player.
#[derive(Clone, Copy)]
pub(super) struct HookWork {
	/// The weight of a hook call before its loop.
	pub(super) overhead: Weight,
	/// The weight of one loop iteration.
	pub(super) unit: Weight,
}

/// The phases the game hooks run.
pub(super) struct GameHooks {
	pub(super) shuffle: HookWork,
	pub(super) player_process: HookWork,
	/// The shuffle insert step, which every stored player costs, whether they play or not.
	pub(super) shuffle_insert: Weight,
	/// The player process step 1, which every stored player costs, whether they play or not.
	pub(super) process_step1: Weight,
}

impl GameHooks {
	pub(super) fn of_runtime(rounds: u32, group_size: u32) -> Self {
		let max_airdrops = u32::from(MAX_GAME_AIRDROPS);
		let get_game = GameWeights::get_game();
		let shuffle_insert = GameWeights::shuffle_step_insert(rounds);
		let shuffle = HookWork {
			overhead: get_game
				.saturating_add(GameWeights::shuffles_base())
				.saturating_add(GameWeights::on_game_cancelled(max_airdrops)),
			unit: shuffle_insert
				.saturating_add(GameWeights::shuffle_step_retrieve(rounds))
				.saturating_add(GameWeights::shuffle_step_compute_weights(rounds * group_size)),
		};
		let process_step1 = GameWeights::player_process_step1_inner_loop(rounds);
		let player_process = HookWork {
			overhead: get_game.saturating_add(
				GameWeights::player_process_step1().max(GameWeights::player_process_step2()),
			),
			// Step 2 clears `PLAYER_PROCESS_STEP2_CHUNK` entries of each map per loop, and a player
			// has an index entry per round.
			unit: process_step1.saturating_add(
				GameWeights::player_process_step2_inner_loop().saturating_mul(u64::from(rounds)) /
					u64::from(PLAYER_PROCESS_STEP2_CHUNK),
			),
		};
		Self { shuffle, player_process, shuffle_insert, process_step1 }
	}
}
