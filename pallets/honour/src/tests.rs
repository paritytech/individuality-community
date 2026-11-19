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
	bench_helpers::{prove_contexts, prove_vote},
	extension::{VoterAuth, VoterAuthData},
	inspect::Score,
	mock::*,
	pallet::Pallet as HonourPallet,
	*,
};
use codec::Encode;
use frame_support::{
	assert_err, assert_noop, assert_ok,
	dispatch::{GetDispatchInfo, PostDispatchInfo},
	pallet_prelude::{InvalidTransaction, TransactionSource, TransactionValidityError, Weight},
	traits::UnixTime,
};
use frame_system::RawOrigin;
use mock::set_time;
use sp_crypto_hashing::blake2_256;
use sp_runtime::{
	generic::ExtensionVersion,
	traits::{BadOrigin, DispatchTransaction, Dispatchable, TransactionExtension},
};
use verifiable::GenerateVerifiable;

type TestCrypto = crate::CryptoOf<Test>;

fn read_score(subject_id: u8) -> crate::Honour {
	<HonourPallet<Test> as Score>::read(&subject(subject_id))
}

fn make_vote(
	voter: u8,
	point: PointId,
	subject: SubjectId,
	direction: Direction,
) -> (RuntimeOrigin, VoteData) {
	let secret_key = voter_secret_key(voter);

	let subject_ctx = context::subject(&subject);
	let point_ctx = context::point(point);

	let subject_alias = TestCrypto::alias_in_context(&secret_key, &subject_ctx)
		.expect("Failed to create subject alias");
	let point_alias = TestCrypto::alias_in_context(&secret_key, &point_ctx)
		.expect("Failed to create point alias");

	let vote_data = VoteData { subject, point, direction };

	let origin =
		crate::Origin::Voter { aliases: crate::VoteAliases { subject_alias, point_alias } };

	(origin.into(), vote_data)
}

/// Whether the point is used and frozen at `now`.
fn is_frozen(point_alias: &PointAlias, now: Seconds) -> bool {
	Points::<Test>::get(point_alias)
		.is_some_and(|point| HonourPallet::<Test>::is_point_frozen(&point, now))
}

fn get_point_alias(voter: u8, point: PointId) -> PointAlias {
	TestCrypto::alias_in_context(&voter_secret_key(voter), &context::point(point))
		.expect("Failed to create point alias")
}

fn voter_secret_key(voter: u8) -> <TestCrypto as GenerateVerifiable>::Secret {
	TestCrypto::new_secret([voter; 32])
}

fn subject(n: u8) -> SubjectId {
	[n; 32]
}

/// The dispatch accepts a second vote for the same subject in the same direction and changes
/// nothing. The extension rejects such a transaction.
#[test]
fn bestow_reuse_can_reuse_same_subject() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;

		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(read_score(1), 0);

		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(read_score(1), 0);
	});
}

/// A user may change their vote for a subject to the opposite direction.
#[test]
fn bestow_reuse_can_reuse_same_subject_different_direction() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;

		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(read_score(1), 0);

		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Dishonourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(read_score(1), -2);

		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(read_score(1), 0);
	});
}

#[test]
fn double_spend_impossible() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;

		let (origin_1, vote_1) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin_1.clone(), vote_1.clone(), 0));

		assert_eq!(read_score(1), 0);
		assert_eq!(read_score(2), -1);

		let (origin_2, vote_2) = make_vote(VOTER, 0, subject(2), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin_2, vote_2, 0));

		assert_eq!(read_score(1), -1);
		assert_eq!(read_score(2), 0);
	});
}

#[test]
fn double_vote_impossible() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;

		let (origin_1, vote_1) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin_1, vote_1, 0));

		let (origin_2, vote_2) = make_vote(VOTER, 1, subject(1), Direction::Honourable);
		assert_noop!(
			HonourPallet::<Test>::bestow(origin_2, vote_2, 0),
			Error::<Test>::SubjectAlreadyVoted
		);
	});
}

#[test]
fn score_reads_correctly() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;

		assert_eq!(<HonourPallet::<Test> as Score>::read(&subject(1)), -1);

		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));

		assert_eq!(<HonourPallet::<Test> as Score>::read(&subject(1)), 0);

		let (origin, vote) = make_vote(VOTER, 0, subject(2), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));

		assert_eq!(<HonourPallet::<Test> as Score>::read(&subject(1)), -1);
		assert_eq!(<HonourPallet::<Test> as Score>::read(&subject(2)), 0);
	});
}

#[test]
fn events_deposited() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		const VOTER: u8 = 0;

		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));

		System::assert_has_event(
			Event::<Test>::VoteCast { subject: subject(1), direction: Direction::Honourable }
				.into(),
		);
		System::assert_has_event(
			Event::<Test>::HonourChanged { subject: subject(1), old_value: -1, new_value: 0 }
				.into(),
		);

		System::reset_events();

		let (origin, vote) = make_vote(VOTER, 0, subject(2), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));

		System::assert_has_event(
			Event::<Test>::VoteReused {
				old_subject: subject(1),
				old_direction: Direction::Honourable,
				new_subject: subject(2),
				new_direction: Direction::Honourable,
			}
			.into(),
		);
		System::assert_has_event(
			Event::<Test>::HonourChanged { subject: subject(1), old_value: 0, new_value: -1 }
				.into(),
		);
		System::assert_has_event(
			Event::<Test>::HonourChanged { subject: subject(2), old_value: -1, new_value: 0 }
				.into(),
		);
	});
}

#[test]
fn is_vote_frozen_works() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		const POINT: u8 = 0;

		let point_alias = get_point_alias(VOTER, POINT);

		let now = 2;
		set_time(now);

		assert!(!is_frozen(&point_alias, now));

		let (origin, vote) = make_vote(VOTER, POINT, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));

		assert!(is_frozen(&point_alias, now));

		let now = now + PointFreezeDuration::get() - 1;
		set_time(now);

		assert!(is_frozen(&point_alias, now), "Still frozen");

		let now = now + 1;
		set_time(now);

		assert!(!is_frozen(&point_alias, now), "Not frozen");
	});
}

