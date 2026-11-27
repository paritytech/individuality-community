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

//! The player statement limit manager handles the statement allowances of live players.
//!
//! See [`PlayerStatementLimitManager`] for details.

use super::{AliasToStmtAccount, Config, PlayerStatementLimits, PlayerStatementLimitsRecord};
use frame_support::traits::{Defensive, Get};
use indiv_pallet_score::AccountOrPerson;
use indiv_support::statement_allowance::BoundedStatementAllowance;
use sp_statement_store::{decrease_allowance_by, increase_allowance_by};

/// Reads and writes [`PlayerStatementLimits`] and the statement allowances it records.
///
/// Every live player holds [`PlayerStatementLimitsRecord::granted`], so a grant and its revocation
/// must use the same amount. The manager keeps the record and the allowances consistent. Do not
/// read or write [`PlayerStatementLimits`] outside it, except in migrations.
pub struct PlayerStatementLimitManager<T>(core::marker::PhantomData<T>);

impl<T: Config> PlayerStatementLimitManager<T> {
	/// Return the dynamic `PlayerStatementLimit`, or zero when one of its dimensions is zero.
	///
	/// Live players hold [`Self::limits`] instead until player processing moves them to it.
	fn dynamic_player_statement_limit() -> BoundedStatementAllowance {
		let limit = T::PlayerStatementLimit::get();
		if limit.is_depleted() {
			BoundedStatementAllowance::default()
		} else {
			limit.into()
		}
	}

	/// Return [`PlayerStatementLimits`], or [`Self::dynamic_player_statement_limit`] when unset.
	pub(crate) fn limits() -> PlayerStatementLimitsRecord {
		PlayerStatementLimits::<T>::get().unwrap_or_else(|| PlayerStatementLimitsRecord {
			granted: Self::dynamic_player_statement_limit(),
			next: None,
		})
	}

	/// Grant [`PlayerStatementLimitsRecord::granted`] to `account`.
	///
	/// Store [`PlayerStatementLimits`] when unset, so revocations use the same amount.
	pub(crate) fn grant(account: &T::AccountId) {
		let granted = PlayerStatementLimits::<T>::get()
			.unwrap_or_else(|| {
				let limits = PlayerStatementLimitsRecord {
					granted: Self::dynamic_player_statement_limit(),
					next: None,
				};
				PlayerStatementLimits::<T>::put(&limits);
				limits
			})
			.granted;
		increase_allowance_by(account.clone().into(), granted.into());
	}

	/// Revoke [`PlayerStatementLimitsRecord::granted`] from `account`.
	pub(crate) fn revoke(account: &T::AccountId) {
		let granted = Self::limits().granted;
		decrease_allowance_by(account.clone().into(), granted.into());
	}

	/// Move the allowance of `player_id` from [`PlayerStatementLimitsRecord::granted`] to
	/// [`PlayerStatementLimitsRecord::next`], if set.
	///
	/// Grant before revoking: the statement store removes the whole allowance of an account
	/// when one dimension reaches zero.
	pub(crate) fn move_to_next(player_id: &AccountOrPerson<T::AccountId>) {
		let PlayerStatementLimitsRecord { granted, next: Some(next) } = Self::limits() else {
			return;
		};
		let stmt_account = match player_id {
			AccountOrPerson::Person(alias) => AliasToStmtAccount::<T>::get(alias)
				.defensive_proof("indiv-pallet-game: alias player must have a statement account"),
			AccountOrPerson::Account(account) => Some(account.clone()),
		};
		let Some(stmt_account) = stmt_account else { return };
		increase_allowance_by(stmt_account.clone().into(), next.into());
		decrease_allowance_by(stmt_account.into(), granted.into());
	}

	/// Set [`PlayerStatementLimitsRecord::next`] to the dynamic limit if it differs from
	/// [`PlayerStatementLimitsRecord::granted`].
	///
	/// Player processing calls this at its start.
	pub(crate) fn stage_next() {
		let limits = Self::limits();
		let limit = Self::dynamic_player_statement_limit();
		let next = (limit != limits.granted).then_some(limit);
		if next != limits.next {
			PlayerStatementLimits::<T>::put(PlayerStatementLimitsRecord { next, ..limits });
		}
	}

	/// Make [`PlayerStatementLimitsRecord::next`] the granted allowance, if set.
	///
	/// Player processing calls this at its end, once [`Self::move_to_next`] moved every live
	/// player.
	pub(crate) fn apply_next() {
		if let Some(PlayerStatementLimitsRecord { next: Some(next), .. }) =
			PlayerStatementLimits::<T>::get()
		{
			PlayerStatementLimits::<T>::put(PlayerStatementLimitsRecord {
				granted: next,
				next: None,
			});
		}
	}
}
