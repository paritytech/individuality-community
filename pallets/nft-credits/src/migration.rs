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

use crate::{Config, NftClaimCreditRoots, Pallet};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-credits::migration";

/// Files every recorded root under the bucket its deadline falls in.
///
/// A root recorded before this upgrade has no [`RootExpiries`](crate::RootExpiries) entry, so no
/// sweep reads it and it stays on chain for good, together with the awards the claims chain builds
/// proofs from.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToRootExpiries<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// Use [`MigrateV0ToV1`](super::MigrateV0ToV1) rather than this directly.
	///
	/// The root carries the award block's wall-clock time, which is what the bucket comes from, so
	/// each root is filed under the same bucket a root recorded today would be.
	pub struct MigrateToRootExpiries<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToRootExpiries<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut reads = 0u64;
			let mut writes = 0u64;

			for (block, tree) in NftClaimCreditRoots::<T>::iter() {
				reads.saturating_inc();
				// This lowers the watermark to the earliest bucket held, so the sweep starts at
				// the oldest root rather than behind it.
				Pallet::<T>::note_root_expiry(block, tree.timestamp);
				writes.saturating_accrue(2);
			}

			log::info!(target: LOG_TARGET, "filed {reads} roots under their expiry bucket");
			T::DbWeight::get().reads_writes(reads.saturating_add(1), writes)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			use crate::{NextRootExpiryBucket, RootExpiries};
			use indiv_support::credit_trees::expiry_bucket;

			let next = NextRootExpiryBucket::<T>::get();
			for (block, tree) in NftClaimCreditRoots::<T>::iter() {
				let bucket = expiry_bucket(tree.timestamp);
				ensure!(RootExpiries::<T>::contains_key(bucket, block), "a root has no bucket");
				ensure!(next.is_some_and(|next| next <= bucket), "a root is behind the sweep");
			}
			Ok(())
		}
	}
}
