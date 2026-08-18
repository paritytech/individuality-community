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

use super::*;
use frame_support::assert_ok;

fn register_lite_person(
	attester: &AccountId32,
	lite_pair: &sr25519::Pair,
) -> <Crypto as verifiable::GenerateVerifiable>::Secret {
	let lite_account = pair_to_account_id(lite_pair);
	let ring_secret = Crypto::new_secret([42u8; 32]);
	let ring_member = Crypto::member_from_secret(&ring_secret);

	let msg = lite_account.using_encoded(|account_bytes| {
		ring_member.using_encoded(|ring_bytes| {
			[&indiv_pallet_people_lite::MSG_PREFIX[..], account_bytes, ring_bytes].concat()
		})
	});

	let candidate_signature = MultiSignature::from(lite_pair.sign(&msg));
	let proof =
		Crypto::sign(&ring_secret, &msg).expect("ring key can sign the attestation payload");

	assert_ok!(indiv_pallet_people_lite::Pallet::<Runtime>::attest(
		RuntimeOrigin::signed(attester.clone()),
		lite_account,
		candidate_signature,
		ring_member,
		proof,
		None,
	));

	ring_secret
}

fn build_lite_person_signed_ext(who: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
	build_people_lite_auth_ext(
		who,
		indiv_pallet_people_lite::PeopleLiteAuthData::AsLitePerson,
		call,
	)
}

#[test]
fn lite_people_free_transaction_updates_usage() {
	new_test_ext().execute_with(|| {
		let attester = Sr25519Keyring::Bob.to_account_id();
		assert_ok!(indiv_pallet_people_lite::Pallet::<Runtime>::increase_attestation_allowance(
			RuntimeOrigin::root(),
			attester.clone(),
			1,
		));

		let lite_pair = sr25519::Pair::from_seed(&[77u8; 32]);
		let lite_account = pair_to_account_id(&lite_pair);
		assert_eq!(Balances::free_balance(lite_account.clone()), 0);

		register_lite_person(&attester, &lite_pair);
		assert!(
			indiv_pallet_people_lite::LitePeople::<Runtime>::contains_key(&lite_account),
			"lite person must be registered before running the free transaction"
		);

		let nested_call = RuntimeCall::System(frame_system::Call::<Runtime>::remark_with_event {
			remark: b"lite people free tx".to_vec(),
		});

		let outer_call = RuntimeCall::PeopleLite(
			indiv_pallet_people_lite::Call::<Runtime>::dispatch_as_signer {
				call: Box::new(nested_call),
			},
		);

		let entity = crate::RestrictedEntity::LitePerson(lite_account.clone());
		assert!(
			indiv_pallet_origin_restriction::Usages::<Runtime>::get(entity.clone()).is_none(),
			"usage starts empty for lite person"
		);

		let uxt = build_lite_person_signed_ext(&lite_pair, outer_call);
		Executive::apply_extrinsic(uxt)
			.expect("lite person transaction is valid")
			.expect("lite person dispatch succeeds");
		assert_eq!(Balances::free_balance(lite_account.clone()), 0);

		let remarked = frame_system::Pallet::<Runtime>::events().iter().any(|rec| {
			matches!(
				rec.event,
				RuntimeEvent::System(frame_system::Event::Remarked { ref sender, .. })
					if *sender == lite_account
			)
		});
		assert!(remarked, "lite person remark_with_event must emit the System::Remarked event");

		let usage = indiv_pallet_origin_restriction::Usages::<Runtime>::get(entity)
			.expect("usage should be tracked for lite person");
		assert!(
			usage.used > 0,
			"usage must be accounted even when the transaction is otherwise free"
		);
	});
}
