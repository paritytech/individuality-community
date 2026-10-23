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

//! Records of statement allowances that a pallet granted.
//!
//! The statement allowance of an account is the sum of the grants of several pallets. A
//! governance-mutable limit can change between a grant and its revocation, so a pallet records
//! each grant and revokes the recorded amount, not the current limit.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;
use sp_statement_store::StatementAllowance;

/// A statement allowance that a pallet granted to an account.
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo,
)]
pub struct GrantedStatementAllowance {
	/// Maximum number of statements.
	pub max_count: u32,
	/// Maximum total statement size in bytes.
	pub max_size: u32,
}

impl From<StatementAllowance> for GrantedStatementAllowance {
	fn from(value: StatementAllowance) -> Self {
		Self { max_count: value.max_count, max_size: value.max_size }
	}
}

impl From<GrantedStatementAllowance> for StatementAllowance {
	fn from(value: GrantedStatementAllowance) -> Self {
		Self { max_count: value.max_count, max_size: value.max_size }
	}
}
