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

//! An estimate of how many players each game phase can take and of how long each phase of a game
//! of a given size takes, from this runtime's weights, block limits and game configuration.
//!
//! Nothing is executed. The estimate repeats the metering of the game pallet:
//! * Sign-ups and reports are `Normal` transactions. The estimate builds each one and books it as
//!   `CheckWeight` does: the base extrinsic weight, the weight of every transaction extension, the
//!   call weight and the encoded length as proof size. The encoded lengths also count toward the
//!   `Normal` block length.
//! * An account path is a general transaction that `VerifySignature` signs.
//! * `report` is charged for the early enactment of every co-player and the reporter, then refunded
//!   to the real count. A player is enacted at most once in a game, so a report enacts one player
//!   on average. The estimate uses that refunded weight: the charge only has to fit when the
//!   transaction is included.
//! * A transaction declares its benchmarked worst-case proof size. `StorageWeightReclaim` returns
//!   the unused part after dispatch, so a block takes more of the transactions bound by the proof
//!   size than the estimate.
//! * `report` refuses a report whose credits the NFT claim credit buffers cannot hold. Every block
//!   commits one tree of `AWARDS_PER_TREE` credits, including the blocks the game hooks skip. This
//!   bounds the reports per block. When every credit is backfilled, it also bounds the players the
//!   player process takes per block.
//! * The shuffle and the player process run in `on_poll` and `on_idle`. Each hook takes half of the
//!   weight it is given. In a block without transactions the two hooks get 3/4 of the block. In a
//!   full block only `on_poll` runs, with 1/2. Both hooks skip one block in
//!   `GAME_PROCESS_SKIPPED_BLOCK`. A hook call runs at most `OP_UPPER_BOUND` loop iterations, one
//!   per player in the shuffle and in the player process step 1.
//! * The shuffle insert step and the player process step 1 iterate over every stored player,
//!   including the players who did not sign up for this game. The estimate assumes that every
//!   stored player plays. Each other stored player costs one more step, which the estimate prints.
//!
//! The estimate is split into what a transaction costs ([`paths`]), what the game hooks cost
//! ([`hooks`]), how much of each fits in a block ([`limits`]) and the printed tables ([`tables`]).
//!
//! The benchmarking build widens `MaxRounds` and `MaxGroupSize`, so it ignores the test.
//!
//! Print the estimate with:
//! `SKIP_WASM_BUILD=1 cargo test -p next-people-paseo-runtime --lib game_processing_time --
//! --nocapture`

mod hooks;
mod limits;
mod paths;
mod tables;

use super::*;
use hooks::GameHooks;
use indiv_pallet_game::PhaseDurationValues;
use indiv_pallet_nft_credits::AWARDS_PER_TREE;
use indiv_support::weight_budget::OcwWeightBudget;
use limits::{BlockLoad, Bound, Limits, PerBlock};
use paths::Paths;

type GameWeights = <Runtime as indiv_pallet_game::Config>::WeightInfo;

/// The player counts to print the phase durations for.
const PLAYER_COUNTS: [u64; 3] = [10_000, 100_000, 1_000_000];

/// What the estimate prints from: the runtime's limits and game configuration, and the cost of
/// each phase's work.
struct Estimate {
	limits: Limits,
	phases: PhaseDurationValues,
	rounds: u32,
	group_size: u32,
	/// The co-players a report covers, each of whom a `Person` vote awards a credit to.
	co_players: u32,
	paths: Paths,
	hooks: GameHooks,
	/// A report awards up to one credit per co-player, and a block commits one tree of credits.
	reports_per_credit_tree: PerBlock,
}

#[test]
#[cfg_attr(
	feature = "runtime-benchmarks",
	ignore = "the benchmarking build widens `MaxRounds` and `MaxGroupSize`"
)]
fn game_processing_time_estimate() {
	let limits = Limits::of_runtime();
	let rounds: u32 = <Runtime as indiv_pallet_game::Config>::MaxRounds::get();
	let group_size: u32 = <Runtime as indiv_pallet_game::Config>::MaxGroupSize::get();
	let phases = <Runtime as indiv_pallet_game::Config>::DefaultPhaseDurations::get();
	let co_players = rounds * (group_size - 1);

	let paths = new_test_ext().execute_with(|| Paths::build(&limits, rounds, group_size));
	let hooks = GameHooks::of_runtime(rounds, group_size);
	let reports_per_credit_tree =
		PerBlock { units: u64::from(AWARDS_PER_TREE / co_players), bound: Bound::Credits };

	// The declared weight of a report covers the early enactment of every co-player.
	let budget = OcwWeightBudget::from_normal_max::<Runtime>();
	for path in paths.sign_ups.all().chain(paths.reports.all()) {
		budget.assert_fits(path.name, path.declared);
	}
	for (name, work) in [("shuffle", hooks.shuffle), ("player process", hooks.player_process)] {
		assert!(
			limits.hook_units_per_block(BlockLoad::Full, work).units >= u64::from(group_size),
			"a full block cannot {name} one group"
		);
	}

	let estimate = Estimate {
		limits,
		phases,
		rounds,
		group_size,
		co_players,
		paths,
		hooks,
		reports_per_credit_tree,
	};
	estimate.print_header();
	estimate.print_players_per_window();
	estimate.print_phase_durations();
	estimate.print_phase_windows();
}
