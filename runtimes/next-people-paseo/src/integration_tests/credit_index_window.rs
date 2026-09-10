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

//! The per-claimant award-block index against the window a credit stays mintable in.
//!
//! `MaxCreditBlocksPerClaimant` counts award blocks while `AwardRetentionTtl` counts seconds, so
//! the two only agree at an assumed game cadence. This pins that assumption: change the schedule,
//! the retention window or the group and round bounds, and it fails rather than the index quietly
//! evicting blocks whose credits are still mintable.

use super::*;
use frame_support::traits::Get;

/// The intended schedule: one game a week. Governance sets it, `new_game` only refusing a second
/// concurrent game, so nothing in the runtime pins this and it is stated here instead.
const GAME_PERIOD_SECONDS: u64 = 7 * 24 * 60 * 60;

#[test]
fn credit_blocks_index_spans_the_award_retention_window() {
	// A co-player awards one credit per round they reported `Person` in, and each of those reports
	// can land in a block of its own. The attendance backfill awards whatever is left in a single
	// call, so one more block. This worst case covers every lighter pattern, so a player who plays
	// only as often as keeping their personhood needs sits far inside it.
	let group_size = <<Runtime as indiv_pallet_game::Config>::MaxGroupSize as Get<u32>>::get();
	let rounds = <<Runtime as indiv_pallet_game::Config>::MaxRounds as Get<u32>>::get();
	let per_game = group_size.saturating_sub(1).saturating_mul(rounds).saturating_add(1);

	let ttl = <<Runtime as indiv_pallet_nft_credits::Config>::AwardRetentionTtl as Get<u64>>::get();
	let games = ttl.div_ceil(GAME_PERIOD_SECONDS) as u32;
	let needed = games.saturating_mul(per_game);
	let bound = <<Runtime as indiv_pallet_nft_credits::Config>::MaxCreditBlocksPerClaimant as Get<
		u32,
	>>::get();

	assert!(
		bound >= needed,
		"`MaxCreditBlocksPerClaimant` ({bound}) is below the {needed} award blocks a claimant \
		 reaches over `AwardRetentionTtl`: {games} games of up to {per_game} blocks each, so the \
		 index would evict a block whose credit is still mintable",
	);
}
