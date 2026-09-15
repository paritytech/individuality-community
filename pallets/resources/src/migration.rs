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
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-resources::migration";
const PALLET_MIGRATIONS_ID: &[u8; 22] = b"indiv-pallet-resources";

/// The raw storage key of the last removed entry, the position to resume from. Keys of the cleared
/// maps are at most 81 bytes: two `twox128` prefixes, a `blake2_128` hash and the encoded key.
pub type RawCursor = BoundedVec<u8, ConstU32<128>>;

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
	/// Clearing `UsernameOwnerOf`; holds the last removed key.
	UsernameOwnerOf(Option<RawCursor>),
	/// Clearing `UsernameReservationQueue`; holds the last removed key.
	UsernameReservationQueue(Option<RawCursor>),
	/// Clearing `ReservationOf`; holds the last removed key.
	ReservationOf(Option<RawCursor>),
}

/// Removes usernames from the pallet storage over as many blocks as needed.
///
/// Translates every [`Consumers`] record to the shape without username fields, then clears the
/// four username storage items that no longer exist in the pallet and bumps the storage version
/// to 1. Each step does as much work as the weight meter allows, one item at a time. Every
/// `ReservationOf` item also charges the two writes of the final step, which kills the reservation
/// duration and bumps the storage version.
///
/// Map entries are removed one key at a time with `next_key` and `clear`. `clear_prefix` with a
/// limit is not usable here: it ignores the cursor and does not see removals made earlier in the
/// same block, so repeated calls remove the same entry again.
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

	/// Removes the first entry of `M` after `last`. Returns its key, or `None` when no entry
	/// remains.
	pub(crate) fn clear_next<V: FullCodec, M: StoragePrefixedMap<V>>(
		last: Option<&RawCursor>,
	) -> Option<RawCursor> {
		let prefix = M::final_prefix();
		let key = sp_io::storage::next_key(last.map_or(&prefix[..], |c| c.as_slice()))?;
		if !key.starts_with(&prefix) {
			return None;
		}
		sp_io::storage::clear(&key);
		match RawCursor::try_from(key) {
			Ok(key) => Some(key),
			Err(_) => {
				log::error!(
					target: LOG_TARGET,
					"removed key exceeds {} bytes, leaving the rest of the map in place",
					<ConstU32<128> as Get<u32>>::get()
				);
				None
			},
		}
	}

	fn required_weight(cursor: &Cursor<T::AccountId>) -> Weight {
		match cursor {
			Cursor::Consumers(_) => T::WeightInfo::migrate_v1_translate_consumer(),
			Cursor::UsernameOwnerOf(_) | Cursor::UsernameReservationQueue(_) =>
				T::WeightInfo::migrate_v1_clear_username_entry(),
			Cursor::ReservationOf(_) => T::WeightInfo::migrate_v1_clear_username_entry()
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

		let mut items = 0u32;
		loop {
			let required = Self::required_weight(&cursor);
			if meter.try_consume(required).is_err() {
				// A step that did nothing cannot progress with this meter.
				return if items == 0 {
					Err(SteppedMigrationError::InsufficientWeight { required })
				} else {
					Ok(Some(cursor))
				};
			}
			items.saturating_inc();
			cursor = match cursor {
				Cursor::Consumers(last) => match Self::translate_next(last.as_ref()) {
					Some(account) => Cursor::Consumers(Some(account)),
					None => Cursor::UsernameOwnerOf(None),
				},
				Cursor::UsernameOwnerOf(at) => {
					match Self::clear_next::<_, v0::UsernameOwnerOf<T>>(at.as_ref()) {
						Some(at) => Cursor::UsernameOwnerOf(Some(at)),
						None => Cursor::UsernameReservationQueue(None),
					}
				},
				Cursor::UsernameReservationQueue(at) => {
					match Self::clear_next::<_, v0::UsernameReservationQueue<T>>(at.as_ref()) {
						Some(at) => Cursor::UsernameReservationQueue(Some(at)),
						None => Cursor::ReservationOf(None),
					}
				},
				Cursor::ReservationOf(at) => {
					match Self::clear_next::<_, v0::ReservationOf<T>>(at.as_ref()) {
						Some(at) => Cursor::ReservationOf(Some(at)),
						None => {
							v0::UsernameReservationDuration::<T>::kill();
							StorageVersion::new(1).put::<Pallet<T>>();
							log::info!(target: LOG_TARGET, "username storage removed");
							return Ok(None);
						},
					}
				},
			};
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
