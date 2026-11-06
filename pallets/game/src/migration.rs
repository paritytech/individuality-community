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

extern crate alloc;

use crate::{
	Config, Game, GameAirdrop, GameIdx, GameInfo, GameSchedule, GameSchedules, GameState, Pallet,
};
use alloc::vec::Vec;
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, storage_alias,
	traits::UncheckedOnRuntimeUpgrade,
};

const LOG_TARGET: &str = "runtime::game::migration";

/// The tenure claim window the migration gives every game that is already scheduled or running,
/// in seconds before the game play time.
pub const MIGRATED_TENURE_CLAIM_WINDOW: u32 = 30 * 60;

/// Adds a tenure claim window to every [`GameSchedule`] in [`GameSchedules`] and to the
/// [`GameInfo`] in [`Game`].
///
/// Each of them gets [`MIGRATED_TENURE_CLAIM_WINDOW`].
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::AddTenureClaimWindow<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v0 {
	use super::*;

	/// A game schedule as stored before tenure claims.
	#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Clone, PartialEq, Debug)]
	pub struct GameSchedule<AssetId, Balance> {
		pub game_play_time: u32,
		pub rounds: u8,
		pub max_group_size: u32,
		pub airdrops: BoundedVec<
			GameAirdrop<AssetId, Balance>,
			ConstU32<{ crate::MAX_GAME_AIRDROPS as u32 }>,
		>,
	}

	/// A game as stored before tenure claims.
	#[derive(Encode, Decode, MaxEncodedLen, TypeInfo, Debug)]
	pub struct GameInfo<AccountId: Into<sp_statement_store::AccountId>> {
		pub index: GameIdx,
		pub registration_ends: u32,
		pub shuffle_deadline: u32,
		pub game_date: u32,
		pub report_ends: u32,
		pub state: GameState<AccountId>,
		pub max_group_size: u32,
		pub rounds: u8,
		pub pending_attendance: u32,
		pub airdrops_scheduled: u8,
	}

	pub type GameScheduleOf<T> =
		GameSchedule<<T as Config>::AirdropAssetId, <T as Config>::AirdropAssetBalance>;

	/// The [`crate::GameSchedules`] queue under the old schedule layout.
	#[storage_alias]
	pub type GameSchedules<T: Config> = StorageValue<
		Pallet<T>,
		BoundedVec<GameScheduleOf<T>, <T as Config>::MaxGameSchedules>,
		ValueQuery,
	>;

	/// The [`crate::Game`] value under the old game layout.
	#[storage_alias]
	pub type Game<T: Config> =
		StorageValue<Pallet<T>, GameInfo<<T as frame_system::Config>::AccountId>>;
}

pub mod v1 {
	use super::*;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	pub struct AddTenureClaimWindow<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for AddTenureClaimWindow<T> {
		fn on_runtime_upgrade() -> Weight {
			let schedules = GameSchedules::<T>::translate::<
				BoundedVec<v0::GameScheduleOf<T>, T::MaxGameSchedules>,
				_,
			>(|old| {
				let old = old.unwrap_or_default();
				let new = old
					.into_iter()
					.map(|schedule| GameSchedule {
						game_play_time: schedule.game_play_time,
						rounds: schedule.rounds,
						max_group_size: schedule.max_group_size,
						airdrops: schedule.airdrops,
						tenure_claim_window: MIGRATED_TENURE_CLAIM_WINDOW,
					})
					.collect::<Vec<_>>();
				Some(BoundedVec::truncate_from(new))
			});
			if schedules.is_err() {
				log::error!(target: LOG_TARGET, "game schedules do not decode, so they are removed");
				GameSchedules::<T>::kill();
			}

			let game = Game::<T>::translate::<v0::GameInfo<T::AccountId>, _>(|old| {
				old.map(|game| GameInfo {
					index: game.index,
					registration_ends: game.registration_ends,
					shuffle_deadline: game.shuffle_deadline,
					game_date: game.game_date,
					report_ends: game.report_ends,
					state: game.state,
					max_group_size: game.max_group_size,
					rounds: game.rounds,
					pending_attendance: game.pending_attendance,
					airdrops_scheduled: game.airdrops_scheduled,
					tenure_claim_opens: game.game_date.saturating_sub(MIGRATED_TENURE_CLAIM_WINDOW),
				})
			});
			if game.is_err() {
				// An entry that does not decode reads as no game but still exists, so `new_game`
				// would refuse every later game.
				log::error!(target: LOG_TARGET, "the game does not decode, so it is removed");
				Game::<T>::kill();
			}

			log::info!(target: LOG_TARGET, "added the tenure claim window");
			T::DbWeight::get().reads_writes(2, 2)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			let schedules = v0::GameSchedules::<T>::get().len() as u32;
			let game = v0::Game::<T>::get().map(|game| game.index);
			Ok((schedules, game).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let (schedules, game) = <(u32, Option<GameIdx>)>::decode(&mut &state[..])
				.map_err(|_| "pre-upgrade state must decode")?;
			let new_schedules = GameSchedules::<T>::get();
			ensure!(new_schedules.len() as u32 == schedules, "the schedule count must not change");
			ensure!(
				new_schedules
					.iter()
					.all(|schedule| schedule.tenure_claim_window == MIGRATED_TENURE_CLAIM_WINDOW),
				"every schedule must have the migrated window"
			);
			ensure!(Game::<T>::get().map(|game| game.index) == game, "the game must not change");
			Ok(())
		}
	}
}
