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
//!
//! `try-runtime on-runtime-upgrade` reports a storage version mismatch and undecodable entries
//! for the upgrade that carries a multi-block migration. Both are expected: the pallet checks run
//! after the single-block migrations and before the multi-block migrations.

use crate::{
	types::{Credibility, NotificationReference, NotificationRegistration, StmtStoreAllowanceEntry},
	weights::WeightInfo,
	Config, ConsumerAllowances, Consumers, NotificationRegistrationByAlias, Pallet,
	StatementStoreAllowances,
};
use frame_support::{
	migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
	pallet_prelude::*,
	storage_alias,
	weights::WeightMeter,
};
use indiv_support::{
	statement_allowance::BoundedStatementAllowance, traits::Alias, utils::BigEndianU32,
};
use sp_statement_store::StatementAllowance;

#[cfg(feature = "try-runtime")]
use codec::DecodeAll;

const LOG_TARGET: &str = "runtime::indiv-pallet-resources::migration";

const PALLET_MIGRATIONS_ID: &[u8; 22] = b"indiv-pallet-resources";

pub mod v1 {
	use super::*;

	/// The storage as version 0 sees it.
	pub mod v0 {
		use super::*;

		/// A statement store allowance entry without its granted allowance.
		#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
		pub struct StmtStoreAllowanceEntry<AccountId> {
			pub account_id: AccountId,
			pub seq: u32,
			pub since: u64,
		}

		/// A notification registration without its granted allowance.
		#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
		pub struct NotificationRegistration<AccountId> {
			pub account_id: AccountId,
			pub reference: NotificationReference,
		}

		#[storage_alias]
		pub type StatementStoreAllowances<T: Config> = StorageDoubleMap<
			Pallet<T>,
			Identity,
			BigEndianU32,
			Blake2_128Concat,
			Alias,
			StmtStoreAllowanceEntry<<T as frame_system::Config>::AccountId>,
		>;

		#[storage_alias]
		pub type NotificationRegistrationByAlias<T: Config> = StorageMap<
			Pallet<T>,
			Blake2_128Concat,
			Alias,
			NotificationRegistration<<T as frame_system::Config>::AccountId>,
		>;
	}

	/// The progress of [`RecordGrantedAllowances`].
	#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
	pub enum Cursor {
		/// The last migrated statement store key.
		StmtStore(Option<(BigEndianU32, Alias)>),
		/// The last migrated notification alias.
		Notification(Option<Alias>),
	}

	impl Cursor {
		/// The weight of one step at this cursor.
		fn step_weight<T: Config>(&self) -> Weight {
			match self {
				Cursor::StmtStore(_) => T::WeightInfo::migrate_stmt_store_allowance_step(),
				Cursor::Notification(_) => T::WeightInfo::migrate_notification_registration_step(),
			}
		}
	}

	/// Records the granted allowance in every statement store allowance entry and notification
	/// registration over several blocks.
	///
	/// An entry gets the current `AccountsApiAllowance` and a registration gets the current
	/// `NotificationAllowance`. This is correct only if the values have not changed since the
	/// entry was written.
	pub struct RecordGrantedAllowances<T>(PhantomData<T>);

	impl<T: Config> SteppedMigration for RecordGrantedAllowances<T> {
		type Cursor = Cursor;
		type Identifier = MigrationId<22>;

		fn id() -> Self::Identifier {
			MigrationId { pallet_id: *PALLET_MIGRATIONS_ID, version_from: 0, version_to: 1 }
		}

