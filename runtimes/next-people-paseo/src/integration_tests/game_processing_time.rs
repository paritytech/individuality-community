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
//! The benchmarking build widens `MaxRounds` and `MaxGroupSize`, so it ignores the test.
//!
//! Print the estimate with:
//! `SKIP_WASM_BUILD=1 cargo test -p next-people-paseo-runtime --lib game_processing_time --
//! --nocapture`

use super::*;
use crate::{async_backing::MILLISECS_PER_BLOCK, AVERAGE_ON_INITIALIZE_RATIO};
use frame_support::dispatch::{DispatchClass, GetDispatchInfo};
use indiv_pallet_game::{
	FullReport, Report, WeightInfo as _, GAME_PROCESS_SKIPPED_BLOCK, MAX_GAME_AIRDROPS,
	OP_UPPER_BOUND, PLAYER_PROCESS_STEP2_CHUNK,
};
use indiv_pallet_nft_credits::{AWARDS_PER_TREE, MAX_PENDING_CREDIT_TREES};
use indiv_pallet_people::extension::{AsPerson, AsPersonInfo};
use indiv_support::{crypto::BandersnatchSuite, weight_budget::OcwWeightBudget};
use verifiable::ring::ring_signature_size;

type GameWeights = <Runtime as indiv_pallet_game::Config>::WeightInfo;

/// The player counts to print the phase durations for.
const PLAYER_COUNTS: [u64; 3] = [10_000, 100_000, 1_000_000];

/// How much of a block transactions use, which sets the weight left to the game hooks.
#[derive(Clone, Copy)]
enum BlockLoad {
	/// No transactions: `on_poll` takes half of the block and `on_idle` half of the rest.
	Empty,
	/// Transactions use the weight `on_poll` leaves, so `on_idle` gets nothing.
	Full,
}

/// A way to submit a sign-up or a report, with what `CheckWeight` books for one transaction.
struct TxPath {
	name: &'static str,
	/// The weight a block books after the refund, with the encoded length as proof size.
	weight: Weight,
	/// The weight before the refund, with the encoded length as proof size.
	/// It must fit one extrinsic.
	declared: Weight,
	/// The encoded length, which counts toward the `Normal` block length.
	length: u64,
}

impl TxPath {
	/// The path of `uxt`, which pays its declared call weight.
	fn new(name: &'static str, limits: &Limits, uxt: &UncheckedExtrinsic) -> Self {
		Self::refunded(name, limits, uxt, uxt.function.get_dispatch_info().call_weight)
	}

	/// The path of `uxt`, whose call weight is refunded to `call_weight` after dispatch.
	fn refunded(
		name: &'static str,
		limits: &Limits,
		uxt: &UncheckedExtrinsic,
		call_weight: Weight,
	) -> Self {
		let length = uxt.encoded_size() as u64;
		let extension_weight = uxt.extension_weight();
		let declared = uxt
			.function
			.get_dispatch_info()
			.call_weight
			.saturating_add(extension_weight)
			.saturating_add_proof_size(length);
		let weight = limits
			.base_extrinsic
			.saturating_add(extension_weight)
			.saturating_add(call_weight)
			.saturating_add_proof_size(length);
		Self { name, weight, declared, length }
	}

	fn describe(&self) -> String {
		format!("{} {}B", describe(self.weight), self.length)
	}
}

/// The sign-up and report paths the estimate compares.
struct Paths {
	sign_up_by_account: TxPath,
	sign_up_by_account_with_vrf_airdrops: TxPath,
	sign_up_by_account_with_proof_airdrops: TxPath,
	sign_up_by_alias_account: TxPath,
	sign_up_by_alias_account_with_airdrops: TxPath,
	sign_up_by_ring_proof: TxPath,
	report_by_account: TxPath,
	report_by_alias_account: TxPath,
	report_by_ring_proof: TxPath,
}

