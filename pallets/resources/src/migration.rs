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

use crate::{types::Credibility, weights::WeightInfo, Config, ConsumerAllowances, Consumers};
use frame_support::{
	migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
	pallet_prelude::*,
	weights::WeightMeter,
};
use indiv_support::statement_allowance::GrantedStatementAllowance;
use sp_statement_store::StatementAllowance;

const LOG_TARGET: &str = "runtime::indiv-pallet-resources::migration";

const PALLET_MIGRATIONS_ID: &[u8; 15] = b"indiv-resources";

pub mod v1 {
	use super::*;

	/// Records the personhood statement allowance of every consumer in [`ConsumerAllowances`].
	///
	/// A lite or demoted consumer gets the current `LitePersonStatementLimit` and a person gets
	/// the current `PersonStatementLimit`. This is correct only if the limits have not changed
	/// since the first consumer registered.
	pub struct MigrateConsumerAllowances<T>(PhantomData<T>);

	impl<T: Config> SteppedMigration for MigrateConsumerAllowances<T> {
		type Cursor = T::AccountId;
		type Identifier = MigrationId<15>;

		fn id() -> Self::Identifier {
			MigrationId { pallet_id: *PALLET_MIGRATIONS_ID, version_from: 0, version_to: 1 }
		}

		fn step(
			mut cursor: Option<Self::Cursor>,
			meter: &mut WeightMeter,
		) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
			let required = T::WeightInfo::migrate_consumer_allowances_step();
			if meter.remaining().any_lt(required) {
				return Err(SteppedMigrationError::InsufficientWeight { required });
			}

			loop {
				if meter.try_consume(required).is_err() {
					break;
				}

				let mut iter = match &cursor {
					Some(last) => Consumers::<T>::iter_from(Consumers::<T>::hashed_key_for(last)),
					None => Consumers::<T>::iter(),
				};
				let Some((account, info)) = iter.next() else {
					log::info!(target: LOG_TARGET, "recorded the allowances of all consumers");
					return Ok(None);
				};
				let allowance = granted_allowance::<T>(&info.credibility);
				ConsumerAllowances::<T>::insert(
					&account,
					GrantedStatementAllowance::from(allowance),
				);
				cursor = Some(account);
			}
			Ok(cursor)
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
