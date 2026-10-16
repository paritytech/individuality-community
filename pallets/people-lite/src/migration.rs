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

use crate::{Config, LitePeople, LitePeopleCollectionCreated, Pallet, WeightInfo};
use frame_support::{pallet_prelude::*, traits::OnRuntimeUpgrade};
use sp_runtime::Saturating;

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

pub mod v1 {
	use super::*;
	use frame_support::{
		migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
		storage_alias,
		weights::WeightMeter,
	};

	/// The storage as version 0 sees it.
	pub mod v0 {
		use super::*;
		use crate::types::LitePersonInfoOf;

		/// The lite people map without its counter.
		#[storage_alias]
		pub type LitePeople<T: Config> = StorageMap<
			Pallet<T>,
			Blake2_128Concat,
			<T as frame_system::Config>::AccountId,
			LitePersonInfoOf<T>,
		>;
	}

	/// The counter of the [`LitePeople`] counted map.
	#[storage_alias]
	type CounterForLitePeople<T: Config> = StorageValue<Pallet<T>, u32, ValueQuery>;

	/// Counts the lite people of version 0 into the counter of [`LitePeople`] over several blocks.
	///
	/// The cursor is the last counted account. The migration sets the storage version to 1 when it
	/// completes. It does nothing if the storage version is not 0.
	pub struct LazyMigrationV0ToV1<T>(PhantomData<T>);

	impl<T: Config> SteppedMigration for LazyMigrationV0ToV1<T> {
		type Cursor = <T as frame_system::Config>::AccountId;
		type Identifier = MigrationId<24>;

		fn id() -> Self::Identifier {
			MigrationId { pallet_id: *b"indiv-pallet-people-lite", version_from: 0, version_to: 1 }
		}

		fn step(
			mut cursor: Option<Self::Cursor>,
			meter: &mut WeightMeter,
		) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
			if Pallet::<T>::on_chain_storage_version() != 0 {
				return Ok(None);
			}
			let required = <T as Config>::WeightInfo::migrate_v1_step();
			if meter.remaining().any_lt(required) {
				return Err(SteppedMigrationError::InsufficientWeight { required });
			}
			while meter.try_consume(required).is_ok() {
				let mut keys = match &cursor {
					Some(last) => v0::LitePeople::<T>::iter_keys_from(
						v0::LitePeople::<T>::hashed_key_for(last),
					),
					None => v0::LitePeople::<T>::iter_keys(),
				};
				let Some(account) = keys.next() else {
					log::info!(
						target: LOG_TARGET,
						"counted {} lite people",
						CounterForLitePeople::<T>::get()
					);
					StorageVersion::new(1).put::<Pallet<T>>();
					return Ok(None);
				};
				CounterForLitePeople::<T>::mutate(|count| count.saturating_inc());
				cursor = Some(account);
			}
			Ok(cursor)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			ensure!(
				LitePeople::<T>::count() as usize == LitePeople::<T>::iter_keys().count(),
				"the lite people count must match the registered lite people"
			);
			ensure!(
				Pallet::<T>::on_chain_storage_version() == 1,
				"the storage version must be 1 after the migration"
			);
			Ok(())
		}
	}
}
