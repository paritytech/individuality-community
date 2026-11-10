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

use super::{paths::TxPath, steps::StepWork, *};
use crate::async_backing::MILLISECS_PER_BLOCK;
use frame_support::dispatch::DispatchClass;

/// The blocks a phase change takes from the shuffle and from the player process.
///
/// `end_registration` and `end_reporting` land in the first block of the phase. The first step
/// call after them misses the next block about half of the time. The e2e suite `game` measures
/// this on a local network.
pub(super) const PHASE_CHANGE_BLOCKS: u64 = 2;

/// The share of the blocks with a step due that include one, in percent.
///
/// The offchain worker of one block submits a step call and a later block includes it. The e2e
/// suite `game` prints the measured share as `blocks with a step due ... (<share>%)`. A run of
/// seven one-player games on a local network measured 19 of 25 blocks.
pub(super) const STEP_LANDING_PERCENT: u64 = 76;

/// The limit that stops a block from taking more of a phase's work.
#[derive(Clone, Copy)]
pub(super) enum Bound {
	RefTime,
	ProofSize,
	/// The `Normal` block length.
	Length,
	/// The NFT claim credits a block commits.
	Credits,
	/// `OP_UPPER_BOUND`, the loop iterations of one step call.
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
	/// The weight a step call meters its work against.
	pub(super) step_budget: Weight,
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
			step_budget: indiv_pallet_game::Pallet::<Runtime>::step_budget(),
			normal: normal.max_total.expect("the normal class has a maximum total weight"),
			normal_length: u64::from(*RuntimeBlockLength::get().max.get(DispatchClass::Normal)),
			base_extrinsic: normal.base_extrinsic,
		}
	}

	/// The players a step call of `work` processes. A block includes one step call.
	pub(super) fn step_units(&self, work: StepWork) -> PerBlock {
		units_in(self.step_budget, work.overhead, work.unit)
			.min(PerBlock { units: work.max_units, bound: Bound::Iterations })
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

/// The blocks `steps` step calls take when a step lands in `STEP_LANDING_PERCENT` of the blocks.
fn step_blocks(steps: u64) -> u64 {
	(steps * 100).div_ceil(STEP_LANDING_PERCENT)
}

/// The blocks the shuffle takes for `players`, from the end of the registration.
pub(super) fn shuffle_blocks(players: u64, per_step: PerBlock) -> u64 {
	PHASE_CHANGE_BLOCKS + step_blocks(players.div_ceil(per_step.units))
}

/// The blocks the player process takes for `players` with every credit backfilled, from the end
/// of the reporting. Step 1 takes the players that the step call and the credit trees allow.
/// Step 2 starts in the block after the last step 1 call.
pub(super) fn player_process_blocks(
	players: u64,
	step1: PerBlock,
	credits: PerBlock,
	step2: PerBlock,
) -> u64 {
	let steps = players.div_ceil(step1.min(credits).units) + players.div_ceil(step2.units);
	PHASE_CHANGE_BLOCKS + step_blocks(steps)
}

/// The phase window, in seconds rounded up to a minute, that holds `blocks`.
pub(super) fn window_secs(blocks: u64) -> u64 {
	(blocks * MILLISECS_PER_BLOCK / 1_000).div_ceil(60) * 60
}
