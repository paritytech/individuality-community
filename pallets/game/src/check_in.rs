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

//! The check-in before a game, see [Check-in](crate#check-in).

use super::*;
use frame_support::{ensure, pallet_prelude::DispatchError};
use indiv_pallet_score::{Participants, Recognition};

/// A check-in that [`Pallet::check_in`] accepts.
struct NewCheckIn {
	game_index: GameIdx,
	/// The credits the player is granted with this check-in. Never zero.
	credits: u32,
	/// Whether the player is a person, who receives the credits at check-in.
	is_person: bool,
}

impl<T: Config> Pallet<T> {
	/// The number of [`TenureThresholds`] that `tenure` reaches.
	pub fn tenure_credit_count(tenure: GamesAttended) -> u32 {
		TenureThresholds::<T>::get()
			.iter()
			.take_while(|&&threshold| threshold <= tenure)
			.count() as u32
	}

	/// The number of credits that [`Pallet::check_in`] by `player` grants now.
	///
	/// Fails with the error the check-in fails with. A check-in submitted later can still fail,
	/// because the state can change before it is included.
	pub fn check_in_credits(player: AccountOrPerson<T::AccountId>) -> Result<u32, DispatchError> {
		Self::new_check_in(&player).map(|check_in| check_in.credits)
	}

	/// The check-in `player` makes now for the current game.
	///
	/// Holds every check of [`Pallet::check_in`] after its origin check.
	fn new_check_in(player: &AccountOrPerson<T::AccountId>) -> Result<NewCheckIn, DispatchError> {
		let game = Game::<T>::get().ok_or(Error::<T>::NoGame)?;
		let now = T::UnixTime::now().as_secs();
		// A cancelling game is never played. `PlayerProcess` starts after `game_date`, so the
		// time check refuses a check-in in that state.
		let state_open = match game.state {
			GameState::Registration { .. } |
			GameState::Shuffle { .. } |
			GameState::Reporting { .. } |
			GameState::PlayerProcess { .. } => true,
			GameState::Cancelling { .. } => false,
		};
		ensure!(
			state_open && u64::from(game.check_in_opens) <= now && now < u64::from(game.game_date),
			Error::<T>::CheckInClosed
		);
		ensure!(
			Players::<T>::get(player).is_some_and(|player| player.registered),
			Error::<T>::NotRegistered
		);
		ensure!(!CheckIns::<T>::contains_key(game.index, player), Error::<T>::AlreadyCheckedIn);

		let participant = Participants::<T>::get(player);
		let tenure = participant.as_ref().map_or(0, |p| p.attended_games_while_recognized);
		let is_person = Self::is_person(participant.map(|p| p.recognition).as_ref());

		let credits = Self::tenure_credit_count(tenure);
		ensure!(credits > 0, Error::<T>::NoCheckInCredit);

		Ok(NewCheckIn { game_index: game.index, credits, is_person })
	}

	/// Whether a player with `recognition` in indiv-pallet-score is a person, who receives their
	/// check-in credits whether or not they attend.
	///
	/// A suspended person stays a person. Recognition only moves from not recognised to recognised
	/// to suspended, so a player that is a person at check-in is still one after the game.
	fn is_person(recognition: Option<&Recognition>) -> bool {
		match recognition {
			Some(
				Recognition::Recognized(_) |
				Recognition::Suspended(_) |
				Recognition::ExternallyRecognized,
			) => true,
			// One party can control any number of accounts that are not persons, so attendance is
			// what an account pays for its credits with.
			Some(Recognition::NotRecognized) | None => false,
		}
	}

	/// Checks `player` in for the current game, as [`Pallet::check_in`] does once its origin is
	/// checked.
	///
	/// Returns the number of credits it awards now. The weight scales with this number.
	pub(crate) fn do_check_in(player: AccountOrPerson<T::AccountId>) -> Result<u32, DispatchError> {
		let NewCheckIn { game_index, credits, is_person } = Self::new_check_in(&player)?;

		// The attendance backfill awards the credits that do not fit the buffers now.
		let awarded = if is_person {
			T::NftClaimCredits::award_check_in_credits(
				game_index,
				&player,
				credits,
				T::UnixTime::now().as_secs() as u32,
			)
		} else {
			0
		};

		CheckIns::<T>::insert(game_index, &player, credits);
		Self::deposit_event(Event::<T>::CheckedIn { player, game_index, credits });

		Ok(awarded)
	}

	/// Awards the credits of `player`'s check-in for `game_index` if they are a person or
	/// `attendance` is [`Attendance::Attended`], and removes the check-in.
	///
	/// The recognition is read now, not at check-in, so a candidate that became a person since
	/// is paid whether or not they attend.
	///
	/// Returns the number of credits awarded. The caller reserves room for
	/// [`Config::MaxTenureThresholds`] credits first.
	pub(crate) fn settle_check_in(
		game_index: GameIdx,
		player: &AccountOrPerson<T::AccountId>,
		attendance: Attendance,
		award_time: u32,
	) -> u32 {
		let Some(credits) = CheckIns::<T>::take(game_index, player) else { return 0 };
		let recognition = Participants::<T>::get(player).map(|p| p.recognition);
		if !Self::is_person(recognition.as_ref()) && attendance == Attendance::Absent {
			return 0;
		}
		T::NftClaimCredits::award_check_in_credits(game_index, player, credits, award_time)
	}
}