impl Paths {
	/// Builds the transaction of each path. The transaction extensions read the externalities.
	fn build(limits: &Limits, rounds: u32, group_size: u32) -> Self {
		let player = Sr25519Keyring::Alice.pair();
		let event_ids = (0..MAX_GAME_AIRDROPS)
			.map(|index| airdrop_event_id_for(0, index))
			.collect::<Vec<_>>();
		let vrf_airdrops = build_account_airdrop_vrfs(&player, &event_ids);
		let proof_airdrops = AirdropVrfs::Alias {
			proofs: (0..MAX_GAME_AIRDROPS)
				.map(|_| ring_proof())
				.collect::<Vec<_>>()
				.try_into()
				.expect("the proofs are bounded by MAX_GAME_AIRDROPS"),
			ring_index: 0,
			revision: 0,
		};
		let sign_up_with_account = |airdrops| {
			RuntimeCall::Game(indiv_pallet_game::Call::sign_up_with_account {
				identifier_key: [0; 65],
				airdrops,
			})
		};
		let sign_up_with_alias = |airdrops| {
			RuntimeCall::Game(indiv_pallet_game::Call::sign_up_with_alias {
				identifier_key: [0; 65],
				statement_account: Sr25519Keyring::Alice.to_account_id(),
				sig: MultiSignature::from(player.sign(&[])),
				airdrops,
			})
		};
		let by_ring_proof = |call: RuntimeCall| {
			let mut tx_ext = base_tx_ext(call.clone());
			tx_ext.0 .0 .2 = AsPerson::new(Some(AsPersonInfo::AsPersonalAliasWithProof(
				ring_proof(),
				0,
				0,
				[0; 32],
			)));
			finalize_uxt(call, tx_ext)
		};
		// A full report covers every co-player of every round.
		let full_report: FullReport<Runtime> = (0..rounds)
			.map(|_| {
				(1..group_size)
					.map(|_| Report::Person)
					.collect::<Vec<_>>()
					.try_into()
					.expect("a round reports fewer players than the group size")
			})
			.collect::<Vec<_>>()
			.try_into()
			.expect("a report has one entry per round");
		let report = RuntimeCall::Game(indiv_pallet_game::Call::report { full_report });
		let refunded_report = GameWeights::report(rounds * (group_size - 1), 1);

		Self {
			sign_up_by_account: TxPath::new(
				"account, no airdrop",
				limits,
				&build_signed_ext(&player, sign_up_with_account(None)),
			),
			sign_up_by_account_with_vrf_airdrops: TxPath::new(
				"account, max airdrops by VRF",
				limits,
				&build_signed_ext(&player, sign_up_with_account(Some(vrf_airdrops))),
			),
			sign_up_by_account_with_proof_airdrops: TxPath::new(
				"account, max airdrops by alias proof",
				limits,
				&build_signed_ext(&player, sign_up_with_account(Some(proof_airdrops.clone()))),
			),
			sign_up_by_alias_account: TxPath::new(
				"alias account, no airdrop",
				limits,
				&build_signed_as_alias_with_account_ext(&player, sign_up_with_alias(None)),
			),
			sign_up_by_alias_account_with_airdrops: TxPath::new(
				"alias account, max airdrops",
				limits,
				&build_signed_as_alias_with_account_ext(
					&player,
					sign_up_with_alias(Some(proof_airdrops)),
				),
			),
			sign_up_by_ring_proof: TxPath::new(
				"alias ring proof, no airdrop",
				limits,
				&by_ring_proof(sign_up_with_alias(None)),
			),
			report_by_account: TxPath::refunded(
				"account",
				limits,
				&build_signed_ext(&player, report.clone()),
				refunded_report,
			),
			report_by_alias_account: TxPath::refunded(
				"alias account",
				limits,
				&build_signed_as_alias_with_account_ext(&player, report.clone()),
				refunded_report,
			),
			report_by_ring_proof: TxPath::refunded(
				"alias ring proof",
				limits,
				&by_ring_proof(report),
				refunded_report,
			),
		}
	}

