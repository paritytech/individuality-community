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
	Config, Consumers, Pallet, WeightInfo,
};
use codec::FullCodec;
use frame_support::{
	migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
	pallet_prelude::*,
	storage::StoragePrefixedMap,
	storage_alias,
	weights::WeightMeter,
};
use indiv_support::traits::CommunicationIdentifier;
use sp_io::KillStorageResult;

const LOG_TARGET: &str = "runtime::indiv-pallet-resources::migration";
const PALLET_MIGRATIONS_ID: &[u8; 22] = b"indiv-pallet-resources";

/// Storage as it was while the pallet managed usernames.
pub mod v0 {
	use super::*;

	/// The username type stored before usernames moved to the dotNS gateway pallet.
	pub type Username = BoundedVec<u8, ConstU32<32>>;

	/// A consumer record as stored while the pallet managed usernames.
	#[derive(Encode, Decode)]
	pub struct ConsumerInfo {
		pub identifier_key: CommunicationIdentifier,
		pub full_username: Option<Username>,
		pub lite_username: Username,
		pub credibility: Credibility,
	}

	/// The `Consumers` map read with the old value type.
	#[storage_alias]
	pub type Consumers<T: Config> = StorageMap<
		Pallet<T>,
		Blake2_128Concat,
		<T as frame_system::Config>::AccountId,
		ConsumerInfo,
		OptionQuery,
	>;

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

	/// Queue entries are not decoded, so the value type is opaque.
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
}

/// Position of the migration, in the order the stages run.
#[derive(Encode, Decode, MaxEncodedLen, Clone, PartialEq, Eq, Debug)]
pub enum Cursor<AccountId> {
	/// Translating `Consumers`; holds the last translated account.
	Consumers(Option<AccountId>),
	/// Clearing `UsernameOwnerOf`.
	UsernameOwnerOf,
	/// Clearing `UsernameReservationQueue`.
	UsernameReservationQueue,
	/// Clearing `ReservationOf`.
	ReservationOf,
}

/// Removes usernames from the pallet storage over as many blocks as needed.
///
/// Translates every [`Consumers`] record to the shape without username fields, then clears the
/// four username storage items that no longer exist in the pallet and bumps the storage version
/// to 1. Each step does as much work as the weight meter allows. Consumers are translated one at
/// a time. Every `ReservationOf` item also charges the two writes of the final step, which kills
/// the reservation duration and bumps the storage version.
///
/// Single use: remove from the runtime once the upgrade carrying it is live.
pub struct MigrateV0ToV1<T>(PhantomData<T>);

impl<T: Config> MigrateV0ToV1<T> {
	/// Translates the first old-shape consumer record after `last`. Returns its account, or `None`
	/// when no old-shape record remains.
	pub(crate) fn translate_next(last: Option<&T::AccountId>) -> Option<T::AccountId> {
		let mut iter = match last {
			Some(last) => v0::Consumers::<T>::iter_from(v0::Consumers::<T>::hashed_key_for(last)),
			None => v0::Consumers::<T>::iter(),
		};
		let (account, old) = iter.next()?;
		Consumers::<T>::insert(
			&account,
			ConsumerInfo { identifier_key: old.identifier_key, credibility: old.credibility },
		);
		Some(account)
	}

	/// Translates the old-shape consumer record after `last` if `meter` allows. Returns whether no
	/// old-shape record remains, or `Err(unit)` when the meter has no room for one.
	fn translate_stage(
		last: &mut Option<T::AccountId>,
		meter: &mut WeightMeter,
		unit: Weight,
	) -> Result<bool, Weight> {
		meter.try_consume(unit).map_err(|()| unit)?;
		match Self::translate_next(last.as_ref()) {
			Some(account) => {
				*last = Some(account);
				Ok(false)
			},
			None => Ok(true),
		}
	}

	/// Clears as many entries of `M` as `meter` allows at `unit` weight each, in one `clear_prefix`
	/// call. Returns whether the map is empty afterwards, or `Err(unit)` when the meter has no room
	/// for one entry. A partial clear leaves no room for another entry, so the next call is in the
	/// next step.
	fn clear_stage<V: FullCodec, M: StoragePrefixedMap<V>>(
		meter: &mut WeightMeter,
		unit: Weight,
	) -> Result<bool, Weight> {
		let limit = meter.remaining().checked_div_per_component(&unit).unwrap_or(0);
		let limit = u32::try_from(limit).unwrap_or(u32::MAX);
		if limit == 0 {
			return Err(unit);
		}
		let (removed, done) = match sp_io::storage::clear_prefix(&M::final_prefix(), Some(limit)) {
			KillStorageResult::AllRemoved(removed) => (removed, true),
			KillStorageResult::SomeRemaining(removed) => (removed, false),
		};
		// An empty map still costs the call.
		meter.consume(unit.saturating_mul(u64::from(removed.max(1))));
		Ok(done)
	}