#[test]
fn validate_vote_works() {
	new_test_ext().execute_with(|| {
		let message = &[0, 1, 2, 3];

		let (_, vote) = make_vote(0, 0, subject(0), Direction::Honourable);
		let proof = prove_vote::<Test>(&vote, 0, message);

		assert_ok!(Pallet::<Test>::validate_vote_proof(&vote, message, &proof, 0, 0));

		let different_message = &[3, 2, 1, 0];
		assert_err!(
			Pallet::<Test>::validate_vote_proof(&vote, different_message, &proof, 0, 0),
			Error::<Test>::InvalidProof
		);
	});
}

#[test]
fn validate_vote_rejects_unknown_revision() {
	new_test_ext().execute_with(|| {
		let message = &[0, 1, 2, 3];

		let (_, vote) = make_vote(0, 0, subject(0), Direction::Honourable);
		let proof = prove_vote::<Test>(&vote, 0, message);

		// The mock ring only exposes revision 0; any other revision must be rejected.
		assert_err!(
			Pallet::<Test>::validate_vote_proof(&vote, message, &proof, 0, 1),
			Error::<Test>::InvalidProof
		);
	});
}

#[test]
fn validate_vote_rejects_unknown_ring() {
	new_test_ext().execute_with(|| {
		let message = &[0, 1, 2, 3];

		let (_, vote) = make_vote(0, 0, subject(0), Direction::Honourable);
		let proof = prove_vote::<Test>(&vote, 0, message);

		// The mock only has ring 0; any other ring index must be rejected.
		assert_err!(
			Pallet::<Test>::validate_vote_proof(&vote, message, &proof, 1, 0),
			Error::<Test>::InvalidProof
		);
	});
}

#[test]
fn tx_extension_checks_call_validity_time() {
	new_test_ext().execute_with(|| {
		set_time(CallMortality::get() + 1);

		let now = <Test as Config>::Clock::now().as_secs();
		let vote = VoteData { subject: [1; 32], point: 0, direction: Direction::Honourable };

		let call_bestow = |call_valid_from: Seconds| -> Result<(), TransactionValidityError> {
			let call = Call::bestow { vote: vote.clone(), call_valid_from };
			let call: <Test as frame_system::Config>::RuntimeCall = call.into();
			let ext_version: ExtensionVersion = 0;
			let message = (ext_version, &call).using_encoded(blake2_256);
			let proof = prove_vote::<Test>(&vote, 0, &message);

			let extension: VoterAuth<Test> =
				VoterAuth::new(Some(VoterAuthData { proof, ring_index: 0, revision: 0 }));

			let call: <Test as frame_system::Config>::RuntimeCall = call.into();
			let info = call.get_dispatch_info();
			let len = call.encoded_size();
			let post_info = PostDispatchInfo::default();

			extension
				.test_run(RawOrigin::None.into(), &call, &info, len, 0, |_| Ok(post_info))?
				.unwrap();
			Ok(())
		};

		assert_ok!(call_bestow(now));

		assert_err!(
			call_bestow(now + 1),
			TransactionValidityError::Invalid(InvalidTransaction::Future)
		);

		assert_err!(
			call_bestow(now - CallMortality::get()),
			TransactionValidityError::Invalid(InvalidTransaction::Stale)
		);
	});
}

/// A `bestow` on a point used in the last `PointFreezeDuration` seconds is rejected by the
/// extension's freeze check, even with a fresh mortality window; it thaws after exactly
/// `PointFreezeDuration` seconds.
#[test]
fn tx_extension_rejects_frozen_point() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		const POINT: PointId = 0;
		set_time(0);

		let try_bestow = |subject_n: u8| -> Result<(), TransactionValidityError> {
			let now = <Test as Config>::Clock::now().as_secs();
			let vote = VoteData {
				subject: subject(subject_n),
				point: POINT,
				direction: Direction::Honourable,
			};
			let call: <Test as frame_system::Config>::RuntimeCall =
				Call::bestow { vote: vote.clone(), call_valid_from: now }.into();
			let ext_version: ExtensionVersion = 0;
			let message = (ext_version, &call).using_encoded(blake2_256);
			let proof = prove_vote::<Test>(&vote, VOTER, &message);
			let extension =
				VoterAuth::<Test>::new(Some(VoterAuthData { proof, ring_index: 0, revision: 0 }));
			let info = call.get_dispatch_info();
			let len = call.encoded_size();

			extension
				.test_run(RawOrigin::None.into(), &call, &info, len, 0, |origin| {
					call.clone().dispatch(origin)
				})?
				.expect("dispatch should succeed");
			Ok(())
		};
		let frozen = TransactionValidityError::Invalid(InvalidTransaction::Future);

		assert_ok!(try_bestow(1));

		// Past CallMortality, still inside PointFreezeDuration: `call_valid_from = now` is fresh,
		// so only the freeze check can fire.
		set_time(CallMortality::get() + 1);
		assert_err!(try_bestow(2), frozen);

		// One second before the boundary: still frozen.
		set_time(PointFreezeDuration::get() - 1);
		assert_err!(try_bestow(2), frozen);

		// At the boundary: thawed (`is_point_frozen` uses strict `>`).
		set_time(PointFreezeDuration::get());
		assert_ok!(try_bestow(2));
	});
}

