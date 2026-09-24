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

//! Storage migrations for the NFT credits pallet.

use crate::{BlockNumberFor, Config, NftClaimCreditRoots, Pallet};
use frame_support::{
	migrations::VersionedMigration,
	pallet_prelude::*,
	storage_alias,
	traits::{UncheckedOnRuntimeUpgrade, UnixTime},
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-credits::migration";

/// Files every recorded root, and every block whose awards are retained, under the timestamp their
/// deadline runs from.
///
/// A root recorded before this upgrade has no [`RootExpiries`](crate::RootExpiries) entry and its
/// awards no [`NftClaimCreditAwardExpiries`](crate::NftClaimCreditAwardExpiries) one, so no sweep
/// reads either and both stay on chain for good. The ring that used to bound the awards is dropped
/// here, its blocks being filed for expiry instead.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToExpiries<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// The tree blocks whose awards are retained, which is what bounded them before they were
	/// filed for expiry.
	///
	/// This is the only item left that names those blocks. The awards map held one entry per block
	/// before this upgrade and holds one per chunk of a block after it, so an entry written under
	/// the old layout answers no read of the new one.
	#[storage_alias]
	pub type NftClaimCreditAwardBlocks<T: Config> =
		StorageValue<Pallet<T>, alloc::vec::Vec<BlockNumberFor<T>>, ValueQuery>;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	///
	/// The root carries the wall-clock time of the block's first award, which both entries are
	/// filed under, so each block ends up where one recorded today would. A retained block whose
	/// root is already deleted has no such time and is filed under the upgrade's own, which gives
	/// it one full [`Config::AwardRetentionTtl`] from here.
	///
	/// The awards themselves are left where they are. A sweep clears a block by the prefix its
	/// chunks share, and the entry the old layout wrote is that prefix, so filing the block is
	/// what removes it.
	pub struct MigrateToExpiries<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToExpiries<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut roots = 0u64;
			let mut writes = 0u64;

			for (block, tree) in NftClaimCreditRoots::<T>::iter() {
				roots.saturating_inc();
				Pallet::<T>::note_root_expiry(block, tree.timestamp);
				writes.saturating_inc();
			}

			let retained = NftClaimCreditAwardBlocks::<T>::take();
			writes.saturating_inc();

			let now = T::UnixTime::now().as_secs() as u32;
			for block in &retained {
				let timestamp =
					NftClaimCreditRoots::<T>::get(block).map_or(now, |tree| tree.timestamp);
				Pallet::<T>::note_award_expiry(*block, timestamp);
				writes.saturating_inc();
			}

			let awards = retained.len() as u64;
			log::info!(target: LOG_TARGET, "filed {roots} roots and {awards} tree blocks for expiry");
			T::DbWeight::get().reads_writes(roots.saturating_add(awards).saturating_add(1), writes)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok(NftClaimCreditAwardBlocks::<T>::get().encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			use crate::{NftClaimCreditAwardExpiries, RootExpiries};
			use indiv_support::credit_trees::ExpiryTimestamp;

			for (block, tree) in NftClaimCreditRoots::<T>::iter() {
				ensure!(
					RootExpiries::<T>::contains_key(ExpiryTimestamp::from(tree.timestamp), block),
					"a root has no expiry entry"
				);
			}

			let retained = alloc::vec::Vec::<BlockNumberFor<T>>::decode(&mut &state[..])
				.map_err(|_| "retained tree blocks must decode")?;
			let filed = NftClaimCreditAwardExpiries::<T>::iter_keys()
				.map(|(_, block)| block)
				.collect::<alloc::collections::BTreeSet<_>>();
			for block in &retained {
				ensure!(filed.contains(block), "a retained tree block has no expiry entry");
			}

			ensure!(
				!NftClaimCreditAwardBlocks::<T>::exists(),
				"the retained tree block ring must be gone"
			);
			Ok(())
		}
	}
}
