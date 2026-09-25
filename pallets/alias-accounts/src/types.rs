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

//! Types for the alias-accounts pallet.

use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
pub use indiv_support::traits::{
	Alias, Context, ContextualAlias, Identifier, Incarnation, MembershipProver,
	RevisedContextualAlias, RevisionIndex, RingIndex,
};
use scale_info::TypeInfo;

use verifiable::GenerateVerifiable;

/// The proof type from the configured crypto implementation.
pub type ProofOf<T> =
	<<<T as crate::Config>::MemberService as MembershipProver>::Crypto as GenerateVerifiable>::Proof;

/// Full information about an alias-to-account mapping.
///
/// Stored in `AccountToAlias`, which is what a call reads to resolve an account's person.
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub struct AliasAccountInfo {
	/// The collection the member belongs to.
	pub collection: Identifier,
	/// Incarnation of the collection when the proof was verified. `ring` and `revision` identify
	/// a member set only together with it.
	pub incarnation: Incarnation,
	/// Revision of the ring at the time of proof verification.
	pub revision: RevisionIndex,
	/// Index of the ring within the collection.
	pub ring: RingIndex,
	/// The contextual alias derived from the proof.
	pub ca: ContextualAlias,
}

impl AliasAccountInfo {
	/// Builds a mapping from a validated proof.
	///
	/// `incarnation` must be the current value from the member service.
	pub fn from_validated(
		collection: Identifier,
		incarnation: Incarnation,
		rca: &RevisedContextualAlias,
	) -> Self {
		Self { collection, incarnation, revision: rca.revision, ring: rca.ring, ca: rca.ca.clone() }
	}
}
