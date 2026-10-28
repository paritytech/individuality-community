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

//! Runtime API definition for the game pallet.

#![cfg_attr(not(feature = "std"), no_std)]

use codec::Codec;
use indiv_support::identity::AccountOrPerson;
use sp_runtime::DispatchError;

sp_api::decl_runtime_apis! {
	/// The API to query the deposit for gameplay and what a check-in grants.
	#[api_version(2)]
	pub trait PalletGameApi<Balance, AccountId>
	where
		Balance: Codec,
		AccountId: Codec,
	{
		/// Returns the deposit necessary to play a game.
		fn play_deposit() -> Balance;

		/// Returns the number of credits a `check_in` by `player` grants now for the current game,
		/// or the error that call fails with.
		///
		/// A refused check-in pays a fee, so a wallet checks this before it submits one. A
		/// check-in included later can still fail, because the state can change in between.
		#[api_version(2)]
		fn check_in_credits(player: AccountOrPerson<AccountId>) -> Result<u32, DispatchError>;
	}
}