/// The ring proof commits to the `subject` context — re-using a proof with a different subject
/// must fail verification.
#[test]
fn validate_vote_rejects_subject_mismatch() {
	new_test_ext().execute_with(|| {
		let message = &[0, 1, 2, 3];
		let (_, vote_a) = make_vote(0, 0, subject(1), Direction::Honourable);
		let (_, vote_b) = make_vote(0, 0, subject(2), Direction::Honourable);
		let proof = prove_vote::<Test>(&vote_a, 0, message);

		assert_err!(
			Pallet::<Test>::validate_vote_proof(&vote_b, message, &proof, 0, 0),
			Error::<Test>::InvalidProof
		);
	});
}

/// The ring proof commits to the `point` context — re-using a proof with a different point must
/// fail verification.
#[test]
fn validate_vote_rejects_point_mismatch() {
	new_test_ext().execute_with(|| {
		let message = &[0, 1, 2, 3];
		let (_, vote_a) = make_vote(0, 0, subject(1), Direction::Honourable);
		let (_, vote_b) = make_vote(0, 1, subject(1), Direction::Honourable);
		let proof = prove_vote::<Test>(&vote_a, 0, message);

		assert_err!(
			Pallet::<Test>::validate_vote_proof(&vote_b, message, &proof, 0, 0),
			Error::<Test>::InvalidProof
		);
	});
}

/// The ring proof is bound to the call via `inherited_implication`, so a proof generated with
/// `call_valid_from = X` must not be reusable with `call_valid_from = Y`.
#[test]
fn tx_extension_rejects_proof_with_tampered_call_valid_from() {
	new_test_ext().execute_with(|| {
		let now = PointFreezeDuration::get() + 1;
		set_time(now);
		let vote = VoteData { subject: subject(1), point: 0, direction: Direction::Honourable };

		// Proof is generated for `call_valid_from = now`.
		let proof_call = Call::bestow { vote: vote.clone(), call_valid_from: now };
		let proof_call: <Test as frame_system::Config>::RuntimeCall = proof_call.into();
		let ext_version: ExtensionVersion = 0;
		let message = (ext_version, &proof_call).using_encoded(blake2_256);
		let proof = prove_vote::<Test>(&vote, 0, &message);

		// Submit with a different `call_valid_from` (still inside the mortality window).
		let submit_call = Call::bestow { vote: vote.clone(), call_valid_from: now - 1 };
		let submit_call: <Test as frame_system::Config>::RuntimeCall = submit_call.into();
		let extension: VoterAuth<Test> =
			VoterAuth::new(Some(VoterAuthData { proof, ring_index: 0, revision: 0 }));
		let info = submit_call.get_dispatch_info();
		let len = submit_call.encoded_size();
		let post_info = PostDispatchInfo::default();

		assert_err!(
			extension
				.test_run(RawOrigin::None.into(), &submit_call, &info, len, 0, |_| Ok(post_info)),
			TransactionValidityError::Invalid(InvalidTransaction::BadProof)
		);
	});
}

/// `direction` is part of the call (not a proof context), but the proof's message commitment
/// covers the call. Tampering with `direction` after proof generation must fail verification.
#[test]
fn tx_extension_rejects_proof_with_tampered_direction() {
	new_test_ext().execute_with(|| {
		set_time(0);

		let now = 0;
		let vote_a = VoteData { subject: subject(1), point: 0, direction: Direction::Honourable };
		let vote_b = VoteData { direction: Direction::Dishonourable, ..vote_a.clone() };

		let proof_call = Call::bestow { vote: vote_a.clone(), call_valid_from: now };
		let proof_call: <Test as frame_system::Config>::RuntimeCall = proof_call.into();
		let ext_version: ExtensionVersion = 0;
		let message = (ext_version, &proof_call).using_encoded(blake2_256);
		let proof = prove_vote::<Test>(&vote_a, 0, &message);

		let submit_call = Call::bestow { vote: vote_b, call_valid_from: now };
		let submit_call: <Test as frame_system::Config>::RuntimeCall = submit_call.into();
		let extension: VoterAuth<Test> =
			VoterAuth::new(Some(VoterAuthData { proof, ring_index: 0, revision: 0 }));
		let info = submit_call.get_dispatch_info();
		let len = submit_call.encoded_size();
		let post_info = PostDispatchInfo::default();

		assert_err!(
			extension
				.test_run(RawOrigin::None.into(), &submit_call, &info, len, 0, |_| Ok(post_info)),
			TransactionValidityError::Invalid(InvalidTransaction::BadProof)
		);
	});
}

/// Aliases are derived per voter — distinct voters share neither subject_aliases nor
/// point_aliases. Two voters can therefore both bestow on the same subject using their own
/// point 0, and freezing one voter's point does not affect the other.
#[test]
fn aliases_are_per_voter() {
	new_test_ext().execute_with(|| {
		const A: u8 = 1;
		const B: u8 = 2;

		let (origin_a, vote_a) = make_vote(A, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin_a, vote_a, 0));

		// Voter B bestows on the same subject with their own point 0 — succeeds because
		// their `subject_alias` and `point_alias` differ from A's.
		let (origin_b, vote_b) = make_vote(B, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin_b, vote_b, 0));

		// Both contributions counted.
		assert_eq!(read_score(1), 1);

		// Point aliases are distinct.
		assert_ne!(get_point_alias(A, 0), get_point_alias(B, 0));
	});
}

/// `Tally` entries are reaped when a subject's score returns to the default, so that subjects
/// with no net effect do not occupy storage.
#[test]
fn tally_is_reaped_when_returning_to_default() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;

		assert!(!Tally::<Test>::contains_key(subject(1)));

		// Honourable vote: subject(1) goes from default (-1) to 0; entry is stored.
		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(Tally::<Test>::get(subject(1)), Some(0));

		// Redirect to subject(2): subject(1) returns to -1 and its entry is removed.
		let (origin, vote) = make_vote(VOTER, 0, subject(2), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert!(!Tally::<Test>::contains_key(subject(1)));
		assert_eq!(read_score(1), SUBJECT_DEFAULT_SCORE);
		assert_eq!(Tally::<Test>::get(subject(2)), Some(0));

		// Same on the Dishonourable side: subject(3) stored at -2, reaped when redirected away.
		let (origin, vote) = make_vote(VOTER, 1, subject(3), Direction::Dishonourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(Tally::<Test>::get(subject(3)), Some(-2));

		let (origin, vote) = make_vote(VOTER, 1, subject(4), Direction::Dishonourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert!(!Tally::<Test>::contains_key(subject(3)));
		assert_eq!(read_score(3), SUBJECT_DEFAULT_SCORE);
	});
}