	fn required_weight(cursor: &Cursor<T::AccountId>) -> Weight {
		match cursor {
			Cursor::Consumers(_) => T::WeightInfo::migrate_v1_translate_consumer(),
			Cursor::UsernameOwnerOf | Cursor::UsernameReservationQueue => {
				T::WeightInfo::migrate_v1_clear_username_entry()
			},
			Cursor::ReservationOf => T::WeightInfo::migrate_v1_clear_username_entry()
				.saturating_add(<T as frame_system::Config>::DbWeight::get().writes(2)),
		}
	}
}

impl<T: Config> SteppedMigration for MigrateV0ToV1<T> {
	type Cursor = Cursor<T::AccountId>;
	type Identifier = MigrationId<22>;

	fn id() -> Self::Identifier {
		MigrationId { pallet_id: *PALLET_MIGRATIONS_ID, version_from: 0, version_to: 1 }
	}

	fn step(
		cursor: Option<Self::Cursor>,
		meter: &mut WeightMeter,
	) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
		let mut cursor = match cursor {
			Some(cursor) => cursor,
			None if Pallet::<T>::on_chain_storage_version() >= StorageVersion::new(1) => {
				log::info!(target: LOG_TARGET, "storage already at version 1, nothing to migrate");
				return Ok(None);
			},
			None => Cursor::Consumers(None),
		};

		let mut progressed = false;
		loop {
			let unit = Self::required_weight(&cursor);
			let stage_done = match &mut cursor {
				Cursor::Consumers(last) => Self::translate_stage(last, meter, unit),
				Cursor::UsernameOwnerOf => {
					Self::clear_stage::<_, v0::UsernameOwnerOf<T>>(meter, unit)
				},
				Cursor::UsernameReservationQueue => {
					Self::clear_stage::<_, v0::UsernameReservationQueue<T>>(meter, unit)
				},
				Cursor::ReservationOf => Self::clear_stage::<_, v0::ReservationOf<T>>(meter, unit),
			};
			match stage_done {
				// A step that did nothing cannot progress with this meter.
				Err(required) if !progressed => {
					return Err(SteppedMigrationError::InsufficientWeight { required })
				},
				Err(_) => return Ok(Some(cursor)),
				Ok(false) => {},
				Ok(true) => {
					cursor = match cursor {
						Cursor::Consumers(_) => Cursor::UsernameOwnerOf,
						Cursor::UsernameOwnerOf => Cursor::UsernameReservationQueue,
						Cursor::UsernameReservationQueue => Cursor::ReservationOf,
						Cursor::ReservationOf => {
							v0::UsernameReservationDuration::<T>::kill();
							StorageVersion::new(1).put::<Pallet<T>>();
							log::info!(target: LOG_TARGET, "username storage removed");
							return Ok(None);
						},
					}
				},
			}
			progressed = true;
		}
	}

	#[cfg(feature = "try-runtime")]
	fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
		// Keys, not entries: an old lite record decodes under the current type by accident, as
		// `None` and `Credibility::Lite` share the discriminant, while an old person record decodes
		// to wrong values or fails. Only the key count is reliable before the migration runs.
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
			v0::UsernameOwnerOf::<T>::iter_keys().next().is_none(),
			"UsernameOwnerOf is not empty"
		);
		ensure!(
			v0::UsernameReservationQueue::<T>::iter_keys().next().is_none(),
			"UsernameReservationQueue is not empty"
		);
		ensure!(v0::ReservationOf::<T>::iter_keys().next().is_none(), "ReservationOf is not empty");
		ensure!(
			!v0::UsernameReservationDuration::<T>::exists(),
			"UsernameReservationDuration is not empty"
		);
		ensure!(
			Pallet::<T>::on_chain_storage_version() == StorageVersion::new(1),
			"storage version was not bumped"
		);
		Ok(())
	}
}
