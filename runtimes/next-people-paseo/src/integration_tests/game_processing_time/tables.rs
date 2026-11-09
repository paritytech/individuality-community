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
	limits::{player_process_blocks, shuffle_blocks, window_secs, PHASE_CHANGE_BLOCKS},
	paths::TxPath,
	*,
};
use crate::async_backing::MILLISECS_PER_BLOCK;
use indiv_pallet_nft_credits::MAX_PENDING_CREDIT_TREES;

impl Estimate {
	/// The block and game configuration the estimate runs on.
	pub(super) fn print_header(&self) {
		let Self { limits, rounds, group_size, co_players, steps, .. } = self;
		println!(
			"=== Game processing time estimate (next-people-paseo weights and configuration) ==="
		);
		println!(
			"block: every {MILLISECS_PER_BLOCK}ms, one game step call of {}, normal transactions \
			{} {}B",
			describe(limits.step_budget),
			describe(limits.normal),
			limits.normal_length
		);
		println!(
			"game: rounds={rounds} max_group_size={group_size} co-players per report={co_players} \
			airdrops={GAME_AIRDROPS}"
		);
		println!(
			"phase change: {PHASE_CHANGE_BLOCKS} blocks of the shuffle and of the player process"
		);
		println!(
			"NFT claim credits: {AWARDS_PER_TREE} per block, {} buffered ahead at most",
			AWARDS_PER_TREE * (MAX_PENDING_CREDIT_TREES - 1)
		);
		println!(
			"each stored player who does not play: shuffle {}, player process {}",
			describe(steps.shuffle_insert),
			describe(steps.process_step1.unit)
		);
	}

	/// The players each phase takes per block.
	pub(super) fn print_players_per_block(&self) {
		let Self { limits, co_players, paths, steps, reports_per_credit_tree, .. } = self;
		println!();
		println!("--- Players per block ---");
		print_row("phase", "path", "per player", "per block");
		for path in paths.sign_ups.all() {
			print_row("registration", path.name, &path.describe(), limits.txs_per_block(path));
		}
		for path in paths.reports.all() {
			let per_block = limits.txs_per_block(path).min(*reports_per_credit_tree);
			print_row("reporting", path.name, &path.describe(), per_block);
		}
		for (phase, name, work) in [
			("shuffle", "advance_shuffle", steps.shuffle),
			("player process", "process_players, step 1", steps.process_step1),
			("player process", "process_players, step 2", steps.process_step2),
		] {
			print_row(phase, name, &describe(work.unit), limits.step_units(work));
		}
		print_row(
			"player process",
			"step 1, all credits backfilled",
			&format!("{co_players} credits"),
			reports_per_credit_tree,
		);
	}

	/// The phase windows to set for each of `PLAYER_COUNTS`.
	pub(super) fn print_phase_windows(&self) {
		let Self { limits, post_shuffle_margin, paths, steps, reports_per_credit_tree, .. } = self;
		println!();
		println!("--- Phase windows by player count ---");
		println!(
			"the windows to set with `set_game_phases`, rounded up to a minute: sign-ups and \
			reports by {} / by {}, shuffle and player process with every credit backfilled, game \
			with the runtime's post-shuffle margin of {post_shuffle_margin}s",
			paths.sign_ups.without_proof.account.name, paths.sign_ups.with_proof.ring_proof.name
		);
		println!(
			"{:>10}{:>16}{:>10}{:>16}{:>16}{:>16}",
			"players", "registration", "shuffle", "reporting", "player process", "game"
		);
		let shuffle_step = limits.step_units(steps.shuffle);
		let process_step1 = limits.step_units(steps.process_step1);
		let process_step2 = limits.step_units(steps.process_step2);
		for players in PLAYER_COUNTS {
			let registration =
				|path: &TxPath| window_secs(players.div_ceil(limits.txs_per_block(path).units));
			let reporting = |path: &TxPath| {
				window_secs(
					players
						.div_ceil(limits.txs_per_block(path).min(*reports_per_credit_tree).units),
				)
			};
			let shuffle = window_secs(shuffle_blocks(players, shuffle_step));
			let player_process = window_secs(player_process_blocks(
				players,
				process_step1,
				*reports_per_credit_tree,
				process_step2,
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
					shuffle + u64::from(*post_shuffle_margin) +
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

	/// The registration of `AIRDROP_SWEEP_PLAYERS` by the airdrop entries each sign-up carries.
	pub(super) fn print_airdrop_sweep(&self) {
		let Self { limits, paths, .. } = self;
		let Some(first) = paths.airdrop_sweep.first() else { return };
		println!();
		println!("--- Airdrop sweep ---");
		println!(
			"registration of {AIRDROP_SWEEP_PLAYERS} players who all sign up by one path, by the \
			airdrop entries each sign-up carries: sign-ups per block / registration. A player who \
			is not recognized enters by VRF, a recognized one by ring proof"
		);
		print!("{:>10}", "airdrops");
		for path in first.all() {
			print!("{:>24}", path.name);
		}
		println!();
		for row in &paths.airdrop_sweep {
			print!("{:>10}", row.airdrops);
			for path in row.all() {
				let per_block = limits.txs_per_block(path);
				let blocks = AIRDROP_SWEEP_PLAYERS.div_ceil(per_block.units);
				print!("{:>24}", format!("{per_block} / {}", duration(blocks)));
			}
			println!();
		}
	}
}

/// A row of the players per block table.
fn print_row(phase: &str, path: &str, per_player: &str, per_block: impl core::fmt::Display) {
	println!("{phase:<16}{path:<48}{per_player:>28}{per_block:>18}");
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
