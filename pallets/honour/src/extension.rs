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

//! Transaction extension for `indiv-pallet-honour`.

use crate::*;
use codec::{Decode, DecodeWithMemTracking, Encode};
use core::fmt;
use frame_support::{
	ensure,
	pallet_prelude::Weight,
	traits::{IsSubType, OriginTrait, UnixTime},
	CloneNoBound, DefaultNoBound, EqNoBound, PartialEqNoBound,
};
use indiv_support::{
	traits::{RevisionIndex, RingIndex},
	tx_priority,
};
use scale_info::TypeInfo;
use sp_core::Get;
use sp_runtime::{
	traits::{DispatchInfoOf, TransactionExtension, ValidateResult},
	transaction_validity::{
		InvalidTransaction, TransactionSource, TransactionValidityError, ValidTransaction,
	},
};

/// Tag prefix for every transaction that uses a point.
///
/// `bestow` and `clear` share it, so two transactions for the same point conflict in the pool.
const POINT_TAG: &str = "pallet-honour:point";

type RuntimeCallOf<T> = <T as frame_system::Config>::RuntimeCall;

/// Check the state preconditions of a `bestow` call.
fn ensure_can_bestow<T: Config>(
	vote: &VoteData,
	aliases: &VoteAliases,
	now: Seconds,
) -> Result<(), InvalidTransaction> {
	let point = Points::<T>::get(aliases.point_alias);

	match &point {
		// The point already holds the vote of this voter for this subject. Only a change of
		// direction is a new vote.
		Some(PointInfo::Active { subject_alias, direction, .. })
			if *subject_alias == aliases.subject_alias =>
			if *direction == vote.direction {
				return Err(InvalidTransaction::Stale);
			},
		// Another point of this voter holds the vote for this subject.
		_ =>
			if Votes::<T>::contains_key(aliases.subject_alias) {
				return Err(InvalidTransaction::Stale);
			},
	}

	// The end of the freeze does not make a stale vote valid, so the freeze check comes last.
	if point.is_some_and(|point| Pallet::<T>::is_point_frozen(&point, now)) {
		return Err(InvalidTransaction::Future);
	}

	Ok(())
}

/// Check the state preconditions of a `clear` call.
fn ensure_can_clear<T: Config>(
	point_alias: &PointAlias,
	now: Seconds,
) -> Result<(), InvalidTransaction> {
	let Some(point @ PointInfo::Active { .. }) = Points::<T>::get(point_alias) else {
		return Err(InvalidTransaction::Stale);
	};

	if Pallet::<T>::is_point_frozen(&point, now) {
		return Err(InvalidTransaction::Future);
	}

	Ok(())
}

#[derive(Clone, Eq, PartialEq, Encode, Decode, TypeInfo, DecodeWithMemTracking, Debug)]
#[scale_info(skip_type_params(T))]
pub struct VoterAuthData<T: Config> {
	pub proof: RingProofOf<T>,
	/// Ring to validate the `proof` against.
	pub ring_index: RingIndex,
	/// The ring revision the `proof` was constructed against.
	pub revision: RevisionIndex,
}

#[derive(
	Encode,
	Decode,
	TypeInfo,
	EqNoBound,
	CloneNoBound,
	PartialEqNoBound,
	DefaultNoBound,
	DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct VoterAuth<T: Config>(Option<VoterAuthData<T>>);

impl<T: Config> VoterAuth<T> {
	pub fn new(data: Option<VoterAuthData<T>>) -> Self {
		Self(data)
	}
}

impl<T: Config> fmt::Debug for VoterAuth<T> {
	#[cfg(feature = "std")]
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "VoterAuth")
	}

	#[cfg(not(feature = "std"))]
	fn fmt(&self, _: &mut fmt::Formatter) -> fmt::Result {
		Ok(())
	}
}

impl<T: Config + Send + Sync> TransactionExtension<RuntimeCallOf<T>> for VoterAuth<T> {
	const IDENTIFIER: &'static str = "HonourAuth";
	type Implicit = ();

	type Val = ();
	type Pre = ();

	fn weight(&self, call: &RuntimeCallOf<T>) -> Weight {
		if self.0.is_none() {
			return Weight::zero();
		}

		match call.is_sub_type() {
			Some(Call::<T>::clear { .. }) => T::WeightInfo::extension_validate_clear(),
			_ => T::WeightInfo::extension_validate(),
		}
	}

	fn validate(
		&self,
		origin: T::RuntimeOrigin,
		call: &RuntimeCallOf<T>,
		_info: &DispatchInfoOf<RuntimeCallOf<T>>,
		_len: usize,
		_self_implicit: Self::Implicit,
		inherited_implication: &impl Encode,
		_source: TransactionSource,
	) -> ValidateResult<Self::Val, RuntimeCallOf<T>> {
		let Some(VoterAuthData { proof, ring_index, revision }) = &self.0 else {
			// Extension not in use by this transaction: pass through with default validity. The
			// effective priority comes from whichever extension authorizes the call.
			return Ok((ValidTransaction::default(), (), origin));
		};

		// The origin must not have already been authorized by another extension.
		ensure!(
			matches!(origin.as_system_ref(), Some(frame_system::RawOrigin::None)),
			InvalidTransaction::BadSigner
		);

		let Some(call) = call.is_sub_type() else {
			return Err(InvalidTransaction::Call.into());
		};
		let call_valid_from = match call {
			Call::<T>::bestow { call_valid_from, .. } |
			Call::<T>::clear { call_valid_from, .. } => call_valid_from,
			_ => return Err(InvalidTransaction::Call.into()),
		};

		let now = T::Clock::now().as_secs();

		if *call_valid_from > now {
			return Err(InvalidTransaction::Future.into());
		}
		let call_valid_to = call_valid_from.saturating_add(T::CallMortality::get());
		if call_valid_to <= now {
			return Err(InvalidTransaction::Stale.into());
		}

		let message = inherited_implication.using_encoded(sp_crypto_hashing::blake2_256);

		let (point_alias, local_origin) = match call {
			Call::<T>::bestow { vote, .. } => {
				let aliases =
					Pallet::<T>::validate_vote_proof(vote, &message, proof, *ring_index, *revision)
						.map_err(|_| InvalidTransaction::BadProof)?;
				ensure_can_bestow::<T>(vote, &aliases, now)?;

				(aliases.point_alias, Origin::Voter { aliases })
			},
			Call::<T>::clear { point, .. } => {
				let point_alias = Pallet::<T>::validate_point_proof(
					*point,
					&message,
					proof,
					*ring_index,
					*revision,
				)
				.map_err(|_| InvalidTransaction::BadProof)?;
				ensure_can_clear::<T>(&point_alias, now)?;

				(point_alias, Origin::PointHolder { point_alias })
			},
			_ => return Err(InvalidTransaction::Call.into()),
		};

		let validity = ValidTransaction::with_tag_prefix(POINT_TAG)
			.and_provides(point_alias)
			.priority(tx_priority::USER_HIGH)
			.into();

		let mut origin = origin;
		origin.set_caller_from(local_origin);

		Ok((validity, (), origin))
	}

	fn prepare(
		self,
		_val: Self::Val,
		_origin: &T::RuntimeOrigin,
		_call: &RuntimeCallOf<T>,
		_info: &DispatchInfoOf<RuntimeCallOf<T>>,
		_len: usize,
	) -> Result<Self::Pre, TransactionValidityError> {
		Ok(())
	}
}
