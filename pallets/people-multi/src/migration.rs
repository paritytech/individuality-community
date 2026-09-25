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

//! Storage migrations for the people pallet.

extern crate alloc;

use crate::{
	Config, MemberOf, Pallet, People, PeopleCollectionCreated, PersonRecord, WeightInfo,
	PEOPLE_MEMBER_IDENTIFIER,
};
use frame_support::{
	migrations::VersionedMigration,
	pallet_prelude::*,
	storage_alias,
	traits::{OnRuntimeUpgrade, UncheckedOnRuntimeUpgrade, UnixTime},
};
use indiv_support::traits::{AppendOnlyMembers, RecognitionHistory};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::people::migration";

/// Adds a [`RecognitionHistory`] to every [`PersonRecord`].
///
/// The real recognition time of an existing person is unknown, so a recognized person gets a
/// period open since the upgrade and tenure counts from there. A suspended person gets no open
/// period.
///
/// Runs in one block, so the number of people must fit the block's proof size.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::AddRecognitionHistory<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v0 {
	use super::*;

	/// A person record as stored before recognition history was tracked.
	#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Eq, Debug)]
	pub struct PersonRecord<Member, AccountId> {
		pub key: Member,
		pub account: Option<AccountId>,
	}

	/// The [`crate::People`] map under the old record layout.
	#[storage_alias]
	pub type People<T: Config> = StorageMap<
		Pallet<T>,
		Blake2_128Concat,
		indiv_support::traits::PersonalId,
		PersonRecord<MemberOf<T>, <T as frame_system::Config>::AccountId>,
	>;
}

pub mod v1 {
	use super::*;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	///
	/// A person whose key is an active member of the people collection is recognized. A key the
	/// collection reports as suspended, or does not know, leaves the person without an open period.
	pub struct AddRecognitionHistory<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for AddRecognitionHistory<T> {
		fn on_runtime_upgrade() -> Weight {
			let now = T::Clock::now().as_secs();
			let mut translated = 0u64;
			People::<T>::translate::<v0::PersonRecord<MemberOf<T>, T::AccountId>, _>(|id, old| {
				translated.saturating_inc();
				let status = T::MemberService::member_status(PEOPLE_MEMBER_IDENTIFIER, &old.key);
				let recognized = match status {
					Some(position) => !position.suspended(),
					None => {
						log::error!(target: LOG_TARGET, "person {id} has no member status");
						false
					},
				};
				let history = if recognized {
					RecognitionHistory::open_since(now)
				} else {
					RecognitionHistory::default()
				};
				Some(PersonRecord { key: old.key, account: old.account, history })
			});
			log::info!(target: LOG_TARGET, "added recognition history to {translated} people");
			// Reading the record and the member status, then writing the record back.
			T::DbWeight::get().reads_writes(translated.saturating_mul(2), translated)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			Ok((v0::People::<T>::iter().count() as u64).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u64::decode(&mut &state[..]).map_err(|_| "people count must decode")?;
			let mut after = 0u64;
			for (_, record) in People::<T>::iter() {
				after.saturating_inc();
				let suspended =
					T::MemberService::member_status(PEOPLE_MEMBER_IDENTIFIER, &record.key)
						.is_none_or(|position| position.suspended());
				ensure!(
					record.history.recognized_since.is_none() == suspended,
					"an open period must match an active member"
				);
				ensure!(record.history.periods.is_empty(), "no closed period is expected");
				ensure!(record.history.settled == 0, "no settled tenure is expected");
			}
			ensure!(before == after, "the number of people must not change");
			Ok(())
		}
	}
}

pub struct CreatePeopleCollection<T>(PhantomData<T>);

impl<T: Config> OnRuntimeUpgrade for CreatePeopleCollection<T> {
	fn on_runtime_upgrade() -> Weight {
		let exists_check = <T as Config>::WeightInfo::authorize_create_people_collection();
		if PeopleCollectionCreated::<T>::get() {
			log::info!(target: LOG_TARGET, "people collection already exists; skipping.");
			return exists_check;
		}

		match Pallet::<T>::do_create_people_collection() {
			Ok(()) => log::info!(target: LOG_TARGET, "people collection created."),
			Err(e) => {
				log::error!(target: LOG_TARGET, "failed to create people collection: {e:?}")
			},
		}

		exists_check.saturating_add(<T as Config>::WeightInfo::create_people_collection())
	}

	#[cfg(feature = "try-runtime")]
	fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
		ensure!(
			PeopleCollectionCreated::<T>::get(),
			"people collection must exist after migration"
		);
		Ok(())
	}
}
