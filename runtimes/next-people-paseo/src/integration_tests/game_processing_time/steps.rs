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

//! The shuffle and the player process, the phases the offchain worker runs in step calls.

use super::*;
use indiv_pallet_game::{WeightInfo as _, OP_UPPER_BOUND, PLAYER_PROCESS_STEP2_CHUNK};

/// The work of a step call, per player.
#[derive(Clone, Copy)]
pub(super) struct StepWork {
	/// The weight a step call consumes before its loop, including `game_step_base`.
	pub(super) overhead: Weight,
	/// The weight of one player.
	pub(super) unit: Weight,
	/// The players a step call processes in `OP_UPPER_BOUND` loop iterations.
	pub(super) max_units: u64,
}

/// The phases the offchain worker runs in step calls.
pub(super) struct GameSteps {
	/// `advance_shuffle`. A player takes three loop iterations: insert, retrieve and compute
	/// weights.
	pub(super) shuffle: StepWork,
	/// `process_players` in step 1, one loop iteration per player.
	pub(super) process_step1: StepWork,
	/// `process_players` in step 2. A loop iteration clears `PLAYER_PROCESS_STEP2_CHUNK` entries
	/// of each index map, and a player has one entry per round.
	pub(super) process_step2: StepWork,
	/// The shuffle insert step, which every stored player costs, whether they play or not.
	pub(super) shuffle_insert: Weight,
}

impl GameSteps {
	pub(super) fn of_runtime(rounds: u32, group_size: u32) -> Self {
		let base = GameWeights::game_step_base();
		let shuffle_insert = GameWeights::shuffle_step_insert(rounds);
		let shuffle = StepWork {
			overhead: base
				.saturating_add(GameWeights::shuffles_base())
				.saturating_add(GameWeights::on_game_cancelled(u32::from(GAME_AIRDROPS))),
			unit: shuffle_insert
				.saturating_add(GameWeights::shuffle_step_retrieve(rounds))
				.saturating_add(GameWeights::shuffle_step_compute_weights(rounds * group_size)),
			max_units: u64::from(OP_UPPER_BOUND / 3),
		};
		let process_step1 = StepWork {
			overhead: base.saturating_add(GameWeights::player_process_step1()),
			unit: GameWeights::player_process_step1_inner_loop(rounds),
			max_units: u64::from(OP_UPPER_BOUND),
		};
		let chunk = u64::from(PLAYER_PROCESS_STEP2_CHUNK);
		let process_step2 = StepWork {
			overhead: base.saturating_add(GameWeights::player_process_step2()),
			unit: GameWeights::player_process_step2_inner_loop().saturating_mul(u64::from(rounds)) /
				chunk,
			max_units: u64::from(OP_UPPER_BOUND) * chunk / u64::from(rounds),
		};
		Self { shuffle, process_step1, process_step2, shuffle_insert }
	}
}
