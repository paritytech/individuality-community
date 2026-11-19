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

//! Storage migrations for the honour pallet.
//!
//! `try-runtime on-runtime-upgrade` reports a storage version mismatch and undecodable entries
//! for the upgrade that carries a multi-block migration. Both are expected: the pallet checks run
//! after the single-block migrations and before the multi-block migrations.

use crate::{
	types::{Direction, PointAlias, PointInfo, Seconds, SubjectAlias, SubjectId},
	weights::WeightInfo,
	Config, Pallet, Points,
};
use frame_support::{
	migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
	pallet_prelude::*,
	storage_alias,
	weights::WeightMeter,
};

#[cfg(feature = "try-runtime")]
use codec::DecodeAll;

const LOG_TARGET: &str = "runtime::indiv-pallet-honour::migration";

pub mod v1 {
	use super::*;

	/// The storage as version 0 sees it.
	pub mod v0 {
		use super::*;

		/// A point that holds a vote. Version 0 has no cleared points.
		#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
		pub struct PointInfo {
			pub subject: SubjectId,
			pub subject_alias: SubjectAlias,
			pub direction: Direction,
			pub last_used_at: Seconds,
		}

		impl From<PointInfo> for crate::types::PointInfo {
			fn from(point: PointInfo) -> Self {
				Self::Active {
					subject: point.subject,
					subject_alias: point.subject_alias,
					direction: point.direction,
					last_used_at: point.last_used_at,
				}
			}
		}

		#[storage_alias]
		pub type Points<T: Config> = StorageMap<Pallet<T>, Blake2_128Concat, PointAlias, PointInfo>;
	}

	/// Stores every point of version 0 as a [`PointInfo::Active`] point over several blocks.
	///
	/// The migration sets the storage version to 1 when it completes. It does nothing if the
	/// storage version is not 0.
	pub struct MigratePointsToActive<T>(PhantomData<T>);

	impl<T: Config> SteppedMigration for MigratePointsToActive<T> {
		type Cursor = PointAlias;
		type Identifier = MigrationId<19>;

		fn id() -> Self::Identifier {
			MigrationId { pallet_id: *b"indiv-pallet-honour", version_from: 0, version_to: 1 }
		}

		fn step(
			mut cursor: Option<Self::Cursor>,
			meter: &mut WeightMeter,
		) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
			if Pallet::<T>::on_chain_storage_version() != 0 {
				return Ok(None);
			}
			let required = T::WeightInfo::migrate_v1_step();
			if meter.remaining().any_lt(required) {
				return Err(SteppedMigrationError::InsufficientWeight { required });
			}
			while meter.try_consume(required).is_ok() {
				// The map iterates in key order, so every point after the cursor still has the
				// layout of version 0.
				let mut points = match cursor {
					Some(last) => v0::Points::<T>::iter_from(v0::Points::<T>::hashed_key_for(last)),
					None => v0::Points::<T>::iter(),
				};
				let Some((point_alias, point)) = points.next() else {
					log::info!(target: LOG_TARGET, "stored all points as active points");
					StorageVersion::new(1).put::<Pallet<T>>();
					return Ok(None);
				};
				Points::<T>::insert(point_alias, PointInfo::from(point));
				cursor = Some(point_alias);
			}
			Ok(cursor)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			if Pallet::<T>::on_chain_storage_version() != 0 {
				return Ok(None::<PreUpgradeState>.encode());
			}
			Ok(Some(v0::Points::<T>::iter().collect::<PreUpgradeState>()).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let state = Option::<PreUpgradeState>::decode_all(&mut &state[..])
				.map_err(|_| "the pre-upgrade state must decode")?;
			let Some(points) = state else {
				log::info!(target: LOG_TARGET, "the storage version was not 0; nothing to check");
				return Ok(());
			};

			ensure!(
				Points::<T>::iter_keys().count() == points.len(),
				"the migration must keep every point"
			);
			for (point_alias, point) in points {
				let Some(PointInfo::Active { subject, subject_alias, direction, last_used_at }) =
					Points::<T>::get(point_alias)
				else {
					return Err("a point must become an active point".into());
				};
				ensure!(
					subject == point.subject &&
						subject_alias == point.subject_alias &&
						direction == point.direction &&
						last_used_at == point.last_used_at,
					"an active point must keep the fields of the version 0 point"
				);
			}
			ensure!(
				Pallet::<T>::on_chain_storage_version() == 1,
				"the storage version must be 1 after the migration"
			);
			Ok(())
		}
	}

	/// The version 0 points that [`MigratePointsToActive::pre_upgrade`] hands to
	/// [`MigratePointsToActive::post_upgrade`].
	#[cfg(feature = "try-runtime")]
	type PreUpgradeState = alloc::vec::Vec<(PointAlias, v0::PointInfo)>;
}