	fn sign_ups(&self) -> [&TxPath; 6] {
		[
			&self.sign_up_by_account,
			&self.sign_up_by_account_with_vrf_airdrops,
			&self.sign_up_by_account_with_proof_airdrops,
			&self.sign_up_by_alias_account,
			&self.sign_up_by_alias_account_with_airdrops,
			&self.sign_up_by_ring_proof,
		]
	}

	fn reports(&self) -> [&TxPath; 3] {
		[&self.report_by_account, &self.report_by_alias_account, &self.report_by_ring_proof]
	}
}

/// A ring proof of one context. Only its length matters to the estimate.
fn ring_proof<P: TryFrom<Vec<u8>>>() -> P {
	P::try_from(vec![0; ring_signature_size::<BandersnatchSuite>(1)])
		.unwrap_or_else(|_| panic!("a ring proof of one context fits the proof bound"))
}

/// The limit that stops a block from taking more of a phase's work.
#[derive(Clone, Copy)]
enum Bound {
	RefTime,
	ProofSize,
	/// The `Normal` block length.
	Length,
	/// The NFT claim credits a block commits.
	Credits,
	/// `OP_UPPER_BOUND`, the loop iterations of one hook call.
	Iterations,
}

/// The units of a phase's work a block takes, and the limit that stops it.
#[derive(Clone, Copy)]
struct PerBlock {
	units: u64,
	bound: Bound,
}

impl PerBlock {
	fn min(self, other: Self) -> Self {
		if other.units < self.units {
			other
		} else {
			self
		}
	}
}

impl core::fmt::Display for PerBlock {
	fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
		let bound = match self.bound {
			Bound::RefTime => "time",
			Bound::ProofSize => "PoV",
			Bound::Length => "length",
			Bound::Credits => "credits",
			Bound::Iterations => "iterations",
		};
		f.pad(&format!("{} {bound}", self.units))
	}
}

/// The block limits the game phases share.
struct Limits {
	/// The weight `on_poll` and `on_idle` share in a block.
	hooks: Weight,
	/// The weight of all `Normal` transactions in a block.
	normal: Weight,
	/// The encoded length of all `Normal` transactions in a block.
	normal_length: u64,
	/// The weight every transaction pays in addition to its call and extensions.
	base_extrinsic: Weight,
}

impl Limits {
	fn of_runtime() -> Self {
		let block_weights = RuntimeBlockWeights::get();
		let normal = block_weights.get(DispatchClass::Normal);
		Self {
			// `on_poll` gets the block weight that `on_initialize` and the inherents leave.
			hooks: block_weights
				.max_block
				.saturating_sub(block_weights.base_block)
				.saturating_sub(AVERAGE_ON_INITIALIZE_RATIO * block_weights.max_block),
			normal: normal.max_total.expect("the normal class has a maximum total weight"),
			normal_length: u64::from(*RuntimeBlockLength::get().max.get(DispatchClass::Normal)),
			base_extrinsic: normal.base_extrinsic,
		}
	}

	/// The `unit`s of hook work a block processes, after the `overhead` of each hook call.
	fn hook_units_per_block(&self, load: BlockLoad, overhead: Weight, unit: Weight) -> PerBlock {
		let on_poll = self.hooks / 2;
		let in_on_poll = hook_call_units(on_poll, overhead, unit);
		match load {
			BlockLoad::Full => in_on_poll,
			BlockLoad::Empty => {
				let in_on_idle =
					hook_call_units(self.hooks.saturating_sub(on_poll) / 2, overhead, unit);
				// The bound of the hook that takes more units names the limit of the block.
				let bound = if in_on_idle.units > in_on_poll.units {
					in_on_idle.bound
				} else {
					in_on_poll.bound
				};
				PerBlock { units: in_on_poll.units.saturating_add(in_on_idle.units), bound }
			},
		}
	}

	/// The transactions of `path` a block includes.
	fn txs_per_block(&self, path: &TxPath) -> PerBlock {
		let by_length = PerBlock {
			units: self.normal_length.checked_div(path.length).unwrap_or(u64::MAX),
			bound: Bound::Length,
		};
		units_in(self.normal, Weight::zero(), path.weight).min(by_length)
	}
}

