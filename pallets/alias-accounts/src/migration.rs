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

//! Storage migrations for the alias accounts pallet.

use crate::{AccountToAlias, Config, Pallet};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use indiv_support::traits::{ContextualAlias, Identifier, Incarnation, RevisionIndex, RingIndex};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-alias-accounts::migration";

/// Adds the collection incarnation to every stored mapping.
///
/// Without the upgrade, a stored mapping does not decode. It reads as absent but its key still
/// blocks the account.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToIncarnation<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// A mapping as stored before the incarnation.
	#[derive(Encode, Decode)]
	pub struct AliasAccountInfoV0 {
		pub collection: Identifier,
		pub revision: RevisionIndex,
		pub ring: RingIndex,
		pub ca: ContextualAlias,
	}

	/// Use [`MigrateV0ToV1`] rather than this directly.
	///
	/// Every mapping takes incarnation zero, which the member service reports until the first
	/// deletion. A mapping from a collection re-created before this upgrade survives, because no
	/// chain recorded the re-creation. All mappings translate in one block, so their count must
	/// fit its weight.
	pub struct MigrateToIncarnation<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToIncarnation<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut migrated = 0u64;

			AccountToAlias::<T>::translate_values(|old: AliasAccountInfoV0| {
				migrated.saturating_inc();
				Some(crate::types::AliasAccountInfo {
					collection: old.collection,
					incarnation: Incarnation::default(),
					revision: old.revision,
					ring: old.ring,
					ca: old.ca,
				})
			});

			log::info!(target: LOG_TARGET, "Added the incarnation to {migrated} alias mappings");
			T::DbWeight::get().reads_writes(migrated, migrated)
		}

		/// Counts the stored keys. `iter_keys` does not decode the values, so it counts
		/// untranslated mappings.
		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok((AccountToAlias::<T>::iter_keys().count() as u64).encode())
		}

		/// `translate_values` drops an entry that does not decode. Only the count shows it.
		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u64::decode(&mut &state[..])
				.map_err(|_| sp_runtime::TryRuntimeError::Other("pre_upgrade state is corrupt"))?;
			let after = AccountToAlias::<T>::iter_values().count() as u64;
			ensure!(before == after, "the upgrade dropped a mapping it could not decode");
			Ok(())
		}
	}
}
