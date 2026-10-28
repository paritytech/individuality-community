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

//! The block limits, how much of a phase's work a block takes, and how many blocks a phase takes.

use super::{hooks::HookWork, paths::TxPath, *};
use crate::{async_backing::MILLISECS_PER_BLOCK, AVERAGE_ON_INITIALIZE_RATIO};
use frame_support::dispatch::DispatchClass;
use indiv_pallet_game::{GAME_PROCESS_SKIPPED_BLOCK, OP_UPPER_BOUND};

/// How much of a block transactions use, which sets the weight left to the game hooks.
#[derive(Clone, Copy)]
pub(super) enum BlockLoad {
	/// No transactions: `on_poll` takes half of the block and `on_idle` half of the rest.
	Empty,
	/// Transactions use the weight `on_poll` leaves, so `on_idle` gets nothing.
	Full,
}

/// The limit that stops a block from taking more of a phase's work.
#[derive(Clone, Copy)]
pub(super) enum Bound {
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
pub(super) struct PerBlock {
	pub(super) units: u64,
	pub(super) bound: Bound,
}

impl PerBlock {
	pub(super) fn min(self, other: Self) -> Self {
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
pub(super) struct Limits {
	/// The weight `on_poll` and `on_idle` share in a block.
	pub(super) hooks: Weight,
	/// The weight of all `Normal` transactions in a block.
	pub(super) normal: Weight,
	/// The encoded length of all `Normal` transactions in a block.
	pub(super) normal_length: u64,
	/// The weight every transaction pays in addition to its call and extensions.
	pub(super) base_extrinsic: Weight,
}

impl Limits {
	pub(super) fn of_runtime() -> Self {
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

	/// The units of hook `work` a block processes, after the overhead of each hook call.
	pub(super) fn hook_units_per_block(&self, load: BlockLoad, work: HookWork) -> PerBlock {
		let on_poll = self.hooks / 2;
		let in_on_poll = hook_call_units(on_poll, work);
		match load {
			BlockLoad::Full => in_on_poll,
			BlockLoad::Empty => {
				let in_on_idle = hook_call_units(self.hooks.saturating_sub(on_poll) / 2, work);
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
	pub(super) fn txs_per_block(&self, path: &TxPath) -> PerBlock {
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

/// The units of `work` one hook call processes in `budget`.
/// A unit takes one loop iteration of the call.
fn hook_call_units(budget: Weight, work: HookWork) -> PerBlock {
	units_in(budget, work.overhead, work.unit)
		.min(PerBlock { units: u64::from(OP_UPPER_BOUND), bound: Bound::Iterations })
}

/// The blocks in a phase of `secs` seconds.
pub(super) fn window_blocks(secs: u32) -> u64 {
	u64::from(secs) * 1_000 / MILLISECS_PER_BLOCK
}

/// The blocks in a phase of `secs` seconds that the game hooks do not skip.
pub(super) fn hook_window_blocks(secs: u32) -> u64 {
	let every = u64::from(GAME_PROCESS_SKIPPED_BLOCK);
	window_blocks(secs) * (every - 1) / every
}

/// The blocks a hook phase takes for `units` of work, including the blocks the game skips.
pub(super) fn hook_blocks(units: u64, per_block: PerBlock) -> u64 {
	let every = u64::from(GAME_PROCESS_SKIPPED_BLOCK);
	(units.div_ceil(per_block.units) * every).div_ceil(every - 1)
}

/// The blocks the player process takes for `players` at `hook` units per block with every credit
/// backfilled. The hooks skip blocks, but the credit trees commit in every block.
pub(super) fn player_process_blocks(players: u64, hook: PerBlock, credits: PerBlock) -> u64 {
	hook_blocks(players, hook).max(players.div_ceil(credits.units))
}

/// The phase window, in seconds rounded up to a minute, that holds `blocks`.
pub(super) fn window_secs(blocks: u64) -> u64 {
	(blocks * MILLISECS_PER_BLOCK / 1_000).div_ceil(60) * 60
}