/// The number of `unit`s that fit in `budget` after `overhead`, in both weight dimensions.
fn units_in(budget: Weight, overhead: Weight, unit: Weight) -> PerBlock {
	let left = budget.saturating_sub(overhead);
	let in_dimension = |left: u64, unit: u64| left.checked_div(unit).unwrap_or(u64::MAX);
	let by_ref_time =
		PerBlock { units: in_dimension(left.ref_time(), unit.ref_time()), bound: Bound::RefTime };
	let by_proof_size = PerBlock {
		units: in_dimension(left.proof_size(), unit.proof_size()),
		bound: Bound::ProofSize,
	};
	by_ref_time.min(by_proof_size)
}

/// The `unit`s one hook call processes in `budget` after `overhead`.
/// A unit takes one loop iteration of the call.
fn hook_call_units(budget: Weight, overhead: Weight, unit: Weight) -> PerBlock {
	units_in(budget, overhead, unit)
		.min(PerBlock { units: u64::from(OP_UPPER_BOUND), bound: Bound::Iterations })
}

/// The blocks in a phase of `secs` seconds.
fn window_blocks(secs: u32) -> u64 {
	u64::from(secs) * 1_000 / MILLISECS_PER_BLOCK
}

/// The blocks in a phase of `secs` seconds that the game hooks do not skip.
fn hook_window_blocks(secs: u32) -> u64 {
	let every = u64::from(GAME_PROCESS_SKIPPED_BLOCK);
	window_blocks(secs) * (every - 1) / every
}

/// The blocks a hook phase takes for `units` of work, including the blocks the game skips.
fn hook_blocks(units: u64, per_block: PerBlock) -> u64 {
	let every = u64::from(GAME_PROCESS_SKIPPED_BLOCK);
	(units.div_ceil(per_block.units) * every).div_ceil(every - 1)
}

/// The blocks the player process takes for `players` at `hook` units per block with every credit
/// backfilled. The hooks skip blocks, but the credit trees commit in every block.
fn player_process_blocks(players: u64, hook: PerBlock, credits: PerBlock) -> u64 {
	hook_blocks(players, hook).max(players.div_ceil(credits.units))
}

fn duration(blocks: u64) -> String {
	duration_secs(blocks * MILLISECS_PER_BLOCK / 1_000)
}

fn duration_secs(secs: u64) -> String {
	match secs {
		0..60 => format!("{secs}s"),
		60..3_600 => format!("{:.1}m", secs as f64 / 60.0),
		_ => format!("{:.1}h", secs as f64 / 3_600.0),
	}
}

fn describe(weight: Weight) -> String {
	format!(
		"{:.2}ms {:.1}KiB",
		weight.ref_time() as f64 / 1_000_000_000.0,
		weight.proof_size() as f64 / 1_024.0
	)
}

/// The phase window, in seconds rounded up to a minute, that holds `blocks`.
fn window_secs(blocks: u64) -> u64 {
	(blocks * MILLISECS_PER_BLOCK / 1_000).div_ceil(60) * 60
}

