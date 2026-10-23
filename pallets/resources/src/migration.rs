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

use crate::{Config, Pallet, UnrecordedConsumerAllowances};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};

const LOG_TARGET: &str = "runtime::indiv-pallet-resources::migration";

/// Stores the current `LitePersonStatementLimit` and `PersonStatementLimit` as the allowances of
/// consumers without a `ConsumerAllowances` entry.
///
/// This is correct only if the limits have not changed since the first consumer registered.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::InitUnrecordedConsumerAllowances<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// Use [`super::MigrateV0ToV1`] rather than this directly.
	pub struct InitUnrecordedConsumerAllowances<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for InitUnrecordedConsumerAllowances<T> {
		fn on_runtime_upgrade() -> Weight {
			UnrecordedConsumerAllowances::<T>::put(Pallet::<T>::current_allowances());
			log::info!(target: LOG_TARGET, "initialised the unrecorded consumer allowances");
			T::DbWeight::get().writes(1)
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			ensure!(
				UnrecordedConsumerAllowances::<T>::exists(),
				"UnrecordedConsumerAllowances is not set"
			);
			Ok(())
		}
	}
}