		fn step(
			cursor: Option<Self::Cursor>,
			meter: &mut WeightMeter,
		) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
			if Pallet::<T>::on_chain_storage_version() != 0 {
				return Ok(None);
			}
			let mut cursor = cursor.unwrap_or(Cursor::StmtStore(None));
			let required = cursor.step_weight::<T>();
			if meter.remaining().any_lt(required) {
				return Err(SteppedMigrationError::InsufficientWeight { required });
			}
			loop {
				if meter.try_consume(cursor.step_weight::<T>()).is_err() {
					return Ok(Some(cursor));
				}
				cursor = match cursor {
					Cursor::StmtStore(last) => match Self::migrate_stmt_store_entry(last) {
						Some(key) => Cursor::StmtStore(Some(key)),
						None => {
							log::info!(
								target: LOG_TARGET,
								"recorded the allowance of all statement store entries"
							);
							Cursor::Notification(None)
						},
					},
					Cursor::Notification(last) => match Self::migrate_notification(last) {
						Some(alias) => Cursor::Notification(Some(alias)),
						None => {
							log::info!(
								target: LOG_TARGET,
								"recorded the allowance of all notification registrations"
							);
							StorageVersion::new(1).put::<Pallet<T>>();
							return Ok(None);
						},
					},
				};
			}
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			if Pallet::<T>::on_chain_storage_version() != 0 {
				return Ok(None::<PreUpgradeState<T::AccountId>>.encode());
			}
			let state = PreUpgradeState::<T::AccountId> {
				entries: v0::StatementStoreAllowances::<T>::iter().collect(),
				registrations: v0::NotificationRegistrationByAlias::<T>::iter().collect(),
			};
			Ok(Some(state).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let state = Option::<PreUpgradeState<T::AccountId>>::decode_all(&mut &state[..])
				.map_err(|_| "the pre-upgrade state must decode")?;
			let Some(state) = state else {
				log::info!(target: LOG_TARGET, "the storage version was not 0; nothing to check");
				return Ok(());
			};

			ensure!(
				StatementStoreAllowances::<T>::iter_keys().count() == state.entries.len(),
				"the migration must keep every statement store allowance entry"
			);
			for (period, alias, entry) in state.entries {
				let migrated = StatementStoreAllowances::<T>::get(period, alias)
					.ok_or("a statement store allowance entry must decode with its allowance")?;
				ensure!(
					migrated ==
						StmtStoreAllowanceEntry {
							account_id: entry.account_id,
							seq: entry.seq,
							since: entry.since,
							allowance: T::AccountsApiAllowance::get().into(),
						},
					"a statement store allowance entry must keep its fields and record the \
					 accounts API allowance"
				);
			}

			ensure!(
				NotificationRegistrationByAlias::<T>::iter_keys().count() ==
					state.registrations.len(),
				"the migration must keep every notification registration"
			);
			for (alias, registration) in state.registrations {
				let migrated = NotificationRegistrationByAlias::<T>::get(alias)
					.ok_or("a notification registration must decode with its allowance")?;
				ensure!(
					migrated ==
						NotificationRegistration {
							account_id: registration.account_id,
							reference: registration.reference,
							allowance: T::NotificationAllowance::get().into(),
						},
					"a notification registration must keep its fields and record the \
					 notification allowance"
				);
			}

			ensure!(
				Pallet::<T>::on_chain_storage_version() == 1,
				"the storage version must be 1 after the migration"
			);
			Ok(())
		}
	}

	/// The version 0 state that [`RecordGrantedAllowances::pre_upgrade`] hands to
	/// [`RecordGrantedAllowances::post_upgrade`]. Absent when the migration has nothing to do.
	#[cfg(feature = "try-runtime")]
	#[derive(Encode, Decode)]
	struct PreUpgradeState<AccountId> {
		entries: alloc::vec::Vec<(BigEndianU32, Alias, v0::StmtStoreAllowanceEntry<AccountId>)>,
		registrations: alloc::vec::Vec<(Alias, v0::NotificationRegistration<AccountId>)>,
	}

