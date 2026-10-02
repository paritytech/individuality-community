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

//! Storage migrations for the people lite pallet.

extern crate alloc;

use crate::{Config, LitePeople, LitePeopleCollectionCreated, LitePeopleCount, Pallet, WeightInfo};
use frame_support::{
	migrations::VersionedMigration,
	pallet_prelude::*,
	traits::{OnRuntimeUpgrade, UncheckedOnRuntimeUpgrade},
};
use sp_runtime::SaturatedConversion;

const LOG_TARGET: &str = "runtime::indiv-pallet-people-lite::migration";

pub struct CreateLitePeopleCollection<T>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for CreateLitePeopleCollection<T> {
	fn on_runtime_upgrade() -> Weight {
		let exists_check = <T as Config>::WeightInfo::authorize_create_lite_people_collection();
		if LitePeopleCollectionCreated::<T>::get() {
			log::info!(target: LOG_TARGET, "lite people collection already exists; skipping.");
			return exists_check;
		}

		match Pallet::<T>::do_create_lite_people_collection() {
			Ok(()) => log::info!(target: LOG_TARGET, "lite people collection created."),
			Err(e) => {
				log::error!(target: LOG_TARGET, "failed to create lite people collection: {e:?}")
			},
		}

		exists_check.saturating_add(<T as Config>::WeightInfo::create_lite_people_collection())
	}

	#[cfg(feature = "try-runtime")]
	fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
		ensure!(
			LitePeopleCollectionCreated::<T>::get(),
			"lite people collection must exist after migration"
		);
		Ok(())
	}
}

/// Sets [`LitePeopleCount`](crate::LitePeopleCount) to the number of registered lite people.
///
/// Pallet version 0 does not keep the count, so a chain upgrading from it must run this migration.
/// It iterates every `LitePeople` key in one block.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::InitializeLitePeopleCount<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	pub struct InitializeLitePeopleCount<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for InitializeLitePeopleCount<T> {
		fn on_runtime_upgrade() -> Weight {
			let count = LitePeople::<T>::iter_keys().count() as u64;
			LitePeopleCount::<T>::put(count.saturated_into::<u32>());
			log::info!(target: LOG_TARGET, "counted {count} lite people");
			T::DbWeight::get().reads_writes(count.saturating_add(1), 1)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			ensure!(
				LitePeopleCount::<T>::get() as usize == LitePeople::<T>::iter_keys().count(),
				"the lite people count must match the registered lite people"
			);
			Ok(())
		}
	}
}
