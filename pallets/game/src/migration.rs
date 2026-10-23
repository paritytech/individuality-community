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

//! Storage migrations for the game pallet.

use crate::{Config, Pallet, PlayerStatementLimits, PlayerStatementLimitsRecord};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use sp_statement_store::StatementAllowance;

const LOG_TARGET: &str = "runtime::indiv-pallet-game::migration";

/// Stores `Granted` as [`PlayerStatementLimitsRecord::granted`].
///
/// `Granted` must be the allowance that live players hold before the upgrade.
pub type MigrateV0ToV1<T, Granted> = VersionedMigration<
	0,
	1,
	v1::InitialisePlayerStatementLimits<T, Granted>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// Use [`super::MigrateV0ToV1`] rather than this directly.
	pub struct InitialisePlayerStatementLimits<T, Granted>(PhantomData<(T, Granted)>);

	impl<T: Config, Granted: Get<StatementAllowance>> UncheckedOnRuntimeUpgrade
		for InitialisePlayerStatementLimits<T, Granted>
	{
		fn on_runtime_upgrade() -> Weight {
			PlayerStatementLimits::<T>::put(PlayerStatementLimitsRecord {
				granted: Granted::get().into(),
				next: None,
			});
			log::info!(target: LOG_TARGET, "initialised PlayerStatementLimits");
			T::DbWeight::get().writes(1)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			ensure!(PlayerStatementLimits::<T>::exists(), "PlayerStatementLimits is not set");
			Ok(())
		}
	}
}