	impl<T: Config> RecordGrantedAllowances<T> {
		/// Migrates the statement store entry after `last`, or returns `None` when no entry is
		/// left.
		fn migrate_stmt_store_entry(
			last: Option<(BigEndianU32, Alias)>,
		) -> Option<(BigEndianU32, Alias)> {
			let mut iter = match last {
				Some((period, alias)) => v0::StatementStoreAllowances::<T>::iter_from(
					v0::StatementStoreAllowances::<T>::hashed_key_for(period, alias),
				),
				None => v0::StatementStoreAllowances::<T>::iter(),
			};
			let (period, alias, entry) = iter.next()?;
			StatementStoreAllowances::<T>::insert(
				period,
				alias,
				StmtStoreAllowanceEntry {
					account_id: entry.account_id,
					seq: entry.seq,
					since: entry.since,
					allowance: T::AccountsApiAllowance::get().into(),
				},
			);
			Some((period, alias))
		}

		/// Migrates the notification registration after `last`, or returns `None` when no
		/// registration is left.
		fn migrate_notification(last: Option<Alias>) -> Option<Alias> {
			let mut iter = match last {
				Some(alias) => v0::NotificationRegistrationByAlias::<T>::iter_from(
					v0::NotificationRegistrationByAlias::<T>::hashed_key_for(alias),
				),
				None => v0::NotificationRegistrationByAlias::<T>::iter(),
			};
			let (alias, registration) = iter.next()?;
			NotificationRegistrationByAlias::<T>::insert(
				alias,
				NotificationRegistration {
					account_id: registration.account_id,
					reference: registration.reference,
					allowance: T::NotificationAllowance::get().into(),
				},
			);
			Some(alias)
		}
	}
}

pub mod v2 {
	use super::*;

	/// Records the personhood statement allowance of every consumer in [`ConsumerAllowances`].
	///
	/// A lite or demoted consumer gets the current `LitePersonStatementLimit` and a person gets
	/// the current `PersonStatementLimit`. This is correct only if the limits have not changed
	/// since the first consumer registered.
	pub struct MigrateConsumerAllowances<T>(PhantomData<T>);

	impl<T: Config> SteppedMigration for MigrateConsumerAllowances<T> {
		type Cursor = T::AccountId;
		type Identifier = MigrationId<22>;

		fn id() -> Self::Identifier {
			MigrationId { pallet_id: *PALLET_MIGRATIONS_ID, version_from: 1, version_to: 2 }
		}

		fn step(
			mut cursor: Option<Self::Cursor>,
			meter: &mut WeightMeter,
		) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
			if Pallet::<T>::on_chain_storage_version() != 1 {
				return Ok(None);
			}
			let required = T::WeightInfo::migrate_consumer_allowances_step();
			if meter.remaining().any_lt(required) {
				return Err(SteppedMigrationError::InsufficientWeight { required });
			}

			loop {
				if meter.try_consume(required).is_err() {
					return Ok(cursor);
				}

				let mut iter = match &cursor {
					Some(last) => Consumers::<T>::iter_from(Consumers::<T>::hashed_key_for(last)),
					None => Consumers::<T>::iter(),
				};
				let Some((account, info)) = iter.next() else {
					log::info!(target: LOG_TARGET, "recorded the allowances of all consumers");
					StorageVersion::new(2).put::<Pallet<T>>();
					return Ok(None);
				};
				let allowance = granted_allowance::<T>(&info.credibility);
				ConsumerAllowances::<T>::insert(
					&account,
					BoundedStatementAllowance::from(allowance),
				);
				cursor = Some(account);
			}
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			for (account, info) in Consumers::<T>::iter() {
				ensure!(
					ConsumerAllowances::<T>::get(&account) ==
						Some(granted_allowance::<T>(&info.credibility).into()),
					"a consumer has no recorded allowance"
				);
			}
			ensure!(
				Pallet::<T>::on_chain_storage_version() == 2,
				"the storage version must be 2 after the migration"
			);
			Ok(())
		}
	}

	/// The allowance that a consumer with `credibility` holds before the upgrade.
	fn granted_allowance<T: Config>(credibility: &Credibility) -> StatementAllowance {
		match credibility {
			Credibility::Person { demoted: false, .. } => T::PersonStatementLimit::get(),
			Credibility::Lite | Credibility::Person { demoted: true, .. } =>
				T::LitePersonStatementLimit::get(),
		}
	}
}
