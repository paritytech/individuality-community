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

//! A statement allowance with a fixed maximum encoded length.
//!
//! [`StatementAllowance`] does not implement `MaxEncodedLen`, so it can't be stored.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_statement_store::StatementAllowance;

/// A [`StatementAllowance`] with a fixed maximum encoded length.
///
/// TODO(<https://github.com/paritytech/polkadot-sdk/issues/13675>): Remove once the upstream
/// struct has `MaxEncodedLen`.
#[derive(
	Clone,
	Default,
	PartialEq,
	Eq,
	Debug,
	Encode,
	Decode,
	DecodeWithMemTracking,
	MaxEncodedLen,
	TypeInfo,
)]
pub struct BoundedStatementAllowance {
	/// Maximum total statement size in bytes.
	pub max_size: u32,
	/// Maximum number of statements.
	pub max_count: u32,
}

impl From<StatementAllowance> for BoundedStatementAllowance {
	fn from(value: StatementAllowance) -> Self {
		Self { max_size: value.max_size, max_count: value.max_count }
	}
}

impl From<BoundedStatementAllowance> for StatementAllowance {
	fn from(value: BoundedStatementAllowance) -> Self {
		Self { max_size: value.max_size, max_count: value.max_count }
	}
}
