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

//! Storage migrations for the resources pallet.

use crate::{
	types::{ConsumerInfo, Credibility},
	Config, Consumers, Pallet,
};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, storage_alias,
	traits::UncheckedOnRuntimeUpgrade,
};
use indiv_support::traits::CommunicationIdentifier;
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-resources::migration";

pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::RemoveUsernames<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// The username type stored before usernames moved to the dotNS gateway pallet.
	pub type Username = BoundedVec<u8, ConstU32<32>>;

	#[derive(Decode)]
	pub struct OldConsumerInfo {
		pub identifier_key: CommunicationIdentifier,
		pub full_username: Option<Username>,
		pub lite_username: Username,
		pub credibility: Credibility,
	}

	#[storage_alias]
	pub type UsernameOwnerOf<T: Config> = StorageMap<
		Pallet<T>,
		Blake2_128Concat,
		Username,
		<T as frame_system::Config>::AccountId,
		OptionQuery,
	>;

	#[storage_alias]
	pub type UsernameReservationDuration<T: Config> = StorageValue<Pallet<T>, u64, ValueQuery>;

	/// Queue entries are not decoded, so the value type stays opaque.
	#[storage_alias]
	pub type UsernameReservationQueue<T: Config> =
		StorageMap<Pallet<T>, Blake2_128Concat, Username, (), OptionQuery>;

	#[storage_alias]
	pub type ReservationOf<T: Config> = StorageMap<
		Pallet<T>,
		Blake2_128Concat,
		<T as frame_system::Config>::AccountId,
		Username,
		OptionQuery,
	>;

	/// Use [`super::MigrateV0ToV1`] rather than this directly.
	///
	/// Translates every [`Consumers`] record to the shape without username related fields
	/// and clears the username storage items that no longer exist in the pallet.
	pub struct RemoveUsernames<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for RemoveUsernames<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut translated = 0u64;
			Consumers::<T>::translate_values(|old: OldConsumerInfo| {
				translated.saturating_inc();
				Some(ConsumerInfo {
					identifier_key: old.identifier_key,
					credibility: old.credibility,
				})
			});

			let mut removed = 0u64;
			removed.saturating_accrue(UsernameOwnerOf::<T>::clear(u32::MAX, None).unique.into());
			removed.saturating_accrue(
				UsernameReservationQueue::<T>::clear(u32::MAX, None).unique.into(),
			);
			removed.saturating_accrue(ReservationOf::<T>::clear(u32::MAX, None).unique.into());
			UsernameReservationDuration::<T>::kill();
			removed.saturating_inc();

			log::info!(
				target: LOG_TARGET,
				"translated {translated} consumer records, removed {removed} username entries"
			);
			T::DbWeight::get().reads_writes(
				translated.saturating_add(removed),
				translated.saturating_add(removed),
			)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// Keys, not entries: an old record does not decode under the current type, so `iter`
			// would skip it and report it as missing.
			Ok((Consumers::<T>::iter_keys().count() as u32).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let consumers = u32::decode(&mut &state[..])
				.map_err(|_| sp_runtime::TryRuntimeError::Other("pre_upgrade state is not u32"))?;
			ensure!(
				Consumers::<T>::iter().count() as u32 == consumers,
				"a consumer record did not survive the migration"
			);
			ensure!(
				UsernameOwnerOf::<T>::iter_keys().next().is_none(),
				"UsernameOwnerOf is not empty"
			);
			ensure!(
				UsernameReservationQueue::<T>::iter_keys().next().is_none(),
				"UsernameReservationQueue is not empty"
			);
			ensure!(ReservationOf::<T>::iter_keys().next().is_none(), "ReservationOf is not empty");
			ensure!(
				!UsernameReservationDuration::<T>::exists(),
				"UsernameReservationDuration is not empty"
			);
			Ok(())
		}
	}
}
