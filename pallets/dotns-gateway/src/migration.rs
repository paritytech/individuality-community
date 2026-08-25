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

//! Storage migrations for the dotNS gateway pallet.

use crate::{AccountNameRecord, AccountNames, Config, LiteLabelOwner, Pallet};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-dotns-gateway::migration";

pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::BackfillAccountNames<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// Use [`super::MigrateV0ToV1`] rather than this directly.
	///
	/// Fills [`AccountNames`] from [`LiteLabelOwner`] for lite labels registered before the
	/// map existed. Only the label is recoverable from pallet storage: the chat key stays
	/// `None` for these accounts. When an account owns several lite labels, the first one in
	/// storage iteration order wins. Full labels need no backfill: no full registration
	/// happened on any deployment before the map existed.
	pub struct BackfillAccountNames<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for BackfillAccountNames<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut reads = 1u64;
			let mut writes = 0u64;
			for (label, owner) in LiteLabelOwner::<T>::iter() {
				reads.saturating_inc();
				AccountNames::<T>::mutate(&owner, |record| {
					let record = record.get_or_insert_with(AccountNameRecord::default);
					if record.lite.is_none() {
						record.lite = Some(label);
						writes.saturating_inc();
					}
				});
			}
			log::info!(target: LOG_TARGET, "backfilled {writes} lite labels into AccountNames");
			T::DbWeight::get().reads_writes(reads.saturating_add(writes), writes)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok((LiteLabelOwner::<T>::iter().count() as u32).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u32::decode(&mut &state[..]).map_err(|_| {
				sp_runtime::TryRuntimeError::Other("pre_upgrade state is not a u32")
			})?;
			ensure!(
				LiteLabelOwner::<T>::iter().count() as u32 == before,
				"LiteLabelOwner changed during the migration"
			);
			for (_, owner) in LiteLabelOwner::<T>::iter() {
				ensure!(
					AccountNames::<T>::get(&owner).is_some_and(|record| record.lite.is_some()),
					"a lite label owner has no AccountNames record"
				);
			}
			Ok(())
		}
	}
}
