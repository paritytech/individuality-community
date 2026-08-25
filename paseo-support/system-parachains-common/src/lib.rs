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

//! Shared types between system-parachains runtimes.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod randomness;

#[cfg(feature = "multi-asset-bounties")]
pub mod multi_asset_bounty_sources;

/// Extra runtime APIs.
pub mod apis {
	/// Information about the current issuance rate of the system.
	///
	/// Both fields should be treated as best-effort, given that the issuance rate might not be
	/// fully predict-able.
	#[derive(scale_info::TypeInfo, codec::Encode, codec::Decode, Eq, PartialEq)]
	#[cfg_attr(feature = "std", derive(Debug))]
	pub struct InflationInfo {
		/// The rate of issuance estimated per annum, represented as a `Perquintill`.
		pub issuance: sp_runtime::Perquintill,
		/// Next amount that we anticipate to mint.
		///
		/// First item is the amount that goes to stakers, second is the leftover that is usually
		/// forwarded to the treasury.
		pub next_mint: (polkadot_primitives::Balance, polkadot_primitives::Balance),
	}

	sp_api::decl_runtime_apis! {
		pub trait Inflation {
			/// Return the current estimates of the issuance amount.
			///
			/// This is marked as experimental in light of RFC#89. Nonetheless, its usage is highly
			/// recommended over trying to read-storage, or re-create the onchain logic.
			fn experimental_issuance_prediction_info() -> InflationInfo;
		}
	}
}
