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

use super::{
	Attendance, Config, Error, Event, Game, GameIdx, GameState, GamesAttended, Pallet, Players,
	TenureClaims, TenureThresholds,
};
use frame_support::{ensure, pallet_prelude::DispatchError, traits::UnixTime};
use indiv_pallet_score::{AccountOrPerson, Participants};
use indiv_support::credit_trees::AwardCredits;

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

	/// The number of credits that [`Pallet::claim_tenure_credits`] by `player` earns.
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
		// A game whose window is zero opens and closes at play, so it takes no tenure claims.
		ensure!(
			state_open &&
				now < u64::from(game.game_date) &&
				game.tenure_claim_opens < game.game_date,
			Error::<T>::TenureClaimClosed
		);
		ensure!(u64::from(game.tenure_claim_opens) <= now, Error::<T>::TenureClaimNotOpen);
		ensure!(
			Players::<T>::get(player).is_some_and(|player| player.registered),
			Error::<T>::NotRegistered
		);
		ensure!(!TenureClaims::<T>::contains_key(player), Error::<T>::TenureCreditsAlreadyClaimed);

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

		TenureClaims::<T>::insert(&player, credits);
		Self::deposit_event(Event::<T>::TenureCreditsClaimed { player, game_index, credits });

		Ok(awarded)
	}

	/// Awards the credits of `player`'s tenure claim for `game_index` and removes the claim.
	///
	/// Pays a person, or a player that attended, see [Payout](crate#payout). Returns the number of
	/// credits awarded. The caller reserves room for [`Config::MaxTenureThresholds`] credits first.
	pub(crate) fn settle_tenure_claim(
		game_index: GameIdx,
		player: &AccountOrPerson<T::AccountId>,
		attendance: Attendance,
		award_time: u32,
	) -> u32 {
		let Some(credits) = TenureClaims::<T>::take(player) else { return 0 };
		let is_person =
			Participants::<T>::get(player).is_some_and(|p| p.recognition.is_recognized());
		if !is_person && attendance == Attendance::Absent {
			return 0;
		}
		T::NftClaimCredits::award_tenure_credits(game_index, player, credits, award_time)
	}
}
