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

//! The mapping from a credit to its slot in [`crate::AwardedCredits`].
//!
//! Report credits take the slots `round * max_group_size + attester_position`, and tenure credits
//! take the slots after the last report slot. Two credits that share a slot are awarded once
//! between them, so the mapping must be injective for every configuration the pallet's
//! `integrity_test` accepts. The tests at the end of this module check that for each of them.

/// The number of slots an [`crate::AwardedCredits`] set holds, one per bit of its `u128`.
pub(crate) const SLOT_CAPACITY: u32 = u128::BITS;

/// The number of report credit slots a game with these bounds can use.
pub(crate) fn max_report_slots(max_rounds: u32, max_group_size: u32) -> u32 {
	max_rounds.saturating_mul(max_group_size)
}

/// The slot of the report credit of `attester_position` in `round`.
pub(crate) fn report_slot(round: u32, attester_position: u32, max_group_size: u32) -> u32 {
	round.saturating_mul(max_group_size).saturating_add(attester_position)
}

/// The slot of the tenure credit for the tenure threshold at `index`.
pub(crate) fn tenure_slot(index: u32, max_rounds: u32, max_group_size: u32) -> u32 {
	max_report_slots(max_rounds, max_group_size).saturating_add(index)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Records each slot of a configuration's credits and fails on a slot outside the set or a
	/// slot that an earlier credit took.
	fn assert_slots_distinct(max_rounds: u32, max_group_size: u32, max_tenure_thresholds: u32) {
		let report_slots = (0..max_rounds).flat_map(|round| {
			(0..max_group_size).map(move |position| report_slot(round, position, max_group_size))
		});
		let tenure_slots =
			(0..max_tenure_thresholds).map(|index| tenure_slot(index, max_rounds, max_group_size));
		let mut taken = 0u128;
		for slot in report_slots.chain(tenure_slots) {
			assert!(slot < SLOT_CAPACITY, "slot {slot} is outside the set");
			assert_eq!(taken & (1 << slot), 0, "slot {slot} is taken twice");
			taken |= 1 << slot;
		}
	}

	/// Covers every configuration that the pallet's `integrity_test` accepts, in which a game has
	/// at least one report credit slot.
	#[test]
	fn distinct_credits_have_distinct_slots() {
		for max_rounds in 1..=SLOT_CAPACITY {
			for max_group_size in 1..=SLOT_CAPACITY / max_rounds {
				let report_slots = max_report_slots(max_rounds, max_group_size);
				for max_tenure_thresholds in 0..=SLOT_CAPACITY - report_slots {
					assert_slots_distinct(max_rounds, max_group_size, max_tenure_thresholds);
				}
			}
		}
	}

	/// With zero rounds or a group size of zero, a game has no report credits, so each tenure
	/// credit takes the slot of its index. The other bound can take any value.
	#[test]
	fn tenure_slots_start_at_zero_without_report_slots() {
		for (max_rounds, max_group_size) in [(0, 0), (0, u32::MAX), (u32::MAX, 0)] {
			assert_eq!(max_report_slots(max_rounds, max_group_size), 0);
			for index in 0..SLOT_CAPACITY {
				assert_eq!(tenure_slot(index, max_rounds, max_group_size), index);
			}
		}
	}
}
