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

//! The arithmetic of a [`crate::ClaimedLeaves`] bitmap: one bit per leaf of a tree, bit
//! `leaf_index % 8` of byte `leaf_index / 8`.
//!
//! The pallet relies on three facts about it. A set bit stays set, setting one leaf's bit leaves
//! every other leaf's bit unchanged, and no bit at or above `leaf_count` is ever set. The last fact
//! makes "every leaf is claimed" the same as "the count of set bits reaches `leaf_count`". The
//! test at the end of this module checks all three on every bitmap of up to 16 leaves.

use alloc::vec::Vec;

/// The byte of the bitmap that holds `leaf_index`, and the mask of its bit in that byte.
fn leaf_bit(leaf_index: u32) -> (usize, u8) {
	((leaf_index / 8) as usize, 1u8 << (leaf_index % 8))
}

/// The byte length of the bitmap of a tree with `leaf_count` leaves.
fn bitmap_len(leaf_count: u32) -> usize {
	leaf_count.div_ceil(8) as usize
}

/// Whether `leaf_index` is set in `bitmap`. A bit past the end of `bitmap` is not set.
pub(crate) fn is_set(bitmap: &[u8], leaf_index: u32) -> bool {
	let (byte, mask) = leaf_bit(leaf_index);
	bitmap.get(byte).is_some_and(|bits| bits & mask != 0)
}

/// How many bits of `bitmap` are set.
pub(crate) fn count_set(bitmap: &[u8]) -> u32 {
	bitmap.iter().map(|bits| bits.count_ones()).sum()
}

/// Sets the bit of `leaf_index` in `bitmap` and widens `bitmap` to `leaf_count` bits first.
///
/// Returns `Err` and leaves `bitmap` unchanged if `leaf_index` is not below `leaf_count`.
pub(crate) fn set(bitmap: &mut Vec<u8>, leaf_index: u32, leaf_count: u32) -> Result<(), ()> {
	if leaf_index >= leaf_count {
		return Err(());
	}
	let len = bitmap_len(leaf_count);
	if bitmap.len() < len {
		bitmap.resize(len, 0);
	}
	let (byte, mask) = leaf_bit(leaf_index);
	if let Some(bits) = bitmap.get_mut(byte) {
		*bits |= mask;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The largest tree whose every bitmap the test enumerates. Two bytes cover a byte boundary,
	/// and the properties do not depend on the size beyond that.
	const MAX_LEAVES: u32 = 16;

	/// Every bitmap that `set` can produce for a tree of `leaf_count` leaves: at most
	/// `bitmap_len(leaf_count)` bytes, and no bit at or above `leaf_count` set.
	fn reachable_bitmaps(leaf_count: u32) -> impl Iterator<Item = Vec<u8>> {
		(0..1u32 << leaf_count).flat_map(move |bits| {
			let used = (u32::BITS - bits.leading_zeros()).div_ceil(8) as usize;
			(used..=bitmap_len(leaf_count)).map(move |len| bits.to_le_bytes()[..len].to_vec())
		})
	}

	/// `set` sets one leaf's bit, leaves every other bit unchanged, counts the leaf once and
	/// never sets a bit at or above `leaf_count`.
	#[test]
	fn set_changes_one_leaf_only() {
		for leaf_count in 0..=MAX_LEAVES {
			for before in reachable_bitmaps(leaf_count) {
				for leaf_index in 0..=MAX_LEAVES {
					let mut after = before.clone();
					match set(&mut after, leaf_index, leaf_count) {
						Ok(()) => {
							assert!(leaf_index < leaf_count);
							assert!(is_set(&after, leaf_index));
							assert_eq!(after.len(), bitmap_len(leaf_count));
							for other in (0..MAX_LEAVES).filter(|&i| i != leaf_index) {
								assert_eq!(is_set(&after, other), is_set(&before, other));
							}
							let newly_set = u32::from(!is_set(&before, leaf_index));
							assert_eq!(count_set(&after), count_set(&before) + newly_set);
						},
						Err(()) => {
							assert!(leaf_index >= leaf_count);
							assert_eq!(after, before);
						},
					}
					assert!((leaf_count..MAX_LEAVES).all(|i| !is_set(&after, i)));
				}
			}
		}
	}
}
