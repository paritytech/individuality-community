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

//! Storage migrations for the Score pallet.

use crate::{BalanceOf, Config, Pallet, Participant, Participants};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-score::migration";

/// Adds `attended_games_while_recognized` to every stored participant, as zero.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToAttendedGamesCounter<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;
	use crate::{AttendanceHistory, Recognition, Streak};

	/// A participant as stored before the attended games counter existed.
	#[derive(Decode)]
	pub struct OldParticipant<Balance> {
		pub score: u8,
		pub streak: Streak,
		pub attendance_history: AttendanceHistory,
		pub credit: Balance,
		pub cashed_out: bool,
		pub reached_personhood: bool,
		pub has_ever_reached_personhood: bool,
		pub recognition: Recognition,
		pub last_attended_game: Option<u32>,
	}

	/// Use [`MigrateV0ToV1`] rather than this directly.
	///
	/// Running this twice would reset every counter to zero: the new encoding is the old one plus
	/// a trailing `u32`, and `Decode` reads a prefix, so a migrated participant still decodes as
	/// [`OldParticipant`] with the counter left over and ignored. The version gate is what makes
	/// that unreachable.
	pub struct MigrateToAttendedGamesCounter<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToAttendedGamesCounter<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut translated = 0u64;
			Participants::<T>::translate_values(|old: OldParticipant<BalanceOf<T>>| {
				translated.saturating_inc();
				Some(Participant {
					score: old.score,
					streak: old.streak,
					attendance_history: old.attendance_history,
					credit: old.credit,
					cashed_out: old.cashed_out,
					reached_personhood: old.reached_personhood,
					has_ever_reached_personhood: old.has_ever_reached_personhood,
					recognition: old.recognition,
					last_attended_game: old.last_attended_game,
					// Games attended before the counter existed were not tracked, so zero is the
					// only value with a known meaning.
					attended_games_while_recognized: 0,
				})
			});
			log::info!(target: LOG_TARGET, "translated {translated} participants");
			T::DbWeight::get().reads_writes(translated.saturating_add(1), translated)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// Keys, not entries: the values do not decode under the current type yet, and
			// `iter` would skip every one of them and report an empty map.
			Ok((Participants::<T>::iter_keys().count() as u32).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u32::decode(&mut &state[..]).map_err(|_| {
				sp_runtime::TryRuntimeError::Other("pre_upgrade state is not a u32")
			})?;
			// `iter` now, so an entry that failed to translate is counted as missing rather than
			// silently skipped.
			let after = Participants::<T>::iter().count() as u32;
			ensure!(before == after, "a participant did not survive the migration");
			Ok(())
		}
	}
}
