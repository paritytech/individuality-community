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

//! Honour pallet.
//!
//! Baseline reputation system for the People Chain. Individuals can direct up to 256 points
//! toward arbitrary subjects, each either honourable (+1) or dishonourable (-1).
//!
//! Once a point is bestowed, it can be redirected to another subject (or the same subject with a
//! different direction) or cleared. A voter clears a point to withdraw its vote without casting a
//! new one.
//!
//! Every use of a point, bestowing or clearing, freezes the point for
//! [`Config::PointFreezeDuration`] seconds. The transactions are feeless, so the freeze is the
//! spam protection: each person can submit at most one transaction per point in that duration.
//!
//! To bestow or clear a point, the transaction must use the [`VoterAuth`](extension::VoterAuth)
//! extension. For the transaction to succeed, a valid ring proof must be attached, generated for a
//! ring belonging to the `indiv-pallet-people` pallet, thus proving the voter is a verified human.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(test)]
mod bench_helpers;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
pub mod extension;
pub mod migration;
pub mod types;
pub mod weights;

pub use pallet::*;
pub use types::*;
pub use weights::WeightInfo;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use alloc::vec::Vec;
	use frame_support::{
		dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo},
		pallet_prelude::*,
		traits::{IsSubType, UnixTime},
		Blake2_128Concat,
	};
	use frame_system::pallet_prelude::*;
	use indiv_support::traits::{
		Alias, Context, ContextualAlias, MembershipMultiProver, RevisionIndex, RingIndex,
		PEOPLE_IDENTIFIER,
	};
	use sp_runtime::traits::{BadOrigin, Dispatchable};
	use verifiable::GenerateVerifiable;

	const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::origin]
	#[derive(
		Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
	)]
	pub enum Origin {
		/// A person that proved the aliases of a vote.
		Voter { aliases: VoteAliases },
		/// A person that proved the alias of a point.
		PointHolder { point_alias: PointAlias },
	}

	#[pallet::config]
	pub trait Config:
		frame_system::Config<
		RuntimeOrigin: From<Origin>
		                   + From<<Self::RuntimeOrigin as OriginTrait>::PalletsOrigin>
		                   + OriginTrait<
			PalletsOrigin: From<Origin>
			                   + TryInto<
				Origin,
				Error = <Self::RuntimeOrigin as OriginTrait>::PalletsOrigin,
			>,
		>,
		RuntimeCall: Parameter
		                 + GetDispatchInfo
		                 + IsSubType<Call<Self>>
		                 + Dispatchable<
			RuntimeOrigin = Self::RuntimeOrigin,
			Info = DispatchInfo,
			PostInfo = PostDispatchInfo,
		>,
	>
	{
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Ring-membership prover used to verify multi-context proofs sent through
		/// the [`VoterAuth`](extension::VoterAuth) extension.
		type MemberService: MembershipMultiProver<
			Crypto: GenerateVerifiable<Proof: Send + Sync + DecodeWithMemTracking>,
		>;

		/// Clock used to track the current time.
		type Clock: UnixTime;

		/// Duration in seconds that must pass before a point can be reused.
		#[pallet::constant]
		type PointFreezeDuration: Get<Seconds>;

		/// Duration in seconds a call remains valid after its `call_valid_from`.
		#[pallet::constant]
		type CallMortality: Get<Seconds>;

		/// Helper that seeds the membership ring and produces a valid proof for benchmarks.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: benchmarking::BenchmarkHelper<Self>;
	}

	/// Points that have been used by voters.
	#[pallet::storage]
	pub type Points<T: Config> = StorageMap<_, Blake2_128Concat, PointAlias, PointInfo>;

	/// Votes that have been bestowed.
	///
	/// Prevents double-voting on the same subject by the same voter, as [`SubjectAlias`] is
	/// uniquely derived from [`SubjectId`] for each voter.
	#[pallet::storage]
	pub type Votes<T: Config> = StorageMap<_, Blake2_128Concat, SubjectAlias, ()>;

	/// Absolute honour score of a subject. Initialized to -1 to offset self-votes.
	#[pallet::storage]
	pub type Tally<T: Config> = StorageMap<_, Blake2_128Concat, SubjectId, Honour>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A subject was voted on with a point that held no vote.
		VoteCast {
			/// The subject that was voted on.
			subject: SubjectId,
			/// The direction of the vote.
			direction: Direction,
		},
		/// A point was redirected from one subject to another.
		VoteReused {
			/// The subject that previously held the redirected point.
			old_subject: SubjectId,
			/// The previous direction of the point.
			old_direction: Direction,
			/// The subject that now holds the redirected point.
			new_subject: SubjectId,
			/// The new direction of the vote.
			new_direction: Direction,
		},
		/// The honour score of a subject has changed.
		HonourChanged {
			/// The subject whose honour score changed.
			subject: SubjectId,
			/// The honour value before the update.
			old_value: Honour,
			/// The honour value after the update.
			new_value: Honour,
		},
		/// A voter cleared a point and withdrew its vote.
		VoteCleared {
			/// The subject that held the cleared point.
			subject: SubjectId,
			/// The direction of the withdrawn vote.
			direction: Direction,
		},
	}

	#[pallet::error]
	#[derive(PartialEq)]
	pub enum Error<T> {
		/// Arithmetic error like over/underflow, division by zero or similar.
		Arithmetic,
		/// There is already a vote by the same voter for the same subject.
		SubjectAlreadyVoted,
		/// The provided ring proof failed verification.
		InvalidProof,
		/// The point holds no vote.
		PointNotDirected,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			let freeze = T::PointFreezeDuration::get();
			let mortality = T::CallMortality::get();
			// A use of a point freezes it for `freeze` seconds. A transaction that the voter signed
			// before that use expires within `mortality` seconds, so nobody can replay it when the
			// freeze ends.
			assert!(
				mortality <= freeze,
				"CallMortality must be <= PointFreezeDuration to prevent the replay of transactions"
			);
		}
	}

	#[pallet::call(weight(<T as Config>::WeightInfo))]
	impl<T: Config> Pallet<T> {
		/// Bestow a vote.
		///
		/// Accepts only the [`Origin::Voter`] origin, which is created by verifying a ring proof
		/// in the [`VoterAuth`](extension::VoterAuth) transaction extension.
		#[pallet::call_index(0)]
		pub fn bestow(
			origin: OriginFor<T>,
			vote: VoteData,
			_call_valid_from: Seconds,
		) -> DispatchResult {
			let aliases = match origin.into_caller().try_into() {
				Ok(Origin::Voter { aliases }) => aliases,
				_ => return Err(BadOrigin.into()),
			};

			Self::do_bestow(vote, aliases).map_err(Into::into)
		}

		/// Clear a point and withdraw the vote that it holds.
		///
		/// Accepts only the [`Origin::PointHolder`] origin, which is created by verifying a ring
		/// proof in the [`VoterAuth`](extension::VoterAuth) transaction extension.
		#[pallet::call_index(1)]
		pub fn clear(
			origin: OriginFor<T>,
			_point: PointId,
			_call_valid_from: Seconds,
		) -> DispatchResult {
			let point_alias = match origin.into_caller().try_into() {
				Ok(Origin::PointHolder { point_alias }) => point_alias,
				_ => return Err(BadOrigin.into()),
			};

			Self::do_clear(point_alias).map_err(Into::into)
		}
	}

	impl<T: Config> Pallet<T> {
		pub fn do_bestow(vote: VoteData, aliases: VoteAliases) -> Result<(), Error<T>> {
			let now = T::Clock::now().as_secs();

			if let Some(PointInfo::Active {
				subject: old_subject,
				subject_alias: old_subject_alias,
				direction: old_direction,
				..
			}) = Points::<T>::get(aliases.point_alias)
			{
				Votes::<T>::remove(old_subject_alias);

				Pallet::<T>::deposit_event(Event::<T>::VoteReused {
					old_subject,
					old_direction,
					new_subject: vote.subject,
					new_direction: vote.direction,
				});

				let result = Self::tally(&old_subject, old_direction.opposite());

				Pallet::<T>::deposit_event(Event::<T>::HonourChanged {
					subject: old_subject,
					old_value: result.old_value,
					new_value: result.new_value,
				});
			} else {
				Pallet::<T>::deposit_event(Event::<T>::VoteCast {
					subject: vote.subject,
					direction: vote.direction,
				});
			}

			if Votes::<T>::contains_key(aliases.subject_alias) {
				return Err(Error::<T>::SubjectAlreadyVoted);
			}

			Votes::<T>::insert(aliases.subject_alias, ());

			Points::<T>::insert(
				aliases.point_alias,
				PointInfo::Active {
					subject: vote.subject,
					subject_alias: aliases.subject_alias,
					direction: vote.direction,
					last_used_at: now,
				},
			);

			let result = Self::tally(&vote.subject, vote.direction);

			Pallet::<T>::deposit_event(Event::<T>::HonourChanged {
				subject: vote.subject,
				old_value: result.old_value,
				new_value: result.new_value,
			});

			Ok(())
		}

		pub fn do_clear(point_alias: PointAlias) -> Result<(), Error<T>> {
			let Some(PointInfo::Active { subject, subject_alias, direction, .. }) =
				Points::<T>::get(point_alias)
			else {
				return Err(Error::<T>::PointNotDirected);
			};

			Votes::<T>::remove(subject_alias);
			Points::<T>::insert(
				point_alias,
				PointInfo::Cleared { cleared_at: T::Clock::now().as_secs() },
			);

			Pallet::<T>::deposit_event(Event::<T>::VoteCleared { subject, direction });

			let result = Self::tally(&subject, direction.opposite());

			Pallet::<T>::deposit_event(Event::<T>::HonourChanged {
				subject,
				old_value: result.old_value,
				new_value: result.new_value,
			});

			Ok(())
		}

		/// Amend the tally of a subject without knowing whether it exists.
		fn tally(subject: &SubjectId, direction: Direction) -> TallyResult {
			let old_value = Tally::<T>::get(subject).unwrap_or(SUBJECT_DEFAULT_SCORE);
			let new_value = old_value.saturating_add(direction.score());

			if new_value == SUBJECT_DEFAULT_SCORE {
				Tally::<T>::remove(subject);
			} else {
				Tally::<T>::insert(subject, new_value);
			}

			TallyResult { old_value, new_value }
		}

		/// Check if `point` is frozen at `now`.
		pub(crate) fn is_point_frozen(point: &PointInfo, now: Seconds) -> bool {
			point.last_used_at().saturating_add(T::PointFreezeDuration::get()) > now
		}

		/// Verify the ring proof and return the aliases corresponding to the vote.
		pub(crate) fn validate_vote_proof(
			vote: &VoteData,
			message: &[u8],
			proof: &RingProofOf<T>,
			ring_index: RingIndex,
			revision: RevisionIndex,
		) -> Result<VoteAliases, Error<T>> {
			Self::validate_proof(vote.get_contexts(), message, proof, ring_index, revision)
				.map(VoteAliases::from_aliases)
		}

		/// Verify the ring proof and return the alias corresponding to the point.
		pub(crate) fn validate_point_proof(
			point: PointId,
			message: &[u8],
			proof: &RingProofOf<T>,
			ring_index: RingIndex,
			revision: RevisionIndex,
		) -> Result<PointAlias, Error<T>> {
			Self::validate_proof([context::point(point)], message, proof, ring_index, revision)
				.map(|[point_alias]| point_alias)
		}

		/// Verify the ring proof and return one alias for each context, in the same order.
		fn validate_proof<const N: usize>(
			contexts: [Context; N],
			message: &[u8],
			proof: &RingProofOf<T>,
			ring_index: RingIndex,
			revision: RevisionIndex,
		) -> Result<[Alias; N], Error<T>> {
			let aliases = T::MemberService::verify_membership_multi_context(
				PEOPLE_IDENTIFIER,
				proof,
				ring_index,
				revision,
				&contexts,
				message,
			)
			.map_err(|_| Error::<T>::InvalidProof)?;

			let aliases: [ContextualAlias; N] =
				aliases.try_into().map_err(|_: Vec<_>| Error::<T>::InvalidProof)?;

			// should be impossible, but check anyway
			if aliases.iter().zip(&contexts).any(|(alias, context)| alias.context != *context) {
				return Err(Error::<T>::InvalidProof);
			}

			Ok(aliases.map(|alias| alias.alias))
		}
	}

	impl<T: Config> inspect::Score for Pallet<T> {
		fn read(subject: &SubjectId) -> Honour {
			Tally::<T>::get(subject).unwrap_or(SUBJECT_DEFAULT_SCORE)
		}
	}

	struct TallyResult {
		old_value: Honour,
		new_value: Honour,
	}
}
