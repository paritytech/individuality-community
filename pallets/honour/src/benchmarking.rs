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

use crate::{
	migration::v1::{v0, MigratePointsToActive},
	*,
};
use codec::Encode;
use core::marker::{Send, Sync};
use frame_benchmarking::{v2::*, BenchmarkError};
use frame_support::{
	migrations::SteppedMigration,
	traits::{Get, StorageVersion, UnixTime},
	weights::WeightMeter,
};
use indiv_support::traits::Context;
use sp_runtime::{
	traits::{AsTransactionAuthorizedOrigin, DispatchTransaction},
	Saturating,
};

fn advance_to<T: Config>(b: u32) {
	use frame_system::Pallet as System;

	while System::<T>::block_number() < b.into() {
		System::<T>::set_block_number(System::<T>::block_number().saturating_add(1u32.into()));
	}
}

fn assert_has_event<T: Config>(generic_event: <T as frame_system::Config>::RuntimeEvent) {
	frame_system::Pallet::<T>::assert_has_event(generic_event.into());
}

/// Non-zero clock value (seconds since the Unix epoch) the benchmarks set before reading
/// [`Config::Clock`], so `UnixTime::now()` is not queried at genesis (which logs an error and
/// returns `0`).
const BENCH_TIME: Seconds = 1_000;

/// Seeds the membership ring used by [`VoterAuth`](crate::extension::VoterAuth) and produces a
/// proof that verifies for the given contexts and message.
///
/// The proof must validate against whatever ring the runtime's [`Config::MemberService`] actually
/// holds, so the setup is environment-specific: the pallet mock proves against its in-process mock
/// ring, while a real runtime seeds its configured member service and proves against that ring.
pub trait BenchmarkHelper<T: Config> {
	/// Set [`Config::Clock`] to `now` seconds since the Unix epoch.
	fn set_time(now: Seconds);

	/// Seed the ring at ring index `0` and return a proof valid for `contexts` + `message`.
	fn seed_and_create_proof(contexts: &[Context], message: &[u8]) -> RingProofOf<T>;
}

#[benchmarks(
	where T: Send + Sync,
		<T as frame_system::Config>::RuntimeCall:
			Dispatchable<Info = DispatchInfo, PostInfo = PostDispatchInfo> + IsSubType<Call<T>> + From<Call<T>> + GetDispatchInfo,
		<T as frame_system::Config>::RuntimeOrigin: AsTransactionAuthorizedOrigin,
)]
mod benches {
	use frame_support::{
		dispatch::{DispatchInfo, GetDispatchInfo, PostDispatchInfo},
		traits::IsSubType,
	};
	use frame_system::RawOrigin;
	use sp_crypto_hashing::blake2_256;
	use sp_runtime::{generic::ExtensionVersion, traits::Dispatchable};

	use crate::extension::{VoterAuth, VoterAuthData};

	use super::*;

	#[benchmark]
	fn bestow() -> Result<(), BenchmarkError> {
		const POINT: PointId = 0;
		const POINT_ALIAS: PointAlias = [1; 32];

		const SUBJECT_1: SubjectId = [2; 32];
		const SUBJECT_1_ALIAS: SubjectAlias = [3; 32];

		const SUBJECT_2: SubjectId = [4; 32];
		const SUBJECT_2_ALIAS: SubjectAlias = [5; 32];

		T::BenchmarkHelper::set_time(BENCH_TIME);
		advance_to::<T>(2);

		let origin: <T as frame_system::Config>::RuntimeOrigin = Origin::Voter {
			aliases: VoteAliases { subject_alias: SUBJECT_1_ALIAS, point_alias: POINT_ALIAS },
		}
		.into();
		let vote_data =
			VoteData { subject: SUBJECT_1, point: POINT, direction: Direction::Honourable };
		Pallet::<T>::bestow(origin, vote_data, Default::default())
			.map_err(|_| BenchmarkError::Stop("bestow failed"))?;

		assert_has_event::<T>(
			Event::<T>::VoteCast { subject: SUBJECT_1, direction: Direction::Honourable }.into(),
		);

		let origin: <T as frame_system::Config>::RuntimeOrigin = Origin::Voter {
			aliases: VoteAliases { subject_alias: SUBJECT_2_ALIAS, point_alias: POINT_ALIAS },
		}
		.into();
		let vote_data =
			VoteData { subject: SUBJECT_2, point: POINT, direction: Direction::Honourable };
		#[extrinsic_call]
		_(origin, vote_data, Default::default());

		assert_has_event::<T>(
			Event::<T>::VoteReused {
				old_subject: SUBJECT_1,
				old_direction: Direction::Honourable,
				new_subject: SUBJECT_2,
				new_direction: Direction::Honourable,
			}
			.into(),
		);

		Ok(())
	}