const FROZEN: TransactionValidityError =
	TransactionValidityError::Invalid(InvalidTransaction::Future);
const STALE: TransactionValidityError =
	TransactionValidityError::Invalid(InvalidTransaction::Stale);

/// The message that [`VoterAuth`] expects the proof to commit to for `call`.
fn message_for(call: &RuntimeCall) -> [u8; 32] {
	let ext_version: ExtensionVersion = 0;
	(ext_version, call).using_encoded(blake2_256)
}

/// Run `call` through the [`VoterAuth`] extension with `proof` for ring 0 at revision 0.
fn run_with_proof(
	call: RuntimeCall,
	proof: RingProofOf<Test>,
) -> Result<(), TransactionValidityError> {
	run_with_auth(call, VoterAuthData { proof, ring_index: 0, revision: 0 })
}

/// Run `call` through the [`VoterAuth`] extension with `auth`, then dispatch it.
///
/// Panics if the extension accepts the transaction and the dispatch fails.
fn run_with_auth(
	call: RuntimeCall,
	auth: VoterAuthData<Test>,
) -> Result<(), TransactionValidityError> {
	let extension = VoterAuth::<Test>::new(Some(auth));
	let info = call.get_dispatch_info();
	let len = call.encoded_size();

	extension
		.test_run(RawOrigin::None.into(), &call, &info, len, 0, |origin| {
			call.clone().dispatch(origin)
		})?
		.expect("dispatch must succeed for a transaction that the extension accepts");
	Ok(())
}

/// Submit a `bestow` transaction that is valid from the current time.
fn try_bestow(
	voter: u8,
	point: PointId,
	subject_n: u8,
	direction: Direction,
) -> Result<(), TransactionValidityError> {
	let now = <Test as Config>::Clock::now().as_secs();
	let vote = VoteData { subject: subject(subject_n), point, direction };
	let call: RuntimeCall = Call::bestow { vote: vote.clone(), call_valid_from: now }.into();
	let proof = prove_vote::<Test>(&vote, voter, &message_for(&call));

	run_with_proof(call, proof)
}

/// Submit a `clear` transaction that is valid from the current time.
fn try_clear(voter: u8, point: PointId) -> Result<(), TransactionValidityError> {
	try_clear_valid_from(voter, point, <Test as Config>::Clock::now().as_secs())
}

fn try_clear_valid_from(
	voter: u8,
	point: PointId,
	call_valid_from: Seconds,
) -> Result<(), TransactionValidityError> {
	let call: RuntimeCall = Call::clear { point, call_valid_from }.into();
	let proof = prove_contexts::<Test>(&[context::point(point)], voter, &message_for(&call));

	run_with_proof(call, proof)
}

/// A voter changes the direction of a vote with the point that holds it, after the freeze ends.
#[test]
fn tx_extension_allows_direction_switch() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		set_time(0);

		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Honourable));
		assert_eq!(read_score(1), 0);

		set_time(PointFreezeDuration::get() - 1);
		assert_err!(try_bestow(VOTER, 0, 1, Direction::Dishonourable), FROZEN);

		set_time(PointFreezeDuration::get());
		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Dishonourable));
		assert_eq!(read_score(1), -2);

		set_time(2 * PointFreezeDuration::get());
		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Honourable));
		assert_eq!(read_score(1), 0);
	});
}

/// A vote for the same subject in the same direction changes nothing, so the extension rejects it
/// even after the freeze ends.
#[test]
fn tx_extension_rejects_revote_with_same_direction() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		set_time(0);

		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Honourable));

		// The vote is stale, not frozen, while the freeze lasts.
		assert_err!(try_bestow(VOTER, 0, 1, Direction::Honourable), STALE);

		set_time(PointFreezeDuration::get());
		assert_err!(try_bestow(VOTER, 0, 1, Direction::Honourable), STALE);
		assert_eq!(read_score(1), 0);
	});
}

/// A voter cannot direct a second point to a subject, in any direction.
#[test]
fn tx_extension_rejects_second_point_for_same_subject() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		set_time(0);

		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Honourable));

		assert_err!(try_bestow(VOTER, 1, 1, Direction::Honourable), STALE);
		assert_err!(try_bestow(VOTER, 1, 1, Direction::Dishonourable), STALE);
		assert_eq!(read_score(1), 0);

		// The vote is stale also when the second point is frozen.
		assert_ok!(try_bestow(VOTER, 1, 2, Direction::Honourable));
		assert_err!(try_bestow(VOTER, 1, 1, Direction::Honourable), STALE);
	});
}

/// A cleared point is like a second point for a subject that another point holds.
#[test]
fn tx_extension_rejects_cleared_point_for_voted_subject() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		set_time(0);

		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Honourable));
		assert_ok!(try_bestow(VOTER, 1, 2, Direction::Honourable));

		set_time(PointFreezeDuration::get());
		assert_ok!(try_clear(VOTER, 1));

		set_time(2 * PointFreezeDuration::get());
		assert_err!(try_bestow(VOTER, 1, 1, Direction::Dishonourable), STALE);
		assert_ok!(try_bestow(VOTER, 1, 3, Direction::Dishonourable));
	});
}

