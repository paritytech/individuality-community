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
//!   commits one tree of `AWARDS_PER_TREE` credits. This bounds the reports per block. When every
//!   credit is backfilled, it also bounds the players the player process takes per block.
//! * The offchain worker runs the shuffle and the player process in step calls, `advance_shuffle`
//!   and `process_players`. A block includes at most one step call. A step call meters its work
//!   against the step budget, half of the `Normal` maximum extrinsic weight, and runs at most
//!   `OP_UPPER_BOUND` loop iterations. Player process step 1 and step 2 take separate calls.
//! * A phase change takes `PHASE_CHANGE_BLOCKS` blocks from the shuffle and from the player
//!   process. `end_registration` and `end_reporting` land in the first block of the phase. The
//!   first step call after them misses the next block about half of the time. The e2e suite `game`
//!   measures this on a local network.
//! * A game schedules `GAME_AIRDROPS` airdrop events, as in production, and a sign-up enters each.
//!   The airdrop sweep compares other counts.
//! * The shuffle insert step and the player process step 1 iterate over every stored player,
//!   including the players who did not sign up for this game. The estimate assumes that every
//!   stored player plays. Each other stored player costs one more step, which the estimate prints.
//!
//! The estimate is split into what a transaction costs ([`paths`]), what the game steps cost
//! ([`steps`]), how much of each fits in a block ([`limits`]) and the printed tables ([`tables`]).
//!
//! The benchmarking build widens `MaxRounds` and `MaxGroupSize`, so it ignores the test.
//!
//! Print the estimate with:
//! `SKIP_WASM_BUILD=1 cargo test -p next-people-paseo-runtime --lib game_processing_time --
//! --nocapture`

mod limits;
mod paths;
mod steps;
mod tables;

use super::*;
use indiv_pallet_nft_credits::AWARDS_PER_TREE;
use indiv_support::weight_budget::OcwWeightBudget;
use limits::{Bound, Limits, PerBlock};
use paths::Paths;
use steps::GameSteps;

type GameWeights = <Runtime as indiv_pallet_game::Config>::WeightInfo;

/// The player counts to print the phase windows for.
const PLAYER_COUNTS: [u64; 3] = [10_000, 100_000, 1_000_000];

/// The player count the airdrop sweep prints the registration of.
const AIRDROP_SWEEP_PLAYERS: u64 = 10_000;

/// The airdrop events a production game schedules. The sign-up path names state this count.
const GAME_AIRDROPS: u8 = 1;

/// What the estimate prints from: the runtime's limits and game configuration, and the cost of
/// each phase's work.
struct Estimate {
	limits: Limits,
	/// The runtime's default margin between the shuffle deadline and the game date.
	post_shuffle_margin: u32,
	rounds: u32,
	group_size: u32,
	/// The co-players a report covers, each of whom a `Person` vote awards a credit to.
	co_players: u32,
	paths: Paths,
	steps: GameSteps,
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
	let post_shuffle_margin =
		<Runtime as indiv_pallet_game::Config>::DefaultPhaseDurations::get().post_shuffle_margin;
	let co_players = rounds * (group_size - 1);

	let paths = new_test_ext().execute_with(|| Paths::build(&limits, rounds, group_size));
	let steps = GameSteps::of_runtime(rounds, group_size);
	let reports_per_credit_tree =
		PerBlock { units: u64::from(AWARDS_PER_TREE / co_players), bound: Bound::Credits };

	// The declared weight of a report covers the early enactment of every co-player.
	let budget = OcwWeightBudget::from_normal_max::<Runtime>();
	for path in paths
		.sign_ups
		.all()
		.chain(paths.reports.all())
		.chain(paths.airdrop_sweep.iter().flat_map(|row| row.all()))
	{
		budget.assert_fits(path.name, path.declared);
	}
	for (name, work) in [
		("shuffle", steps.shuffle),
		("player process step 1", steps.process_step1),
		("player process step 2", steps.process_step2),
	] {
		assert!(
			limits.step_units(work).units >= u64::from(group_size),
			"a {name} call cannot take one group"
		);
	}

	let estimate = Estimate {
		limits,
		post_shuffle_margin,
		rounds,
		group_size,
		co_players,
		paths,
		steps,
		reports_per_credit_tree,
	};
	estimate.print_header();
	estimate.print_players_per_block();
	estimate.print_phase_windows();
	estimate.print_airdrop_sweep();
}
