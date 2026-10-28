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

//! The tables the estimate prints.

use super::{
	hooks::HookWork,
	limits::{hook_blocks, hook_window_blocks, player_process_blocks, window_blocks, window_secs},
	paths::TxPath,
	*,
};
use crate::async_backing::MILLISECS_PER_BLOCK;
use indiv_pallet_nft_credits::MAX_PENDING_CREDIT_TREES;

impl Estimate {
	/// The block, game and phase configuration the estimate runs on.
	pub(super) fn print_header(&self) {
		let Self { limits, phases, rounds, group_size, co_players, hooks, .. } = self;
		println!(
			"=== Game processing time estimate (next-people-paseo weights and configuration) ==="
		);
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
			describe(hooks.shuffle_insert),
			describe(hooks.process_step1)
		);
	}

	/// The players each phase takes per block and in its default window.
	pub(super) fn print_players_per_window(&self) {
		let Self { limits, phases, co_players, paths, hooks, reports_per_credit_tree, .. } = self;
		println!();
		println!("--- Players per phase window ---");
		println!(
			"{:<16}{:<40}{:>28}{:>18}{:>14}",
			"phase", "path", "per player", "per block", "max players"
		);
		let registration_blocks = window_blocks(phases.registration);
		for path in paths.sign_ups.all() {
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
		for path in paths.reports.all() {
			let per_block = limits.txs_per_block(path).min(*reports_per_credit_tree);
			println!(
				"{:<16}{:<40}{:>28}{:>18}{:>14}",
				"reporting",
				path.name,
				path.describe(),
				per_block,
				per_block.units * reporting_blocks
			);
		}
		for (phase, secs, work) in [
			("shuffle", phases.shuffle, hooks.shuffle),
			("player process", phases.player_process, hooks.player_process),
		] {
			for (load, name) in
				[(BlockLoad::Full, "full blocks"), (BlockLoad::Empty, "empty blocks")]
			{
				let per_block = limits.hook_units_per_block(load, work);
				println!(
					"{:<16}{:<40}{:>28}{:>18}{:>14}",
					phase,
					name,
					describe(work.unit),
					per_block,
					per_block.units * hook_window_blocks(secs)
				);
			}
		}
		let process_in_full_blocks =
			limits.hook_units_per_block(BlockLoad::Full, hooks.player_process);
		println!(
			"{:<16}{:<40}{:>28}{:>18}{:>14}",
			"player process",
			"full blocks, all credits backfilled",
			format!("{co_players} credits"),
			reports_per_credit_tree,
			(process_in_full_blocks.units * hook_window_blocks(phases.player_process))
				.min(reports_per_credit_tree.units * window_blocks(phases.player_process))
		);
	}

	/// How long each phase takes for each of `PLAYER_COUNTS`, against its default window.
	pub(super) fn print_phase_durations(&self) {
		let Self { limits, phases, paths, hooks, reports_per_credit_tree, .. } = self;
		println!();
		println!("--- Phase durations ---");
		println!(
			"registration by {}, reporting by {}, shuffle and player process from empty to full \
			blocks, * overruns the default window",
			paths.sign_ups.without_proof.account.name, paths.reports.account.name
		);
		println!(
			"{:>10}{:>16}{:>22}{:>16}{:>22}",
			"players", "registration", "shuffle", "reporting", "player process"
		);
		let registration_blocks = window_blocks(phases.registration);
		let reporting_blocks = window_blocks(phases.reporting);
		let hook_range = |players: u64, secs: u32, work: HookWork| {
			let window = window_blocks(secs);
			let full = hook_blocks(players, limits.hook_units_per_block(BlockLoad::Full, work));
			let empty = hook_blocks(players, limits.hook_units_per_block(BlockLoad::Empty, work));
			format!("{}-{}", fit(empty, window), fit(full, window))
		};
		for players in PLAYER_COUNTS {
			let sign_up_blocks =
				players.div_ceil(limits.txs_per_block(&paths.sign_ups.without_proof.account).units);
			let report_blocks = players.div_ceil(
				limits.txs_per_block(&paths.reports.account).min(*reports_per_credit_tree).units,
			);
			println!(
				"{:>10}{:>16}{:>22}{:>16}{:>22}",
				players,
				fit(sign_up_blocks, registration_blocks),
				hook_range(players, phases.shuffle, hooks.shuffle),
				fit(report_blocks, reporting_blocks),
				hook_range(players, phases.player_process, hooks.player_process)
			);
		}
	}

	/// The phase windows to set for each of `PLAYER_COUNTS`.
	pub(super) fn print_phase_windows(&self) {
		let Self { limits, phases, paths, hooks, reports_per_credit_tree, .. } = self;
		println!();
		println!("--- Phase windows by player count ---");
		println!(
			"the windows to set with `set_game_phases`, rounded up to a minute: shuffle and player \
			process in full blocks with every credit backfilled, sign-ups and reports by {} / by {}",
			paths.sign_ups.without_proof.account.name, paths.sign_ups.with_proof.ring_proof.name
		);
		println!(
			"{:>10}{:>16}{:>10}{:>16}{:>16}{:>16}",
			"players", "registration", "shuffle", "reporting", "player process", "game"
		);
		let process_in_full_blocks =
			limits.hook_units_per_block(BlockLoad::Full, hooks.player_process);
		for players in PLAYER_COUNTS {
			let registration =
				|path: &TxPath| window_secs(players.div_ceil(limits.txs_per_block(path).units));
			let reporting = |path: &TxPath| {
				window_secs(
					players
						.div_ceil(limits.txs_per_block(path).min(*reports_per_credit_tree).units),
				)
			};
			let shuffle = window_secs(hook_blocks(
				players,
				limits.hook_units_per_block(BlockLoad::Full, hooks.shuffle),
			));
			let player_process = window_secs(player_process_blocks(
				players,
				process_in_full_blocks,
				*reports_per_credit_tree,
			));
			let by_account = (
				registration(&paths.sign_ups.without_proof.account),
				reporting(&paths.reports.account),
			);
			let by_ring_proof = (
				registration(&paths.sign_ups.with_proof.ring_proof),
				reporting(&paths.reports.ring_proof),
			);
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
				format!(
					"{} / {}",
					duration_secs(game(by_account)),
					duration_secs(game(by_ring_proof))
				)
			);
		}
	}
}

/// A weight in milliseconds and KiB.
pub(super) fn describe(weight: Weight) -> String {
	format!(
		"{:.2}ms {:.1}KiB",
		weight.ref_time() as f64 / 1_000_000_000.0,
		weight.proof_size() as f64 / 1_024.0
	)
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

/// A phase duration for `blocks`, marked with `*` when it overruns its window of `window` blocks.
fn fit(blocks: u64, window: u64) -> String {
	let mark = if blocks > window { "*" } else { "" };
	format!("{}{mark}", duration(blocks))
}