#[test]
fn clear_works() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		const VOTER: u8 = 0;
		const POINT: PointId = 0;
		let point_alias = get_point_alias(VOTER, POINT);

		set_time(5);
		let (origin, vote) = make_vote(VOTER, POINT, subject(1), Direction::Dishonourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		assert_eq!(read_score(1), -2);

		set_time(9);
		System::reset_events();
		let origin = crate::Origin::PointHolder { point_alias };
		assert_ok!(HonourPallet::<Test>::clear(origin.into(), POINT, 0));

		assert_eq!(read_score(1), SUBJECT_DEFAULT_SCORE);
		assert!(!Tally::<Test>::contains_key(subject(1)));
		assert_eq!(Votes::<Test>::iter().count(), 0);
		assert_eq!(Points::<Test>::get(point_alias), Some(PointInfo::Cleared { cleared_at: 9 }));

		System::assert_has_event(
			Event::<Test>::VoteCleared { subject: subject(1), direction: Direction::Dishonourable }
				.into(),
		);
		System::assert_has_event(
			Event::<Test>::HonourChanged { subject: subject(1), old_value: -2, new_value: -1 }
				.into(),
		);
	});
}

#[test]
fn clear_fails_for_undirected_point() {
	new_test_ext().execute_with(|| {
		let origin = crate::Origin::PointHolder { point_alias: get_point_alias(0, 0) };

		assert_noop!(
			HonourPallet::<Test>::clear(origin.into(), 0, 0),
			Error::<Test>::PointNotDirected
		);
	});
}

#[test]
fn clear_fails_for_cleared_point() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		let origin = crate::Origin::PointHolder { point_alias: get_point_alias(VOTER, 0) };
		assert_ok!(HonourPallet::<Test>::clear(origin.clone().into(), 0, 0));

		assert_noop!(
			HonourPallet::<Test>::clear(origin.into(), 0, 0),
			Error::<Test>::PointNotDirected
		);
	});
}

/// A vote with a cleared point is a new vote. It does not touch the subject of the withdrawn vote.
#[test]
fn bestow_with_cleared_point_casts_new_vote() {
	new_test_ext().execute_with(|| {
		System::set_block_number(1);
		const VOTER: u8 = 0;
		let (origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));
		let origin = crate::Origin::PointHolder { point_alias: get_point_alias(VOTER, 0) };
		assert_ok!(HonourPallet::<Test>::clear(origin.into(), 0, 0));

		System::reset_events();
		let (origin, vote) = make_vote(VOTER, 0, subject(2), Direction::Dishonourable);
		assert_ok!(HonourPallet::<Test>::bestow(origin, vote, 0));

		assert_eq!(read_score(1), SUBJECT_DEFAULT_SCORE);
		assert!(!Tally::<Test>::contains_key(subject(1)));
		assert_eq!(read_score(2), -2);
		assert_eq!(Votes::<Test>::iter().count(), 1);
		assert_eq!(
			System::events().into_iter().map(|record| record.event).collect::<Vec<_>>(),
			vec![
				Event::<Test>::VoteCast {
					subject: subject(2),
					direction: Direction::Dishonourable
				}
				.into(),
				Event::<Test>::HonourChanged { subject: subject(2), old_value: -1, new_value: -2 }
					.into(),
			]
		);
	});
}

/// Each call accepts only the origin that the extension creates for it.
#[test]
fn calls_reject_origin_of_other_call() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;

		let (voter_origin, vote) = make_vote(VOTER, 0, subject(1), Direction::Honourable);
		assert_ok!(HonourPallet::<Test>::bestow(voter_origin.clone(), vote.clone(), 0));

		assert_noop!(HonourPallet::<Test>::clear(voter_origin, 0, 0), BadOrigin);
		assert_noop!(HonourPallet::<Test>::clear(RawOrigin::Signed(1).into(), 0, 0), BadOrigin);

		let holder_origin = crate::Origin::PointHolder { point_alias: get_point_alias(VOTER, 0) };
		assert_noop!(HonourPallet::<Test>::bestow(holder_origin.into(), vote, 0), BadOrigin);
	});
}

/// `clear` waits for the freeze of the point and then freezes the point again.
#[test]
fn tx_extension_rate_limits_clear() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		const POINT: PointId = 0;
		let point_alias = get_point_alias(VOTER, POINT);
		set_time(0);

		assert_ok!(try_bestow(VOTER, POINT, 1, Direction::Honourable));
		assert_err!(try_clear(VOTER, POINT), FROZEN);

		set_time(PointFreezeDuration::get() - 1);
		assert_err!(try_clear(VOTER, POINT), FROZEN);

		set_time(PointFreezeDuration::get());
		assert_ok!(try_clear(VOTER, POINT));
		assert_eq!(read_score(1), SUBJECT_DEFAULT_SCORE);

		// The point holds no vote, so a second `clear` has nothing to do.
		assert_err!(try_clear(VOTER, POINT), STALE);

		assert_err!(try_bestow(VOTER, POINT, 2, Direction::Honourable), FROZEN);
		set_time(2 * PointFreezeDuration::get() - 1);
		assert_err!(try_bestow(VOTER, POINT, 2, Direction::Honourable), FROZEN);

		set_time(2 * PointFreezeDuration::get());
		assert_ok!(try_bestow(VOTER, POINT, 2, Direction::Honourable));
		assert_eq!(read_score(2), 0);
		assert!(matches!(Points::<Test>::get(point_alias), Some(PointInfo::Active { .. })));
	});
}

#[test]
fn tx_extension_rejects_clear_of_unused_point() {
	new_test_ext().execute_with(|| {
		set_time(0);

		assert_err!(try_clear(0, 0), STALE);
	});
}

/// After a voter clears a point, another point of the voter can vote for the same subject.
#[test]
fn clear_releases_subject_for_another_point() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		set_time(0);

		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Honourable));

		set_time(PointFreezeDuration::get());
		assert_err!(try_bestow(VOTER, 1, 1, Direction::Dishonourable), STALE);
		assert_ok!(try_clear(VOTER, 0));
		assert_ok!(try_bestow(VOTER, 1, 1, Direction::Dishonourable));
		assert_eq!(read_score(1), -2);
	});
}