	#[benchmark]
	fn clear() -> Result<(), BenchmarkError> {
		const POINT: PointId = 0;
		const POINT_ALIAS: PointAlias = [1; 32];

		const SUBJECT: SubjectId = [2; 32];
		const SUBJECT_ALIAS: SubjectAlias = [3; 32];

		T::BenchmarkHelper::set_time(BENCH_TIME);
		advance_to::<T>(2);

		let origin: <T as frame_system::Config>::RuntimeOrigin = Origin::Voter {
			aliases: VoteAliases { subject_alias: SUBJECT_ALIAS, point_alias: POINT_ALIAS },
		}
		.into();
		let vote_data =
			VoteData { subject: SUBJECT, point: POINT, direction: Direction::Honourable };
		Pallet::<T>::bestow(origin, vote_data, Default::default())
			.map_err(|_| BenchmarkError::Stop("bestow failed"))?;

		let origin: <T as frame_system::Config>::RuntimeOrigin =
			Origin::PointHolder { point_alias: POINT_ALIAS }.into();
		#[extrinsic_call]
		_(origin, POINT, Default::default());

		assert_has_event::<T>(
			Event::<T>::VoteCleared { subject: SUBJECT, direction: Direction::Honourable }.into(),
		);
		assert_eq!(
			Points::<T>::get(POINT_ALIAS),
			Some(PointInfo::Cleared { cleared_at: BENCH_TIME })
		);
		assert!(!Votes::<T>::contains_key(SUBJECT_ALIAS));

		Ok(())
	}

	#[benchmark]
	fn extension_validate() -> Result<(), BenchmarkError> {
		T::BenchmarkHelper::set_time(BENCH_TIME);

		let vote = VoteData { subject: [1; 32], point: 0, direction: Direction::Honourable };
		let now = <T as Config>::Clock::now().as_secs();

		let call = Call::bestow { vote: vote.clone(), call_valid_from: now };
		let call: <T as frame_system::Config>::RuntimeCall = call.into();

		let ext_version: ExtensionVersion = 0;
		let message = (ext_version, &call).using_encoded(blake2_256);

		let proof = T::BenchmarkHelper::seed_and_create_proof(&vote.get_contexts(), &message);

		let extension: VoterAuth<T> =
			VoterAuth::new(Some(VoterAuthData { proof, ring_index: 0, revision: 0 }));

		let info = call.get_dispatch_info();
		let len = call.encoded_size();
		let post_info = PostDispatchInfo::default();

		#[block]
		{
			extension
				.test_run(RawOrigin::None.into(), &call, &info, len, 0, |_| Ok(post_info))
				.unwrap()
				.unwrap();
		}

		Ok(())
	}

	#[benchmark]
	fn extension_validate_clear() -> Result<(), BenchmarkError> {
		const POINT: PointId = 0;

		// The extension accepts the call only after the freeze of the point ends.
		T::BenchmarkHelper::set_time(T::PointFreezeDuration::get().saturating_add(BENCH_TIME));
		let now = <T as Config>::Clock::now().as_secs();

		let call = Call::clear { point: POINT, call_valid_from: now };
		let call: <T as frame_system::Config>::RuntimeCall = call.into();

		let ext_version: ExtensionVersion = 0;
		let message = (ext_version, &call).using_encoded(blake2_256);

		let proof = T::BenchmarkHelper::seed_and_create_proof(&[context::point(POINT)], &message);

		// Direct the point of the benchmark voter, so that the extension finds a vote to clear.
		let point_alias = Pallet::<T>::validate_point_proof(POINT, &message, &proof, 0, 0)
			.map_err(|_| BenchmarkError::Stop("proof validation failed"))?;
		Points::<T>::insert(
			point_alias,
			PointInfo::Active {
				subject: [1; 32],
				subject_alias: [2; 32],
				direction: Direction::Honourable,
				last_used_at: BENCH_TIME,
			},
		);

		let extension: VoterAuth<T> =
			VoterAuth::new(Some(VoterAuthData { proof, ring_index: 0, revision: 0 }));

		let info = call.get_dispatch_info();
		let len = call.encoded_size();
		let post_info = PostDispatchInfo::default();

		#[block]
		{
			extension
				.test_run(RawOrigin::None.into(), &call, &info, len, 0, |_| Ok(post_info))
				.unwrap()
				.unwrap();
		}

		Ok(())
	}

	#[benchmark]
	fn migrate_v1_step() {
		for i in 0..2u8 {
			v0::Points::<T>::insert(
				[i; 32],
				v0::PointInfo {
					subject: [1; 32],
					subject_alias: [2; 32],
					direction: Direction::Honourable,
					last_used_at: BENCH_TIME,
				},
			);
		}
		StorageVersion::new(0).put::<Pallet<T>>();
		let mut point_aliases = v0::Points::<T>::iter_keys();
		let first = point_aliases.next().expect("two points are seeded");
		let second = point_aliases.next().expect("two points are seeded");
		let mut meter = WeightMeter::with_limit(T::WeightInfo::migrate_v1_step());
		let cursor;

		#[block]
		{
			cursor = MigratePointsToActive::<T>::step(Some(first), &mut meter).unwrap();
		}

		assert_eq!(cursor, Some(second));
		assert_eq!(
			Points::<T>::get(second),
			Some(PointInfo::Active {
				subject: [1; 32],
				subject_alias: [2; 32],
				direction: Direction::Honourable,
				last_used_at: BENCH_TIME,
			})
		);
	}

	// Implements a test for each benchmark. Execute with:
	// `cargo test -p indiv-pallet-honour --features runtime-benchmarks`.
	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
