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

use crate::{
	AccountOrPerson, AttendanceHistory, BalanceOf, Config, Pallet, Participant, Participants,
	Recognition, Streak,
};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, storage_alias,
	traits::UncheckedOnRuntimeUpgrade,
};
use indiv_support::traits::PersonalId;
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

/// Adds `newly_recognized` to every stored participant that is `Recognized`, as `false`.
///
/// Use it on a testnet only. It translates every participant in one block and its weight does not
/// include the proof size.
pub type MigrateV1ToV2TestnetOnly<T> = VersionedMigration<
	1,
	2,
	v2::MigrateToNewlyRecognizedTestnetOnly<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// The recognition status as stored at versions 0 and 1.
	#[derive(Encode, Decode, Eq, PartialEq, Debug)]
	pub enum Recognition {
		ExternallyRecognized,
		NotRecognized,
		Suspended(PersonalId),
		Recognized(PersonalId),
	}

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

	/// A participant as stored at version 1.
	#[derive(Encode, Decode, Eq, PartialEq, Debug)]
	pub struct Participant<Balance> {
		pub score: u8,
		pub streak: Streak,
		pub attendance_history: AttendanceHistory,
		pub credit: Balance,
		pub cashed_out: bool,
		pub reached_personhood: bool,
		pub has_ever_reached_personhood: bool,
		pub recognition: Recognition,
		pub last_attended_game: Option<u32>,
		pub attended_games_while_recognized: u32,
	}

	/// The [`crate::Participants`] map under the version 1 participant layout.
	#[storage_alias]
	pub type Participants<T: Config> = StorageMap<
		Pallet<T>,
		Blake2_128Concat,
		AccountOrPerson<<T as frame_system::Config>::AccountId>,
		Participant<BalanceOf<T>>,
	>;

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
			// Keys, not entries: the values do not decode under the version 1 type yet, and
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

pub mod v2 {
	use super::*;

	/// Use [`MigrateV1ToV2TestnetOnly`] rather than this directly.
	///
	/// A second run decodes a migrated `Recognized` participant with the version 1 layout. It then
	/// reads `newly_recognized` as part of `last_attended_game` and corrupts the entry. The
	/// version gate prevents a second run.
	pub struct MigrateToNewlyRecognizedTestnetOnly<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToNewlyRecognizedTestnetOnly<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut translated = 0u64;
			Participants::<T>::translate_values(|old: v1::Participant<BalanceOf<T>>| {
				translated.saturating_inc();
				Some(Participant {
					score: old.score,
					streak: old.streak,
					attendance_history: old.attendance_history,
					credit: old.credit,
					cashed_out: old.cashed_out,
					reached_personhood: old.reached_personhood,
					has_ever_reached_personhood: old.has_ever_reached_personhood,
					recognition: match old.recognition {
						v1::Recognition::ExternallyRecognized => Recognition::ExternallyRecognized,
						v1::Recognition::NotRecognized => Recognition::NotRecognized,
						v1::Recognition::Suspended(id) => Recognition::Suspended(id),
						// The game accepted only the alias variant from a recognized player
						// before the flag existed. `false` keeps that rule.
						v1::Recognition::Recognized(id) =>
							Recognition::Recognized { id, newly_recognized: false },
					},
					last_attended_game: old.last_attended_game,
					attended_games_while_recognized: old.attended_games_while_recognized,
				})
			});
			log::info!(target: LOG_TARGET, "translated {translated} participants");
			// `DbWeight` gives the `ref_time` of the reads and writes only. The weight has no
			// `proof_size` for the participants that the translation pulls into the PoV. A testnet
			// accepts that, hence the `TestnetOnly` suffix.
			T::DbWeight::get().reads_writes(translated.saturating_add(1), translated)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// The version 1 map, because a `Recognized` participant does not decode under the
			// current type yet.
			let recognized = v1::Participants::<T>::iter_values()
				.filter(|p| matches!(p.recognition, v1::Recognition::Recognized(_)))
				.count() as u32;
			let total = Participants::<T>::iter_keys().count() as u32;
			Ok((total, recognized).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let (total_before, recognized_before) =
				<(u32, u32)>::decode(&mut &state[..]).map_err(|_| {
					sp_runtime::TryRuntimeError::Other("pre_upgrade state is not a (u32, u32)")
				})?;
			let mut total = 0u32;
			let mut recognized = 0u32;
			for participant in Participants::<T>::iter_values() {
				total.saturating_inc();
				match participant.recognition {
					Recognition::Recognized { newly_recognized: false, .. } =>
						recognized.saturating_inc(),
					Recognition::Recognized { newly_recognized: true, .. } =>
						return Err("the migration marked a participant as newly recognized".into()),
					_ => {},
				}
			}
			ensure!(total_before == total, "a participant did not survive the migration");
			ensure!(
				recognized_before == recognized,
				"a recognized participant did not survive the migration"
			);
			Ok(())
		}
	}
}
