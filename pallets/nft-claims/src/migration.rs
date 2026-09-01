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

//! Storage migrations for the NFT claims pallet.

use crate::{ClaimedLeafBytes, ClaimedLeaves, Config, CreditTrees, Pallet};
use alloc::{vec, vec::Vec};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, storage_alias,
	traits::UncheckedOnRuntimeUpgrade,
};
use indiv_support::credit_trees::{AwardBlock, NftClaimCreditLeaf};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-claims::migration";

/// Files every stored tree under its expiry bucket and replaces the per-leaf claim records with
/// the [`ClaimedLeaves`] bitmap.
///
/// Without the expiry entry a tree stored before this upgrade is one no sweep ever reads, so it
/// stays on chain for good, and the claims against it keep succeeding past their deadline.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToClaimedLeaves<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// The claimed leaves of an award block, as recorded before the bitmap. The leaf hash keys the
	/// entry, so it does not name the leaf's index.
	#[storage_alias]
	pub type ClaimedCredits<T: Config> = StorageDoubleMap<
		Pallet<T>,
		Twox64Concat,
		AwardBlock,
		Identity,
		NftClaimCreditLeaf,
		(),
		OptionQuery,
	>;

	/// How many of an award block's leaves had been claimed, as counted before the bitmap.
	#[storage_alias]
	pub type ClaimedCounts<T: Config> =
		StorageMap<Pallet<T>, Twox64Concat, AwardBlock, u32, ValueQuery>;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	///
	/// A tree that had claims against it is marked fully claimed, because the old records key a
	/// leaf by its hash and the bitmap keys it by its index. The tree holds the root only, so no
	/// leaf can be mapped to an index here, and any other filling would either let a spent credit
	/// mint a second NFT or block a credit that was never spent. The credits left unclaimed on
	/// such a tree are unmintable from this upgrade on, and each one is logged.
	pub struct MigrateToClaimedLeaves<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToClaimedLeaves<T> {
		fn on_runtime_upgrade() -> Weight {
			let max_leaves = T::MaxCreditsPerAwardBlock::get();
			let mut reads = 1u64;
			let mut writes = 0u64;
			let mut indexed = 0u64;
			let mut settled = 0u64;
			let mut removed = Vec::new();

			for (block, tree) in CreditTrees::<T>::iter() {
				reads.saturating_inc();

				if tree.leaf_count > max_leaves {
					// `ClaimedLeaves` holds one bit per leaf up to this bound, which the tree
					// predates, so no bitmap here covers its leaves. A delivery of such a tree is
					// refused from now on, so a replay cannot bring it back.
					log::error!(
						target: LOG_TARGET,
						"Removing the oversized tree of block {}: {} leaves against a bound of \
						 {max_leaves}",
						block,
						tree.leaf_count,
					);
					removed.push(block);
					continue;
				}

				let claimed = ClaimedCounts::<T>::get(block);
				reads.saturating_inc();
				if claimed > 0 {
					log::warn!(
						target: LOG_TARGET,
						"Settling the tree of block {} as fully claimed: {claimed} of {} credits \
						 were claimed under leaf hashes, so the {} left cannot be claimed any more",
						block,
						tree.leaf_count,
						tree.leaf_count.saturating_sub(claimed),
					);
					ClaimedLeaves::<T>::insert(block, all_leaves_claimed::<T>(tree.leaf_count));
					removed.push(block);
					writes.saturating_inc();
					settled.saturating_inc();
				}

				// Every tree the sweep has to reach, and every bitmap it has to remove, is filed
				// here. A tree removed above keeps its entry, as both removal paths do.
				Pallet::<T>::note_tree_expiry(block, tree.timestamp);
				writes.saturating_accrue(2);
				indexed.saturating_inc();
			}

			for block in &removed {
				CreditTrees::<T>::remove(block);
				writes.saturating_inc();
			}
			// One call queues one message's worth, which is what a full queue reports as dropped.
			let per_message = T::MaxTreeDeletionsPerMessage::get().max(1) as usize;
			for chunk in removed.chunks(per_message) {
				Pallet::<T>::queue_tree_deletions(chunk);
				writes.saturating_inc();
			}

			let credits = ClaimedCredits::<T>::clear(u32::MAX, None);
			let counts = ClaimedCounts::<T>::clear(u32::MAX, None);
			reads.saturating_accrue(u64::from(credits.loops).saturating_add(counts.loops.into()));
			writes
				.saturating_accrue(u64::from(credits.unique).saturating_add(counts.unique.into()));

			log::info!(
				target: LOG_TARGET,
				"Filed {indexed} trees under their expiry bucket, settled {settled} of them as \
				 fully claimed, removed {} oversized ones, and cleared {} leaf records",
				(removed.len() as u64).saturating_sub(settled),
				credits.unique,
			);
			T::DbWeight::get().reads_writes(reads, writes)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			use crate::{NextExpiryBucket, TreeExpiries};
			use indiv_support::credit_trees::expiry_bucket;

			ensure!(
				ClaimedCredits::<T>::iter().next().is_none(),
				"a leaf record survived the migration"
			);
			ensure!(
				ClaimedCounts::<T>::iter().next().is_none(),
				"a claimed count survived the migration"
			);

			let next = NextExpiryBucket::<T>::get();
			for (block, tree) in CreditTrees::<T>::iter() {
				ensure!(
					tree.leaf_count <= T::MaxCreditsPerAwardBlock::get(),
					"an oversized tree survived the migration"
				);
				let bucket = expiry_bucket(tree.timestamp);
				ensure!(TreeExpiries::<T>::contains_key(bucket, block), "a tree has no bucket");
				ensure!(next.is_some_and(|next| next <= bucket), "a tree is behind the sweep");
			}
			Ok(())
		}
	}

	/// A bitmap with the bit of every leaf below `leaf_count` set.
	///
	/// The bits above `leaf_count` in the last byte stay clear, so the set bits count the leaves.
	fn all_leaves_claimed<T: Config>(leaf_count: u32) -> BoundedVec<u8, ClaimedLeafBytes<T>> {
		let mut bits = vec![u8::MAX; leaf_count.div_ceil(8) as usize];
		let spare = leaf_count % 8;
		if spare > 0 {
			if let Some(last) = bits.last_mut() {
				*last = (1u8 << spare).saturating_sub(1);
			}
		}
		BoundedVec::truncate_from(bits)
	}
}