/// The proof of a `clear` commits to the point context only. A proof for another point or for the
/// two contexts of a vote does not authorize the call.
#[test]
fn tx_extension_binds_clear_proof_to_point() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		set_time(0);

		assert_ok!(try_bestow(VOTER, 0, 1, Direction::Honourable));
		assert_ok!(try_bestow(VOTER, 1, 2, Direction::Honourable));

		let now = PointFreezeDuration::get();
		set_time(now);
		let bad_proof = TransactionValidityError::Invalid(InvalidTransaction::BadProof);
		let call: RuntimeCall = Call::clear { point: 1, call_valid_from: now }.into();
		let message = message_for(&call);

		let other_point = prove_contexts::<Test>(&[context::point(0)], VOTER, &message);
		assert_err!(run_with_proof(call.clone(), other_point), bad_proof);

		let vote = VoteData { subject: subject(2), point: 1, direction: Direction::Honourable };
		let vote_contexts = prove_vote::<Test>(&vote, VOTER, &message);
		assert_err!(run_with_proof(call.clone(), vote_contexts), bad_proof);

		let point = prove_contexts::<Test>(&[context::point(1)], VOTER, &message);
		assert_ok!(run_with_proof(call, point));
	});
}

/// The proof of a `bestow` commits to the subject context and the point context. A proof for the
/// point context alone does not authorize the call.
#[test]
fn tx_extension_rejects_point_proof_for_bestow() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		set_time(0);
		let vote = VoteData { subject: subject(1), point: 0, direction: Direction::Honourable };
		let call: RuntimeCall = Call::bestow { vote: vote.clone(), call_valid_from: 0 }.into();
		let message = message_for(&call);

		let point_only = prove_contexts::<Test>(&[context::point(0)], VOTER, &message);
		assert_err!(
			run_with_proof(call.clone(), point_only),
			TransactionValidityError::Invalid(InvalidTransaction::BadProof)
		);

		assert_ok!(run_with_proof(call, prove_vote::<Test>(&vote, VOTER, &message)));
	});
}

/// A `clear` is valid from its `call_valid_from` for `CallMortality` seconds.
#[test]
fn tx_extension_checks_call_validity_time_of_clear() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		const POINT: PointId = 0;
		set_time(0);
		assert_ok!(try_bestow(VOTER, POINT, 1, Direction::Honourable));

		let now = PointFreezeDuration::get() + CallMortality::get();
		set_time(now);

		assert_err!(
			try_clear_valid_from(VOTER, POINT, now + 1),
			TransactionValidityError::Invalid(InvalidTransaction::Future)
		);
		assert_err!(try_clear_valid_from(VOTER, POINT, now - CallMortality::get()), STALE);
		assert!(matches!(
			Points::<Test>::get(get_point_alias(VOTER, POINT)),
			Some(PointInfo::Active { .. })
		));

		assert_ok!(try_clear_valid_from(VOTER, POINT, now - CallMortality::get() + 1));
	});
}

/// The extension verifies the proof of a `clear` against the ring and revision of the transaction.
#[test]
fn tx_extension_rejects_clear_with_unknown_ring_or_revision() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		const POINT: PointId = 0;
		set_time(0);
		assert_ok!(try_bestow(VOTER, POINT, 1, Direction::Honourable));

		let now = PointFreezeDuration::get();
		set_time(now);
		let bad_proof = TransactionValidityError::Invalid(InvalidTransaction::BadProof);
		let call: RuntimeCall = Call::clear { point: POINT, call_valid_from: now }.into();
		let proof = prove_contexts::<Test>(&[context::point(POINT)], VOTER, &message_for(&call));

		// The mock has only ring 0 at revision 0.
		let unknown_ring = VoterAuthData { proof: proof.clone(), ring_index: 1, revision: 0 };
		assert_err!(run_with_auth(call.clone(), unknown_ring), bad_proof);
		let unknown_revision = VoterAuthData { proof: proof.clone(), ring_index: 0, revision: 1 };
		assert_err!(run_with_auth(call.clone(), unknown_revision), bad_proof);

		assert_ok!(run_with_proof(call, proof));
	});
}

/// `bestow` and `clear` provide the same tag for a point, so the pool keeps one of them.
#[test]
fn tx_extension_tags_point_for_bestow_and_clear() {
	new_test_ext().execute_with(|| {
		const VOTER: u8 = 0;
		const POINT: PointId = 0;
		set_time(0);

		assert_ok!(try_bestow(VOTER, POINT, 1, Direction::Honourable));

		let now = PointFreezeDuration::get();
		set_time(now);

		let provides = |call: RuntimeCall, proof: RingProofOf<Test>| {
			let extension =
				VoterAuth::<Test>::new(Some(VoterAuthData { proof, ring_index: 0, revision: 0 }));
			let info = call.get_dispatch_info();
			let len = call.encoded_size();
			let (validity, _, _) = extension
				.validate_only(
					RawOrigin::None.into(),
					&call,
					&info,
					len,
					TransactionSource::External,
					0,
				)
				.expect("the transaction is valid");
			validity.provides
		};

		let vote = VoteData { subject: subject(2), point: POINT, direction: Direction::Honourable };
		let bestow: RuntimeCall = Call::bestow { vote: vote.clone(), call_valid_from: now }.into();
		let bestow_proof = prove_vote::<Test>(&vote, VOTER, &message_for(&bestow));

		let clear: RuntimeCall = Call::clear { point: POINT, call_valid_from: now }.into();
		let clear_proof =
			prove_contexts::<Test>(&[context::point(POINT)], VOTER, &message_for(&clear));

		let bestow_tags = provides(bestow, bestow_proof);
		assert_eq!(bestow_tags.len(), 1);
		assert_eq!(bestow_tags, provides(clear, clear_proof));
	});
}

