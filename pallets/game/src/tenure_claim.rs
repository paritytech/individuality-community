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

//! The tenure claim before a game, see [Tenure claim](crate#tenure-claim).

use super::*;
use frame_support::{ensure, pallet_prelude::DispatchError};
use indiv_pallet_score::Participants;

/// A tenure claim that [`Pallet::claim_tenure_credits`] accepts.
struct NewTenureClaim {
	game_index: GameIdx,
	/// The credits the player earns with this tenure claim. Never zero.
	credits: u32,
	/// Whether the player is a person, see [`indiv_pallet_score::Recognition::is_recognized`].
	/// Only a person receives the credits when they claim.
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

	/// The number of credits that [`Pallet::claim_tenure_credits`] by `player` grants now.
	///
	/// Fails with the error the tenure claim fails with. A tenure claim submitted later can still
	/// fail, because the state can change before it is included.
	pub fn tenure_credits(player: AccountOrPerson<T::AccountId>) -> Result<u32, DispatchError> {
		Self::new_tenure_claim(&player).map(|claim| claim.credits)
	}

	/// The tenure claim `player` makes now for the current game.
	///
	/// Holds every check of [`Pallet::claim_tenure_credits`] after its origin check.
	fn new_tenure_claim(
		player: &AccountOrPerson<T::AccountId>,
	) -> Result<NewTenureClaim, DispatchError> {
		let game = Game::<T>::get().ok_or(Error::<T>::NoGame)?;
		let now = T::UnixTime::now().as_secs();
		// `Reporting` starts when the shuffle ends, which is before `game_date`. A processing game
		// has started and a cancelling game is never played.
		let state_open = match game.state {
			GameState::Registration { .. } |
			GameState::Shuffle { .. } |
			GameState::Reporting { .. } => true,
			GameState::PlayerProcess { .. } | GameState::Cancelling { .. } => false,
		};
		ensure!(
			state_open &&
				u64::from(game.tenure_claim_opens) <= now &&
				now < u64::from(game.game_date),
			Error::<T>::TenureClaimClosed
		);
		ensure!(
			Players::<T>::get(player).is_some_and(|player| player.registered),
			Error::<T>::NotRegistered
		);
		ensure!(
			!TenureClaims::<T>::contains_key(game.index, player),
			Error::<T>::TenureCreditsAlreadyClaimed
		);

		let participant = Participants::<T>::get(player);
		let tenure = participant.as_ref().map_or(0, |p| p.attended_games_while_recognized);
		let is_person = participant.is_some_and(|p| p.recognition.is_recognized());

		let credits = Self::tenure_credit_count(tenure);
		ensure!(credits > 0, Error::<T>::NoTenureCredit);

		Ok(NewTenureClaim { game_index: game.index, credits, is_person })
	}

	/// Claims `player`'s tenure credits for the current game, as [`Pallet::claim_tenure_credits`]
	/// does once its origin is checked.
	///
	/// Returns the number of credits it awards now. The weight scales with this number.
	pub(crate) fn do_claim_tenure_credits(
		player: AccountOrPerson<T::AccountId>,
	) -> Result<u32, DispatchError> {
		let NewTenureClaim { game_index, credits, is_person } = Self::new_tenure_claim(&player)?;

		// The attendance backfill awards the credits that do not fit the buffers now.
		let awarded = if is_person {
			T::NftClaimCredits::award_tenure_credits(
				game_index,
				&player,
				credits,
				T::UnixTime::now().as_secs() as u32,
			)
		} else {
			0
		};

		TenureClaims::<T>::insert(game_index, &player, credits);
		Self::deposit_event(Event::<T>::TenureCreditsClaimed { player, game_index, credits });

		Ok(awarded)
	}

	/// Awards the credits of `player`'s tenure claim for `game_index` if they are a person or
	/// `attendance` is [`Attendance::Attended`], and removes the tenure claim.
	///
	/// The recognition is read now, not when the player claims, so a candidate that became a person
	/// since is paid whether or not they attend. A suspended person is not a person, so they are
	/// paid only if they attend. One party can control any number of accounts that are not
	/// persons, so attendance is what such an account pays for its credits with.
	///
	/// Returns the number of credits awarded. The caller reserves room for
	/// [`Config::MaxTenureThresholds`] credits first.
	pub(crate) fn settle_tenure_claim(
		game_index: GameIdx,
		player: &AccountOrPerson<T::AccountId>,
		attendance: Attendance,
		award_time: u32,
	) -> u32 {
		let Some(credits) = TenureClaims::<T>::take(game_index, player) else { return 0 };
		let is_person =
			Participants::<T>::get(player).is_some_and(|p| p.recognition.is_recognized());
		if !is_person && attendance == Attendance::Absent {
			return 0;
		}
		T::NftClaimCredits::award_tenure_credits(game_index, player, credits, award_time)
	}
}
