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

use crate::{BlockNumberFor, Config, NftClaimCreditAwards, NftClaimCreditRoots, Pallet};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, storage_alias,
	traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-credits::migration";

/// Files every recorded root, and the awards it commits to, under the timestamp their deadline
/// runs from.
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

	/// The award blocks whose awards were retained, as the bound was recorded before the awards
	/// were filed for expiry.
	#[storage_alias]
	pub type NftClaimCreditAwardBlocks<T: Config> =
		StorageValue<Pallet<T>, alloc::vec::Vec<BlockNumberFor<T>>, ValueQuery>;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	///
	/// The root carries the award block's wall-clock time, which both entries are filed under, so
	/// each block ends up where one recorded today would. A block whose awards are already gone,
	/// the ring having dropped it, is filed for its root alone.
	pub struct MigrateToExpiries<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToExpiries<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut reads = 0u64;
			let mut writes = 0u64;
			let mut awards = 0u64;

			for (block, tree) in NftClaimCreditRoots::<T>::iter() {
				reads.saturating_inc();
				Pallet::<T>::note_root_expiry(block, tree.timestamp);
				writes.saturating_inc();

				reads.saturating_inc();
				if NftClaimCreditAwards::<T>::decode_len(block).unwrap_or(0) > 0 {
					Pallet::<T>::note_award_expiry(block, tree.timestamp);
					writes.saturating_inc();
					awards.saturating_inc();
				}
			}

			NftClaimCreditAwardBlocks::<T>::kill();
			writes.saturating_inc();

			log::info!(target: LOG_TARGET, "filed {reads} roots and {awards} award blocks for expiry");
			T::DbWeight::get().reads_writes(reads.saturating_add(1), writes)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			use crate::{NftClaimCreditAwardExpiries, RootExpiries};
			use indiv_support::credit_trees::ExpiryTimestamp;

			for (block, tree) in NftClaimCreditRoots::<T>::iter() {
				let timestamp = ExpiryTimestamp::from(tree.timestamp);
				ensure!(
					RootExpiries::<T>::contains_key(timestamp, block),
					"a root has no expiry entry"
				);
				ensure!(
					NftClaimCreditAwards::<T>::decode_len(block).unwrap_or(0) == 0 ||
						NftClaimCreditAwardExpiries::<T>::contains_key(timestamp, block),
					"an award block has no expiry entry"
				);
			}
			Ok(())
		}
	}
}