#[test]
fn tx_extension_charges_weight_of_call() {
	let bestow: RuntimeCall = Call::bestow {
		vote: VoteData { subject: subject(1), point: 0, direction: Direction::Honourable },
		call_valid_from: 0,
	}
	.into();
	let clear: RuntimeCall = Call::clear { point: 0, call_valid_from: 0 }.into();
	let extension = VoterAuth::<Test>::new(Some(VoterAuthData {
		proof: Default::default(),
		ring_index: 0,
		revision: 0,
	}));

	let bestow_weight = <() as WeightInfo>::extension_validate();
	let clear_weight = <() as WeightInfo>::extension_validate_clear();
	assert_ne!(bestow_weight, clear_weight);

	assert_eq!(extension.weight(&bestow), bestow_weight);
	assert_eq!(extension.weight(&clear), clear_weight);
	assert_eq!(VoterAuth::<Test>::new(None).weight(&clear), Weight::zero());
}

mod migration {
	use super::*;
	use crate::migration::v1::{v0, MigratePointsToActive};
	use frame_support::{
		migrations::{SteppedMigration, SteppedMigrationError},
		traits::{GetStorageVersion, StorageVersion},
		weights::WeightMeter,
	};

	/// The number of version 0 points that [`set_up_v0`] seeds.
	const V0_POINTS: u8 = 3;

	fn v0_point_alias(i: u8) -> PointAlias {
		[10 + i; 32]
	}

	/// A version 0 point.
	///
	/// The subject does not start with the index of a [`PointInfo`] variant, so the point does not
	/// decode in the current layout. The subject alias ends with the index of a [`Direction`]
	/// variant, so the migrated point decodes in the version 0 layout. A migration that reads a
	/// migrated point again then stores a wrong value.
	fn v0_point(i: u8) -> v0::PointInfo {
		let mut subject_alias = [30 + i; 32];
		subject_alias[31] = i % 2;
		v0::PointInfo {
			subject: [20 + i; 32],
			subject_alias,
			direction: if i.is_multiple_of(2) {
				Direction::Honourable
			} else {
				Direction::Dishonourable
			},
			last_used_at: 100 + Seconds::from(i),
		}
	}

	fn set_up_v0() {
		for i in 0..V0_POINTS {
			v0::Points::<Test>::insert(v0_point_alias(i), v0_point(i));
		}
		StorageVersion::new(0).put::<HonourPallet<Test>>();
		assert_eq!(migrated_points(), 0);
	}

	/// The active point that the migration must store for [`v0_point`].
	fn active_point(i: u8) -> PointInfo {
		let point = v0_point(i);
		PointInfo::Active {
			subject: point.subject,
			subject_alias: point.subject_alias,
			direction: point.direction,
			last_used_at: point.last_used_at,
		}
	}

	/// The seeded points that the current layout reads as active points with their old fields.
	fn migrated_points() -> usize {
		(0..V0_POINTS)
			.filter(|i| Points::<Test>::get(v0_point_alias(*i)) == Some(active_point(*i)))
			.count()
	}

	fn step_weight() -> Weight {
		<() as WeightInfo>::migrate_v1_step()
	}

	#[test]
	fn migration_stores_points_as_active_points() {
		new_test_ext().execute_with(|| {
			set_up_v0();
			let mut meter = WeightMeter::new();

			assert_eq!(MigratePointsToActive::<Test>::step(None, &mut meter), Ok(None));

			assert_eq!(HonourPallet::<Test>::on_chain_storage_version(), 1);
			assert_eq!(Points::<Test>::iter().count(), usize::from(V0_POINTS));
			assert_eq!(
				Points::<Test>::get(v0_point_alias(1)),
				Some(PointInfo::Active {
					subject: [21; 32],
					subject_alias: v0_point(1).subject_alias,
					direction: Direction::Dishonourable,
					last_used_at: 101,
				})
			);
			assert_eq!(migrated_points(), usize::from(V0_POINTS));
		});
	}

	/// A vote of version 0 keeps its freeze and its voter can clear it after the migration.
	#[test]
	fn voter_can_clear_a_migrated_point() {
		new_test_ext().execute_with(|| {
			const VOTER: u8 = 0;
			const POINT: PointId = 0;
			let (_, vote) = make_vote(VOTER, POINT, subject(1), Direction::Honourable);
			let subject_alias = TestCrypto::alias_in_context(
				&voter_secret_key(VOTER),
				&context::subject(&vote.subject),
			)
			.expect("Failed to create subject alias");
			v0::Points::<Test>::insert(
				get_point_alias(VOTER, POINT),
				v0::PointInfo {
					subject: vote.subject,
					subject_alias,
					direction: vote.direction,
					last_used_at: 0,
				},
			);
			Votes::<Test>::insert(subject_alias, ());
			Tally::<Test>::insert(vote.subject, 0);
			StorageVersion::new(0).put::<HonourPallet<Test>>();

			let mut meter = WeightMeter::new();
			assert_eq!(MigratePointsToActive::<Test>::step(None, &mut meter), Ok(None));

			set_time(PointFreezeDuration::get() - 1);
			assert_err!(try_clear(VOTER, POINT), FROZEN);

			set_time(PointFreezeDuration::get());
			assert_ok!(try_clear(VOTER, POINT));
			assert_eq!(read_score(1), SUBJECT_DEFAULT_SCORE);
			assert_eq!(Votes::<Test>::iter().count(), 0);
		});
	}