/// A phase duration for `blocks`, marked with `*` when it overruns its window of `window` blocks.
fn fit(blocks: u64, window: u64) -> String {
	let mark = if blocks > window { "*" } else { "" };
	format!("{}{mark}", duration(blocks))
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
	let max_airdrops = u32::from(MAX_GAME_AIRDROPS);
	// The co-players a report covers, each of whom a `Person` vote awards a credit to.
	let co_players = rounds * (group_size - 1);

	let paths = new_test_ext().execute_with(|| Paths::build(&limits, rounds, group_size));
	let (sign_ups, reports) = (paths.sign_ups(), paths.reports());
	// A report awards up to one credit per co-player, and a block commits one tree of credits.
	let reports_per_credit_tree =
		PerBlock { units: u64::from(AWARDS_PER_TREE / co_players), bound: Bound::Credits };

	let get_game = GameWeights::get_game();
	let shuffle_overhead = get_game
		.saturating_add(GameWeights::shuffles_base())
		.saturating_add(GameWeights::on_game_cancelled(max_airdrops));
	let shuffle_insert = GameWeights::shuffle_step_insert(rounds);
	let shuffle = shuffle_insert
		.saturating_add(GameWeights::shuffle_step_retrieve(rounds))
		.saturating_add(GameWeights::shuffle_step_compute_weights(rounds * group_size));
	let process_overhead = get_game.saturating_add(
		GameWeights::player_process_step1().max(GameWeights::player_process_step2()),
	);
	let process_step1 = GameWeights::player_process_step1_inner_loop(rounds);
	// Step 2 clears `PLAYER_PROCESS_STEP2_CHUNK` entries of each map per loop, and a player has an
	// index entry per round.
	let process = process_step1.saturating_add(
		GameWeights::player_process_step2_inner_loop().saturating_mul(u64::from(rounds)) /
			u64::from(PLAYER_PROCESS_STEP2_CHUNK),
	);

	// The declared weight of a report covers the early enactment of every co-player.
	let budget = OcwWeightBudget::from_normal_max::<Runtime>();
	for path in sign_ups.into_iter().chain(reports) {
		budget.assert_fits(path.name, path.declared);
	}
	for (name, overhead, unit) in
		[("shuffle", shuffle_overhead, shuffle), ("player process", process_overhead, process)]
	{
		assert!(
			limits.hook_units_per_block(BlockLoad::Full, overhead, unit).units >=
				u64::from(group_size),
			"a full block cannot {name} one group"
		);
	}

	println!("=== Game processing time estimate (next-people-paseo weights and configuration) ===");
	println!(
		"block: every {MILLISECS_PER_BLOCK}ms, hooks share {}, normal transactions {} {}B",
		describe(limits.hooks),
		describe(limits.normal),
		limits.normal_length
	);
	println!(
		"game: rounds={rounds} max_group_size={group_size} co-players per report={co_players}"
	);
	println!(
		"phases: registration={}s shuffle={}s reporting={}s player_process={}s",
		phases.registration, phases.shuffle, phases.reporting, phases.player_process
	);
	println!(
		"NFT claim credits: {AWARDS_PER_TREE} per block, {} buffered ahead at most",
		AWARDS_PER_TREE * (MAX_PENDING_CREDIT_TREES - 1)
	);
	println!(
		"each stored player who does not play: shuffle {}, player process {}",
		describe(shuffle_insert),
		describe(process_step1)
	);

	println!();
	println!("--- Players per phase window ---");
	println!(
		"{:<16}{:<40}{:>28}{:>18}{:>14}",
		"phase", "path", "per player", "per block", "max players"
	);
	let registration_blocks = window_blocks(phases.registration);
	for path in sign_ups {
		let per_block = limits.txs_per_block(path);
		println!(
			"{:<16}{:<40}{:>28}{:>18}{:>14}",
			"registration",
			path.name,
			path.describe(),
			per_block,
			per_block.units * registration_blocks
		);
	}
	let reporting_blocks = window_blocks(phases.reporting);
	for path in reports {
		let per_block = limits.txs_per_block(path).min(reports_per_credit_tree);
		println!(
			"{:<16}{:<40}{:>28}{:>18}{:>14}",
			"reporting",
			path.name,
			path.describe(),
			per_block,
			per_block.units * reporting_blocks
		);
	}
	for (phase, secs, overhead, unit) in [
		("shuffle", phases.shuffle, shuffle_overhead, shuffle),
		("player process", phases.player_process, process_overhead, process),
	] {
		for (load, name) in [(BlockLoad::Full, "full blocks"), (BlockLoad::Empty, "empty blocks")] {
			let per_block = limits.hook_units_per_block(load, overhead, unit);
			println!(
				"{:<16}{:<40}{:>28}{:>18}{:>14}",
				phase,
				name,
				describe(unit),
				per_block,
				per_block.units * hook_window_blocks(secs)
			);
		}
	}
	let process_in_full_blocks =
		limits.hook_units_per_block(BlockLoad::Full, process_overhead, process);
	println!(
		"{:<16}{:<40}{:>28}{:>18}{:>14}",
		"player process",
		"full blocks, all credits backfilled",
		format!("{co_players} credits"),
		reports_per_credit_tree,
		(process_in_full_blocks.units * hook_window_blocks(phases.player_process))
			.min(reports_per_credit_tree.units * window_blocks(phases.player_process))
	);

	println!();
	println!("--- Phase durations ---");
	println!(
		"registration by {}, reporting by {}, shuffle and player process from empty to full \
		blocks, * overruns the default window",
		paths.sign_up_by_account.name, paths.report_by_account.name
	);
	println!(
		"{:>10}{:>16}{:>22}{:>16}{:>22}",
		"players", "registration", "shuffle", "reporting", "player process"
	);
	let hook_range = |players: u64, secs: u32, overhead: Weight, unit: Weight| {
		let window = window_blocks(secs);
		let full =
			hook_blocks(players, limits.hook_units_per_block(BlockLoad::Full, overhead, unit));
		let empty =
			hook_blocks(players, limits.hook_units_per_block(BlockLoad::Empty, overhead, unit));
		format!("{}-{}", fit(empty, window), fit(full, window))
	};
	for players in PLAYER_COUNTS {
		let sign_up_blocks =
			players.div_ceil(limits.txs_per_block(&paths.sign_up_by_account).units);
		let report_blocks = players.div_ceil(
			limits
				.txs_per_block(&paths.report_by_account)
				.min(reports_per_credit_tree)
				.units,
		);
		println!(
			"{:>10}{:>16}{:>22}{:>16}{:>22}",
			players,
			fit(sign_up_blocks, registration_blocks),
			hook_range(players, phases.shuffle, shuffle_overhead, shuffle),
			fit(report_blocks, reporting_blocks),
			hook_range(players, phases.player_process, process_overhead, process)
		);
	}

	println!();
	println!("--- Phase windows by player count ---");
	println!(
		"the windows to set with `set_game_phases`, rounded up to a minute: shuffle and player \
		process in full blocks with every credit backfilled, sign-ups and reports by {} / by {}",
		paths.sign_up_by_account.name, paths.sign_up_by_ring_proof.name
	);
	println!(
		"{:>10}{:>16}{:>10}{:>16}{:>16}{:>16}",
		"players", "registration", "shuffle", "reporting", "player process", "game"
	);
	for players in PLAYER_COUNTS {
		let registration =
			|path: &TxPath| window_secs(players.div_ceil(limits.txs_per_block(path).units));
		let reporting = |path: &TxPath| {
			window_secs(
				players.div_ceil(limits.txs_per_block(path).min(reports_per_credit_tree).units),
			)
		};
		let shuffle = window_secs(hook_blocks(
			players,
			limits.hook_units_per_block(BlockLoad::Full, shuffle_overhead, shuffle),
		));
		let player_process = window_secs(player_process_blocks(
			players,
			process_in_full_blocks,
			reports_per_credit_tree,
		));
		let by_account =
			(registration(&paths.sign_up_by_account), reporting(&paths.report_by_account));
		let by_ring_proof =
			(registration(&paths.sign_up_by_ring_proof), reporting(&paths.report_by_ring_proof));
		let game = |(registration, reporting): (u64, u64)| {
			registration +
				shuffle + u64::from(phases.post_shuffle_margin) +
				reporting + player_process
		};
		println!(
			"{:>10}{:>16}{:>10}{:>16}{:>16}{:>16}",
			players,
			format!("{} / {}", duration_secs(by_account.0), duration_secs(by_ring_proof.0)),
			duration_secs(shuffle),
			format!("{} / {}", duration_secs(by_account.1), duration_secs(by_ring_proof.1)),
			duration_secs(player_process),
			format!("{} / {}", duration_secs(game(by_account)), duration_secs(game(by_ring_proof)))
		);
	}
}
