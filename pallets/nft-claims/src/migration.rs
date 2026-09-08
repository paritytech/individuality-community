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

use crate::{Config, CreditTrees, Pallet};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-claims::migration";

/// Files every stored tree under the bucket its claim deadline falls in.
///
/// Without that entry a tree stored before this upgrade is one no sweep ever reads, so it stays on
/// chain for good and the claims against it keep succeeding past their deadline.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToExpiryBuckets<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	pub struct MigrateToExpiryBuckets<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToExpiryBuckets<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut reads = 1u64;
			let mut writes = 0u64;

			for (block, tree) in CreditTrees::<T>::iter() {
				reads.saturating_inc();
				// Two writes: the bucket entry, and the cursor a tree earlier than the current one
				// lowers.
				Pallet::<T>::note_expiry(block, tree.timestamp);
				writes.saturating_accrue(2);
			}

			log::info!(target: LOG_TARGET, "filed {} trees for expiry", reads.saturating_sub(1));
			T::DbWeight::get().reads_writes(reads, writes)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			use crate::TreeExpiries;
			use indiv_support::credit_trees::expiry_bucket;

			for (block, tree) in CreditTrees::<T>::iter() {
				ensure!(
					TreeExpiries::<T>::contains_key(expiry_bucket(tree.timestamp), block),
					"a tree has no expiry entry"
				);
			}
			Ok(())
		}
	}
}