	#[test]
	fn migration_step_weight_covers_one_point() {
		new_test_ext().execute_with(|| {
			set_up_v0();

			let mut cursor = None;
			for migrated in 1..=usize::from(V0_POINTS) {
				let mut meter = WeightMeter::with_limit(step_weight());
				cursor = MigratePointsToActive::<Test>::step(cursor, &mut meter).unwrap();
				assert!(cursor.is_some());
				assert_eq!(migrated_points(), migrated);
			}
			assert_eq!(HonourPallet::<Test>::on_chain_storage_version(), 0);

			let mut meter = WeightMeter::with_limit(step_weight());
			assert_eq!(MigratePointsToActive::<Test>::step(cursor, &mut meter), Ok(None));
			assert_eq!(HonourPallet::<Test>::on_chain_storage_version(), 1);
			assert_eq!(migrated_points(), usize::from(V0_POINTS));
		});
	}

	#[test]
	fn migration_step_requires_the_weight_of_one_point() {
		new_test_ext().execute_with(|| {
			set_up_v0();
			let required = step_weight();
			let mut meter =
				WeightMeter::with_limit(required.saturating_sub(Weight::from_parts(1, 0)));

			assert_eq!(
				MigratePointsToActive::<Test>::step(None, &mut meter),
				Err(SteppedMigrationError::InsufficientWeight { required })
			);

			assert_eq!(migrated_points(), 0);
		});
	}

	#[test]
	fn migration_completes_on_empty_map() {
		new_test_ext().execute_with(|| {
			StorageVersion::new(0).put::<HonourPallet<Test>>();
			let mut meter = WeightMeter::with_limit(step_weight());

			assert_eq!(MigratePointsToActive::<Test>::step(None, &mut meter), Ok(None));

			assert_eq!(HonourPallet::<Test>::on_chain_storage_version(), 1);
		});
	}

	#[test]
	fn migration_skips_a_chain_past_version_0() {
		new_test_ext().execute_with(|| {
			set_up_v0();
			StorageVersion::new(1).put::<HonourPallet<Test>>();
			let mut meter = WeightMeter::with_limit(step_weight());

			assert_eq!(MigratePointsToActive::<Test>::step(None, &mut meter), Ok(None));

			assert_eq!(meter.consumed(), Weight::zero());
			assert_eq!(migrated_points(), 0);
		});
	}

	/// Run the migration on seeded version 0 state and return the pre-upgrade state.
	#[cfg(feature = "try-runtime")]
	fn migrate_with_pre_upgrade_state() -> Vec<u8> {
		set_up_v0();
		let state = MigratePointsToActive::<Test>::pre_upgrade().unwrap();
		let mut meter = WeightMeter::new();
		assert_eq!(MigratePointsToActive::<Test>::step(None, &mut meter), Ok(None));
		state
	}

	#[test]
	#[cfg(feature = "try-runtime")]
	fn try_runtime_checks_accept_a_complete_migration() {
		new_test_ext().execute_with(|| {
			let state = migrate_with_pre_upgrade_state();

			assert_ok!(MigratePointsToActive::<Test>::post_upgrade(state));
		});
	}

	#[test]
	#[cfg(feature = "try-runtime")]
	fn try_runtime_checks_reject_a_lost_point() {
		new_test_ext().execute_with(|| {
			let state = migrate_with_pre_upgrade_state();
			Points::<Test>::remove(v0_point_alias(0));

			assert_eq!(
				MigratePointsToActive::<Test>::post_upgrade(state),
				Err("the migration must keep every point".into())
			);
		});
	}

	#[test]
	#[cfg(feature = "try-runtime")]
	fn try_runtime_checks_reject_a_point_that_is_not_migrated() {
		new_test_ext().execute_with(|| {
			let state = migrate_with_pre_upgrade_state();
			v0::Points::<Test>::insert(v0_point_alias(0), v0_point(0));

			assert_eq!(
				MigratePointsToActive::<Test>::post_upgrade(state),
				Err("a point must become an active point".into())
			);
		});
	}

	#[test]
	#[cfg(feature = "try-runtime")]
	fn try_runtime_checks_reject_a_point_with_a_changed_field() {
		for changed in 0..4 {
			new_test_ext().execute_with(|| {
				let state = migrate_with_pre_upgrade_state();
				let PointInfo::Active { subject, subject_alias, direction, last_used_at } =
					active_point(0)
				else {
					unreachable!("`active_point` returns an active point")
				};
				let point = match changed {
					0 => PointInfo::Active {
						subject: subject_alias,
						subject_alias,
						direction,
						last_used_at,
					},
					1 => PointInfo::Active {
						subject,
						subject_alias: subject,
						direction,
						last_used_at,
					},
					2 => PointInfo::Active {
						subject,
						subject_alias,
						direction: direction.opposite(),
						last_used_at,
					},
					_ => PointInfo::Active {
						subject,
						subject_alias,
						direction,
						last_used_at: last_used_at + 1,
					},
				};
				Points::<Test>::insert(v0_point_alias(0), point);

				assert_eq!(
					MigratePointsToActive::<Test>::post_upgrade(state),
					Err("an active point must keep the fields of the version 0 point".into())
				);
			});
		}
	}

	#[test]
	#[cfg(feature = "try-runtime")]
	fn try_runtime_checks_reject_an_old_storage_version() {
		new_test_ext().execute_with(|| {
			let state = migrate_with_pre_upgrade_state();
			StorageVersion::new(0).put::<HonourPallet<Test>>();

			assert_eq!(
				MigratePointsToActive::<Test>::post_upgrade(state),
				Err("the storage version must be 1 after the migration".into())
			);
		});
	}

	/// The checks have nothing to compare on a chain where the migration does nothing.
	#[test]
	#[cfg(feature = "try-runtime")]
	fn try_runtime_checks_skip_a_chain_past_version_0() {
		new_test_ext().execute_with(|| {
			set_up_v0();
			StorageVersion::new(2).put::<HonourPallet<Test>>();
			let state = MigratePointsToActive::<Test>::pre_upgrade().unwrap();

			assert_ok!(MigratePointsToActive::<Test>::post_upgrade(state));
		});
	}
}
