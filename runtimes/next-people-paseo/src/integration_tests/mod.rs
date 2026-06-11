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

//! Integration-style tests for the next-people-paseo runtime.

use crate::{
	people::{ExternalAssetLocation, FungibleExternalAsset, GameAirdropSource, PlayDepositDefault},
	Address, Balances, Executive, Runtime, RuntimeCall, TransactionExtension, UncheckedExtrinsic,
	*,
};
use codec::Encode;
use frame_support::{
	traits::{
		fungible::{Inspect, InspectHold, Mutate},
		Get as _, Hooks, OffchainWorker, OnIdle,
	},
	weights::{Weight, WeightMeter},
};
use indiv_pallet_airdrop::{
	context_for_event,
	types::{AirdropPrize, RegistrationEntry, Status},
	vrf::transcript_for_event,
};
use indiv_pallet_game::{AirdropVrf, GameTimes};
use indiv_pallet_score::{AccountOrPerson, SCORE_CONTEXT};
use indiv_support::traits::{
	Alias, AppendOnlyMembers as _, Callback, Judgement, JudgementContext, RevisionIndex, RingIndex,
	Statement, StatementOracle, Truth,
};
use sp_core::{ed25519, sr25519, Pair as _};
use sp_io::TestExternalities;
use sp_keyring::Sr25519Keyring;
use sp_runtime::{
	generic,
	offchain::{
		testing::{PoolState, TestOffchainExt, TestTransactionPoolExt},
		OffchainDbExt, OffchainWorkerExt, TransactionPoolExt,
	},
	traits::{TransactionExtension as _, Zero},
	AccountId32, BoundedVec, BuildStorage, MultiSignature,
};
use std::{
	cell::{Cell, RefCell},
	sync::Arc,
};
use verifiable::{ring::bandersnatch::BandersnatchVrfVerifiable as Crypto, GenerateVerifiable};

// TODO: bring back key_migration_flow - tests old migration API removed in Members refactor
// mod key_migration_flow;
mod coinage_fee_sanity;
mod coinage_infallible_unpaid_load;
mod coinage_non_anonymous_flow;
mod coinage_paid_flow;
mod coinage_people_flow;
mod coinage_token_allowance;
mod external_asset_teleport;
mod lite_people_free_tx;
mod score_game_deposit_flow;
mod score_game_invitation_flow;
mod score_game_person_flow;
mod statement_allowance;
mod tx_payment_external_asset;
mod value_transfer_auth_key;

type VrfSecret = <Crypto as GenerateVerifiable>::Secret;

fn recycler_ring_exponent() -> indiv_support::traits::RingExponent {
	crate::people::RecyclerRingExponent::get()
}

fn paid_unload_token_ring_exponent() -> indiv_support::traits::RingExponent {
	crate::people::PaidUnloadTokenRingExponent::get()
}

fn ring_domain_size(
	ring_exponent: indiv_support::traits::RingExponent,
) -> verifiable::ring::RingDomainSize {
	ring_exponent.try_into().expect("ring exponent should map to a ring domain")
}

fn recycler_ring_size() -> <Crypto as GenerateVerifiable>::Config {
	ring_domain_size(recycler_ring_exponent())
}

fn paid_unload_token_ring_size() -> <Crypto as GenerateVerifiable>::Config {
	ring_domain_size(paid_unload_token_ring_exponent())
}

// Tests are executed in their own thread and only use one thread. This setups a global variable for
// each test. If we ever need multi-threaded tests, this will need to be reworked.
thread_local! {
	static TRANSACTION_POOL: RefCell<Arc<parking_lot::RwLock<PoolState>>> =
		RefCell::new(Arc::new(parking_lot::RwLock::new(PoolState {
			transactions: Vec::new(),
		})));
	static UNIQUE_SECRET_COUNTER: Cell<u64> = const { Cell::new(10_000) };
}

fn pair_to_account_id(pair: &sr25519::Pair) -> AccountId32 {
	(*pair.public().as_array_ref()).into()
}

fn create_unique_secret() -> VrfSecret {
	let seed = UNIQUE_SECRET_COUNTER.with(|counter| {
		let value = counter.get();
		counter.set(value.checked_add(1).expect("unique secret counter overflowed"));
		sp_io::hashing::twox_256(&(b"integration_unique_secrets", value).encode())
	});
	Crypto::new_secret(seed)
}

fn register_lite_person_for_integration(
	lite_pair: &sr25519::Pair,
) -> <Crypto as verifiable::GenerateVerifiable>::Secret {
	let attester = Sr25519Keyring::Bob.to_account_id();
	let onboarding_size = crate::people::LitePeopleOnboardingSize::get();
	indiv_pallet_people_lite::Pallet::<Runtime>::increase_attestation_allowance(
		RuntimeOrigin::root(),
		attester.clone(),
		onboarding_size,
	)
	.expect("root can grant lite-person attestation allowance");

	let filler_pairs =
		[sr25519::Pair::from_seed(&[201u8; 32]), sr25519::Pair::from_seed(&[202u8; 32])];
	let mut target_secret = None;
	let mut target_member = None;
	for pair in core::iter::once(lite_pair)
		.chain(filler_pairs.iter())
		.take(onboarding_size as usize)
	{
		let lite_account = pair_to_account_id(pair);
		let ring_secret = create_unique_secret();
		let ring_member = Crypto::member_from_secret(&ring_secret);

		let msg = lite_account.using_encoded(|account_bytes| {
			ring_member.using_encoded(|ring_bytes| {
				[&indiv_pallet_people_lite::MSG_PREFIX[..], account_bytes, ring_bytes].concat()
			})
		});

		let candidate_signature = MultiSignature::from(pair.sign(&msg));
		let proof = Crypto::sign(&ring_secret, &msg)
			.expect("ring key can sign the lite attestation payload");

		indiv_pallet_people_lite::Pallet::<Runtime>::attest(
			RuntimeOrigin::signed(attester.clone()),
			lite_account.clone(),
			candidate_signature,
			ring_member,
			proof,
			None,
		)
		.expect("lite person attestation should succeed");

		assert!(
			indiv_pallet_people_lite::LitePeople::<Runtime>::contains_key(&lite_account),
			"lite person must be registered for runtime integration tests"
		);

		if lite_account == pair_to_account_id(lite_pair) {
			target_secret = Some(ring_secret);
			target_member = Some(ring_member);
		}
	}

	let identifier = *indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER;
	let ring_index = indiv_pallet_members::CurrentRingIndex::<Runtime>::get(identifier);
	let (head, _) = indiv_pallet_members::QueuePageIndices::<Runtime>::get(identifier);
	let first_member = indiv_pallet_members::OnboardingQueue::<Runtime>::get(identifier, head)
		.first()
		.cloned();
	Members::onboard_members_authorized(
		frame_system::RawOrigin::Authorized.into(),
		identifier,
		ring_index,
		head,
		first_member,
		0,
	)
	.expect("lite people onboarding should succeed in runtime integration tests");
	let revision = indiv_pallet_members::Root::<Runtime>::get(identifier, ring_index)
		.map(|root| root.revision)
		.unwrap_or_default();
	let ring_exponent = indiv_pallet_members::Collections::<Runtime>::get(identifier)
		.expect("collection must exist")
		.ring_size;
	let to_include = indiv_pallet_members::Pallet::<Runtime>::should_build_ring(
		&identifier,
		ring_index,
		<Runtime as indiv_pallet_members::Config>::RingBuildingMemberLimit::get(),
	)
	.expect("ring should be ready to build in runtime integration tests");
	Members::build_ring_authorized(
		frame_system::RawOrigin::Authorized.into(),
		identifier,
		ring_index,
		ring_exponent,
		Some(revision),
		to_include,
		0,
	)
	.expect("lite people ring build should succeed in runtime integration tests");
	let ring_member = target_member.expect("target lite member must be recorded");
	assert!(
		indiv_pallet_members::Pallet::<Runtime>::member_status(
			indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER,
			&ring_member,
		)
		.and_then(|status| status.ring_index())
		.is_some(),
		"lite person ring member must be included before building friend-request proofs"
	);

	target_secret.expect("target lite secret must be recorded")
}

fn run_people_lite_on_poll() {
	let current = frame_system::Pallet::<Runtime>::block_number();
	let mut wm_people_lite = WeightMeter::with_limit(Weight::MAX);
	indiv_pallet_people_lite::Pallet::<Runtime>::on_poll(current, &mut wm_people_lite);
}

fn new_test_ext() -> TestExternalities {
	install_test_authorization_pubkey_override();
	let alice = Sr25519Keyring::Alice.to_account_id();
	let storage = crate::RuntimeGenesisConfig {
		system: frame_system::GenesisConfig::default(),
		sudo: pallet_sudo::GenesisConfig { key: Some(alice.clone()) },
		balances: pallet_balances::GenesisConfig {
			balances: vec![(alice, 10_000_000_000_000)],
			dev_accounts: None,
		},
		..Default::default()
	}
	.build_storage()
	.expect("runtime genesis storage builds");

	let mut ext: TestExternalities = storage.into();
	let (offchain, _state) = TestOffchainExt::new();
	let (pool, state) = TestTransactionPoolExt::new();
	TRANSACTION_POOL.set(state);
	ext.register_extension(OffchainDbExt::new(offchain.clone()));
	ext.register_extension(OffchainWorkerExt::new(offchain));
	ext.register_extension(TransactionPoolExt::new(pool));

	// Initialize chunks and people collection (replaces old genesis configs).
	ext.execute_with(|| {
		use indiv_support::traits::{RingExponent, RingMode};
		use verifiable::ring::RingDomainSize;

		let chunks = indiv_support::genesis::ring_verifier_builder_params(RingDomainSize::Domain11);
		let page_size = crate::people::ChunkPageSize::get() as usize;
		for (page_index, page_chunks) in chunks.chunks(page_size).enumerate() {
			let page: BoundedVec<_, crate::people::ChunkPageSize> = page_chunks
				.iter()
				.cloned()
				.map(indiv_pallet_chunks_manager::UncheckedChunk)
				.collect::<Vec<_>>()
				.try_into()
				.expect("chunks must fit into page");
			indiv_pallet_chunks_manager::Chunks::<Runtime>::insert(
				RingExponent::R2e9,
				page_index as u32,
				page,
			);
		}

		for ring_exponent in [recycler_ring_exponent(), paid_unload_token_ring_exponent()] {
			let chunks = indiv_support::genesis::ring_verifier_builder_params(ring_domain_size(
				ring_exponent,
			));
			for (page_index, page_chunks) in chunks.chunks(page_size).enumerate() {
				let page: BoundedVec<_, crate::people::ChunkPageSize> = page_chunks
					.iter()
					.cloned()
					.map(indiv_pallet_chunks_manager::UncheckedChunk)
					.collect::<Vec<_>>()
					.try_into()
					.expect("chunks must fit into page");
				indiv_pallet_chunks_manager::Chunks::<Runtime>::insert(
					ring_exponent,
					page_index as u32,
					page,
				);
			}
		}
		Members::create_collection(
			crate::people::PeopleCollectionOwner::get(),
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			1u32,
			RingMode::Flexible,
			RingExponent::R2e9,
			None,
		)
		.expect("create people collection");
		indiv_pallet_people::PeopleCollectionCreated::<Runtime>::put(true);
	});

	ext.execute_with(|| {
		frame_system::Pallet::<Runtime>::set_block_number(1);
		pallet_timestamp::Now::<Runtime>::put(1_000u64);
		run_people_lite_on_poll();
		setup_external_asset();
	});

	ext
}

#[allow(unused)]
#[track_caller]
fn exec_unsigned(call: RuntimeCall) {
	let uxt = UncheckedExtrinsic::new_bare(call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

fn test_authorization_pair() -> ed25519::Pair {
	ed25519::Pair::from_seed(&[0x42u8; 32])
}

fn install_test_authorization_pubkey_override() {
	paseo_runtime_constants::auth_keys::set_value_transfer_authorization_pubkey_override(Some(
		test_authorization_pair().public(),
	));
}

fn finalize_uxt(call: RuntimeCall, mut tx_ext: TransactionExtension) -> UncheckedExtrinsic {
	let rest_ext = (
		(
			tx_ext.0 .0 .1.clone(),
			tx_ext.0 .0 .2.clone(),
			tx_ext.0 .0 .3.clone(),
			tx_ext.0 .0 .4.clone(),
			tx_ext.0 .0 .5.clone(),
			tx_ext.0 .0 .6.clone(),
			tx_ext.0 .0 .7.clone(),
			tx_ext.0 .0 .8.clone(),
			tx_ext.0 .0 .9.clone(),
			tx_ext.0 .0 .10.clone(),
		),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded = (implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded)
	};

	let sig = test_authorization_pair().sign(&msg);
	tx_ext.0 .0 .0 = indiv_pallet_value_transfer_auth::extension::AuthorizeValueTransfer::<
		Runtime,
		paseo_runtime_constants::ValueTransferAuthorizationPubkey,
	>(Some(sig), core::marker::PhantomData);

	UncheckedExtrinsic::new_transaction(call, tx_ext)
}

// Some basic transaction extension to be modified as needed.
fn base_tx_ext(_call: RuntimeCall) -> TransactionExtension {
	(
		(
			indiv_pallet_value_transfer_auth::extension::AuthorizeValueTransfer::<
				Runtime,
				paseo_runtime_constants::ValueTransferAuthorizationPubkey,
			>::default(),
			pallet_verify_signature::VerifySignature::<Runtime>::Disabled,
			indiv_pallet_people::extension::AsPerson::<Runtime>::new(None),
			indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Runtime>::new(None),
			indiv_pallet_score::ScoreAsParticipant::<Runtime>::new(None),
			indiv_pallet_game::GameAsInvited::<Runtime>::new(None),
			indiv_pallet_people_lite::extension::PeopleLiteAuth::<Runtime>::new(None),
			indiv_pallet_members::extension::AsMember::<Runtime>::new(None),
			indiv_pallet_coinage::extension::AsCoinage::<Runtime>::new(None),
			indiv_pallet_resources::extension::AsResources::<Runtime>::new(None),
			indiv_pallet_honour::extension::VoterAuth::<Runtime>::new(None),
			frame_system::AuthorizeCall::<Runtime>::new(),
		),
		indiv_pallet_origin_restriction::RestrictOrigin::<Runtime>::new(true),
		frame_system::CheckNonZeroSender::<Runtime>::new(),
		frame_system::CheckSpecVersion::<Runtime>::new(),
		frame_system::CheckTxVersion::<Runtime>::new(),
		frame_system::CheckGenesis::<Runtime>::new(),
		frame_system::CheckEra::<Runtime>::from(generic::Era::Immortal),
		frame_system::CheckNonce::<Runtime>::from(0),
		frame_system::CheckWeight::<Runtime>::new(),
		pallet_skip_feeless_payment::SkipCheckIfFeeless::<
			Runtime,
			pallet_asset_tx_payment::ChargeAssetTxPayment<Runtime>,
		>::from(pallet_asset_tx_payment::ChargeAssetTxPayment::<Runtime>::from(0u128, None)),
	)
		.into()
}

fn build_as_alias_with_proof_ext(
	who_secret: &VrfSecret,
	context: [u8; 32],
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let rest_ext = (
		(
			tx_ext.0 .0 .3.clone(),
			tx_ext.0 .0 .4.clone(),
			tx_ext.0 .0 .5.clone(),
			tx_ext.0 .0 .6.clone(),
			tx_ext.0 .0 .7.clone(),
			tx_ext.0 .0 .8.clone(),
			tx_ext.0 .0 .9.clone(),
			tx_ext.0 .0 .10.clone(),
		),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded_implications =
			(implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded_implications)
	};

	let member = Crypto::member_from_secret(who_secret);
	let ring_index = indiv_pallet_members::Pallet::<Runtime>::member_status(
		indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
		&member,
	)
	.unwrap()
	.ring_index()
	.unwrap();
	let members = indiv_pallet_members::RingKeys::<Runtime>::get((
		indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
		ring_index,
		0u32,
	));
	let commitment =
		Crypto::open(verifiable::ring::RingDomainSize::Domain11, &member, members.into_iter())
			.unwrap();
	let proof = Crypto::create(commitment, who_secret, &context[..], &msg[..]).unwrap().0;

	tx_ext.0 .0 .2 = indiv_pallet_people::extension::AsPerson::new(Some(
		indiv_pallet_people::extension::AsPersonInfo::AsPersonalAliasWithProof(
			proof, ring_index, context,
		),
	));

	finalize_uxt(call, tx_ext)
}

fn exec_as_alias_with_proof(who_secret: &VrfSecret, context: [u8; 32], call: RuntimeCall) {
	let uxt = build_as_alias_with_proof_ext(who_secret, context, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

fn build_friend_request_registration_ext(
	who_secret: &VrfSecret,
	period: u32,
	seq: u8,
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());
	let context =
		Resources::friend_request_context(indiv_pallet_resources::types::FriendRequestReference {
			period,
			seq,
		});

	let rest_ext = (
		(tx_ext.0 .0 .10.clone(),),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded_implications =
			(implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded_implications)
	};

	let member = Crypto::member_from_secret(who_secret);
	let ring_index = indiv_pallet_members::Pallet::<Runtime>::member_status(
		indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
		&member,
	)
	.unwrap()
	.ring_index()
	.unwrap();
	let members = indiv_pallet_members::RingKeys::<Runtime>::get((
		indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
		ring_index,
		0u32,
	));
	let commitment =
		Crypto::open(verifiable::ring::RingDomainSize::Domain11, &member, members.into_iter())
			.unwrap();
	let proof = Crypto::create(commitment, who_secret, &context[..], &msg[..]).unwrap().0;

	tx_ext.0 .0 .9 = indiv_pallet_resources::extension::AsResources::new(Some(
		indiv_pallet_resources::extension::AsResourcesInfo::RegisterFriendRequestWithProof(
			proof, ring_index,
		),
	));

	finalize_uxt(call, tx_ext)
}

fn build_friend_request_for_collection_ext(
	who_secret: &VrfSecret,
	period: u32,
	seq: u8,
	call: RuntimeCall,
	identifier: &indiv_support::traits::Identifier,
	collection: indiv_pallet_resources::types::MembershipCollection,
) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());
	let context =
		Resources::friend_request_context(indiv_pallet_resources::types::FriendRequestReference {
			period,
			seq,
		});

	let rest_ext = (
		(tx_ext.0 .0 .10.clone(),),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded_implications =
			(implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded_implications)
	};

	let member = Crypto::member_from_secret(who_secret);
	let ring_index = indiv_pallet_members::Pallet::<Runtime>::member_status(identifier, &member)
		.unwrap()
		.ring_index()
		.unwrap();
	let members = indiv_pallet_members::RingKeys::<Runtime>::get((identifier, ring_index, 0u32));
	let commitment =
		Crypto::open(verifiable::ring::RingDomainSize::Domain11, &member, members.into_iter())
			.unwrap();
	let proof = Crypto::create(commitment, who_secret, &context[..], &msg[..]).unwrap().0;

	tx_ext.0 .0 .9 = indiv_pallet_resources::extension::AsResources::new(Some(
		indiv_pallet_resources::extension::AsResourcesInfo::RegisterFriendRequestForCollection(
			proof, ring_index, collection,
		),
	));

	finalize_uxt(call, tx_ext)
}

fn build_lite_friend_request_registration_ext(
	who_secret: &VrfSecret,
	period: u32,
	seq: u8,
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	build_friend_request_for_collection_ext(
		who_secret,
		period,
		seq,
		call,
		indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER,
		indiv_pallet_resources::types::MembershipCollection::LitePeople,
	)
}

fn exec_friend_request_registration_with_proof(
	who_secret: &VrfSecret,
	period: u32,
	seq: u8,
	call: RuntimeCall,
) {
	let uxt = build_friend_request_registration_ext(who_secret, period, seq, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

#[allow(dead_code)]
fn build_set_personal_id_account_ext(
	who_secret: &VrfSecret,
	person_id: u64,
	account: &sr25519::Pair,
) -> UncheckedExtrinsic {
	let account_id = pair_to_account_id(account);
	let call_valid_at = frame_system::Pallet::<Runtime>::block_number();

	let call = RuntimeCall::People(indiv_pallet_people::Call::set_personal_id_account {
		account: account_id.clone(),
		call_valid_at,
	});

	let mut tx_ext = base_tx_ext(call.clone());

	let rest_ext = (
		(
			tx_ext.0 .0 .3.clone(),
			tx_ext.0 .0 .4.clone(),
			tx_ext.0 .0 .5.clone(),
			tx_ext.0 .0 .6.clone(),
			tx_ext.0 .0 .7.clone(),
			tx_ext.0 .0 .8.clone(),
			tx_ext.0 .0 .9.clone(),
			tx_ext.0 .0 .10.clone(),
		),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded_implications =
			(implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded_implications)
	};

	let signature = Crypto::sign(who_secret, &msg[..]).expect("signing should work");

	tx_ext.0 .0 .2 = indiv_pallet_people::extension::AsPerson::new(Some(
		indiv_pallet_people::extension::AsPersonInfo::AsPersonalIdentityWithProof(
			signature, person_id,
		),
	));

	finalize_uxt(call, tx_ext)
}

#[allow(dead_code)]
#[track_caller]
fn exec_set_personal_id_account(who_secret: &VrfSecret, person_id: u64, account: &sr25519::Pair) {
	let uxt = build_set_personal_id_account_ext(who_secret, person_id, account);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

#[allow(dead_code)]
fn build_signed_as_personal_id_ext(who: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let who_account = pair_to_account_id(who);
	let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);

	tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);

	tx_ext.0 .0 .2 = indiv_pallet_people::extension::AsPerson::new(Some(
		indiv_pallet_people::extension::AsPersonInfo::AsPersonalIdentityWithAccount(nonce),
	));

	let rest_ext = (
		(
			tx_ext.0 .0 .2.clone(),
			tx_ext.0 .0 .3.clone(),
			tx_ext.0 .0 .4.clone(),
			tx_ext.0 .0 .5.clone(),
			tx_ext.0 .0 .6.clone(),
			tx_ext.0 .0 .7.clone(),
			tx_ext.0 .0 .8.clone(),
			tx_ext.0 .0 .9.clone(),
			tx_ext.0 .0 .10.clone(),
		),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded_implications =
			(implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded_implications)
	};

	let raw_sig = who.sign(&msg);

	tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
		MultiSignature::from(raw_sig),
		who_account,
	);

	finalize_uxt(call, tx_ext)
}

#[allow(dead_code)]
#[track_caller]
fn exec_signed_as_personal_id(who: &sr25519::Pair, call: RuntimeCall) {
	let uxt = build_signed_as_personal_id_ext(who, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

fn build_signed_as_alias_with_account_revised_ext(
	who: &sr25519::Pair,
	who_secret: &VrfSecret,
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let who_account = pair_to_account_id(who);
	let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);

	// update CheckNonce
	{
		tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);
	}

	// update AsPerson
	{
		let ca = indiv_pallet_people::AccountToAlias::<Runtime>::get(&who_account).unwrap();
		let context = ca.ca.context;
		let rest_ext = (
			(
				tx_ext.0 .0 .3.clone(),
				tx_ext.0 .0 .4.clone(),
				tx_ext.0 .0 .5.clone(),
				tx_ext.0 .0 .6.clone(),
				tx_ext.0 .0 .7.clone(),
				tx_ext.0 .0 .8.clone(),
				tx_ext.0 .0 .9.clone(),
				tx_ext.0 .0 .10.clone(),
			),
			tx_ext.0 .1.clone(),
			tx_ext.0 .2.clone(),
			tx_ext.0 .3.clone(),
			tx_ext.0 .4.clone(),
			tx_ext.0 .5.clone(),
			tx_ext.0 .6.clone(),
			tx_ext.0 .7.clone(),
			tx_ext.0 .8.clone(),
			tx_ext.0 .9.clone(),
		);
		let msg = {
			let implication_base = (0u8, &call);
			let implication_explicit = &rest_ext;
			let implication_implicit = &rest_ext.implicit().unwrap();
			let inherited_implication =
				(implication_base, implication_explicit, implication_implicit);
			let tuple = (inherited_implication, String::from("revise"), &who_account, nonce);

			sp_io::hashing::blake2_256(&tuple.encode())
		};

		let member = Crypto::member_from_secret(who_secret);
		let ring_index = indiv_pallet_members::Pallet::<Runtime>::member_status(
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			&member,
		)
		.unwrap()
		.ring_index()
		.unwrap();
		let members = indiv_pallet_members::RingKeys::<Runtime>::get((
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			ring_index,
			0u32,
		));
		let commitment =
			Crypto::open(verifiable::ring::RingDomainSize::Domain11, &member, members.into_iter())
				.unwrap();
		let proof = Crypto::create(commitment, who_secret, &context[..], &msg[..]).unwrap().0;

		assert_eq!(ring_index, ca.ring);

		tx_ext.0 .0 .2 = indiv_pallet_people::extension::AsPerson::new(Some(
			indiv_pallet_people::extension::AsPersonInfo::AsPersonalAliasWithAccountRevised(
				nonce, proof, ring_index, context,
			),
		));
	}

	// update VerifySignature
	{
		let rest_ext = (
			(
				tx_ext.0 .0 .2.clone(),
				tx_ext.0 .0 .3.clone(),
				tx_ext.0 .0 .4.clone(),
				tx_ext.0 .0 .5.clone(),
				tx_ext.0 .0 .6.clone(),
				tx_ext.0 .0 .7.clone(),
				tx_ext.0 .0 .8.clone(),
				tx_ext.0 .0 .9.clone(),
				tx_ext.0 .0 .10.clone(),
			),
			tx_ext.0 .1.clone(),
			tx_ext.0 .2.clone(),
			tx_ext.0 .3.clone(),
			tx_ext.0 .4.clone(),
			tx_ext.0 .5.clone(),
			tx_ext.0 .6.clone(),
			tx_ext.0 .7.clone(),
			tx_ext.0 .8.clone(),
			tx_ext.0 .9.clone(),
		);

		let msg = {
			let implication_base = (0u8, &call);
			let implication_explicit = &rest_ext;
			let implication_implicit = &rest_ext.implicit().unwrap();
			let encoded_implications =
				(implication_base, implication_explicit, implication_implicit).encode();
			sp_io::hashing::blake2_256(&encoded_implications)
		};

		// Sign the message with the sr25519 key.
		let raw_sig = who.sign(&msg);

		tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
			MultiSignature::from(raw_sig),
			who_account.clone(),
		);
	}

	finalize_uxt(call, tx_ext)
}

#[track_caller]
fn exec_signed_as_alias_with_account_revised(
	who_alias: &sr25519::Pair,
	who_secret: &VrfSecret,
	call: RuntimeCall,
) {
	let uxt = build_signed_as_alias_with_account_revised_ext(who_alias, who_secret, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

fn build_signed_as_alias_with_account_ext(
	who: &sr25519::Pair,
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let who_account = pair_to_account_id(who);
	let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);

	// update CheckNonce
	{
		tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);
	}

	// update AsPerson
	{
		tx_ext.0 .0 .2 = indiv_pallet_people::extension::AsPerson::new(Some(
			indiv_pallet_people::extension::AsPersonInfo::AsPersonalAliasWithAccount(nonce),
		));
	}

	// update VerifySignature
	{
		let rest_ext = (
			(
				tx_ext.0 .0 .2.clone(),
				tx_ext.0 .0 .3.clone(),
				tx_ext.0 .0 .4.clone(),
				tx_ext.0 .0 .5.clone(),
				tx_ext.0 .0 .6.clone(),
				tx_ext.0 .0 .7.clone(),
				tx_ext.0 .0 .8.clone(),
				tx_ext.0 .0 .9.clone(),
				tx_ext.0 .0 .10.clone(),
			),
			tx_ext.0 .1.clone(),
			tx_ext.0 .2.clone(),
			tx_ext.0 .3.clone(),
			tx_ext.0 .4.clone(),
			tx_ext.0 .5.clone(),
			tx_ext.0 .6.clone(),
			tx_ext.0 .7.clone(),
			tx_ext.0 .8.clone(),
			tx_ext.0 .9.clone(),
		);

		let msg = {
			let implication_base = (0u8, &call);
			let implication_explicit = &rest_ext;
			let implication_implicit = &rest_ext.implicit().unwrap();
			let encoded_implications =
				(implication_base, implication_explicit, implication_implicit).encode();
			sp_io::hashing::blake2_256(&encoded_implications)
		};

		// Sign the message with the sr25519 key.
		let raw_sig = who.sign(&msg);

		tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
			MultiSignature::from(raw_sig),
			who_account.clone(),
		);
	}

	finalize_uxt(call, tx_ext)
}

#[track_caller]
fn exec_signed_as_alias_with_account(who: &sr25519::Pair, call: RuntimeCall) {
	let uxt = build_signed_as_alias_with_account_ext(who, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

fn build_signed_ext(who: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let who_account = pair_to_account_id(who);

	// update CheckNonce
	{
		let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);
		tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);
	}

	// update VerifySignature
	{
		let rest_ext = (
			(
				tx_ext.0 .0 .2.clone(),
				tx_ext.0 .0 .3.clone(),
				tx_ext.0 .0 .4.clone(),
				tx_ext.0 .0 .5.clone(),
				tx_ext.0 .0 .6.clone(),
				tx_ext.0 .0 .7.clone(),
				tx_ext.0 .0 .8.clone(),
				tx_ext.0 .0 .9.clone(),
				tx_ext.0 .0 .10.clone(),
			),
			tx_ext.0 .1.clone(),
			tx_ext.0 .2.clone(),
			tx_ext.0 .3.clone(),
			tx_ext.0 .4.clone(),
			tx_ext.0 .5.clone(),
			tx_ext.0 .6.clone(),
			tx_ext.0 .7.clone(),
			tx_ext.0 .8.clone(),
			tx_ext.0 .9.clone(),
		);

		let msg = {
			let implication_base = (0u8, &call);
			let implication_explicit = &rest_ext;
			let implication_implicit = &rest_ext.implicit().unwrap();
			let encoded_implications =
				(implication_base, implication_explicit, implication_implicit).encode();
			sp_io::hashing::blake2_256(&encoded_implications)
		};

		// Sign the message with the sr25519 key.
		let raw_sig = who.sign(&msg);

		tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
			MultiSignature::from(raw_sig),
			who_account,
		);
	}

	finalize_uxt(call, tx_ext)
}

#[track_caller]
fn exec_signed(who: &sr25519::Pair, call: RuntimeCall) {
	let uxt = build_signed_ext(who, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

fn build_signed_game_invited_ext(
	who: &sr25519::Pair,
	ticket: &sr25519::Pair,
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let who_account = pair_to_account_id(who);
	let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);

	// update CheckNonce (although unnecessary)
	{
		tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);
	}

	// update GameAsInvited
	{
		let inviter = Sr25519Keyring::Alice.to_account_id();
		let ticket_account = pair_to_account_id(ticket);
		let signature = ticket.sign(&who_account.encode()[..]).into();

		tx_ext.0 .0 .5 = indiv_pallet_game::GameAsInvited::<Runtime>::new(Some(
			indiv_pallet_game::GameAsInvitedData {
				nonce,
				inviter,
				ticket: ticket_account,
				signature,
			},
		));
	}

	// update VerifySignature
	{
		let rest_ext = (
			(
				tx_ext.0 .0 .2.clone(),
				tx_ext.0 .0 .3.clone(),
				tx_ext.0 .0 .4.clone(),
				tx_ext.0 .0 .5.clone(),
				tx_ext.0 .0 .6.clone(),
				tx_ext.0 .0 .7.clone(),
				tx_ext.0 .0 .8.clone(),
				tx_ext.0 .0 .9.clone(),
				tx_ext.0 .0 .10.clone(),
			),
			tx_ext.0 .1.clone(),
			tx_ext.0 .2.clone(),
			tx_ext.0 .3.clone(),
			tx_ext.0 .4.clone(),
			tx_ext.0 .5.clone(),
			tx_ext.0 .6.clone(),
			tx_ext.0 .7.clone(),
			tx_ext.0 .8.clone(),
			tx_ext.0 .9.clone(),
		);

		let msg = {
			let implication_base = (0u8, &call);
			let implication_explicit = &rest_ext;
			let implication_implicit = &rest_ext.implicit().unwrap();
			let encoded_implications =
				(implication_base, implication_explicit, implication_implicit).encode();
			sp_io::hashing::blake2_256(&encoded_implications)
		};

		// Sign the message with the sr25519 key.
		let raw_sig = who.sign(&msg);

		tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
			MultiSignature::from(raw_sig),
			who_account,
		);
	}

	finalize_uxt(call, tx_ext)
}

#[track_caller]
fn exec_signed_game_invited(who: &sr25519::Pair, ticket: &sr25519::Pair, call: RuntimeCall) {
	let uxt = build_signed_game_invited_ext(who, ticket, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

fn build_signed_as_participant_ext(who: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());

	let who_account = pair_to_account_id(who);
	let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);

	// update CheckNonce
	tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);

	// update ScoreAsParticipant
	tx_ext.0 .0 .4 = indiv_pallet_score::ScoreAsParticipant::<Runtime>::new(Some(
		indiv_pallet_score::ScoreAsParticipantData { nonce },
	));

	// update VerifySignature
	let rest_ext = (
		(
			tx_ext.0 .0 .2.clone(),
			tx_ext.0 .0 .3.clone(),
			tx_ext.0 .0 .4.clone(),
			tx_ext.0 .0 .5.clone(),
			tx_ext.0 .0 .6.clone(),
			tx_ext.0 .0 .7.clone(),
			tx_ext.0 .0 .8.clone(),
			tx_ext.0 .0 .9.clone(),
			tx_ext.0 .0 .10.clone(),
		),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded_implications =
			(implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded_implications)
	};

	let raw_sig = who.sign(&msg);

	tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
		MultiSignature::from(raw_sig),
		who_account,
	);

	finalize_uxt(call, tx_ext)
}

#[track_caller]
fn exec_signed_as_score_participant(who: &sr25519::Pair, call: RuntimeCall) {
	let uxt = build_signed_as_participant_ext(who, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

// Helper function to build an extrinsic signed by a shielded coin account (using AsCoin extension)
fn build_as_coin_ext(who_pair: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());
	let who_account = pair_to_account_id(who_pair);

	// 1. Set the extension to AsCoin (Index 0.0.8)
	tx_ext.0 .0 .8 = indiv_pallet_coinage::extension::AsCoinage::<Runtime>::new(Some(
		indiv_pallet_coinage::extension::AsCoinageInfo::AsCoin,
	));

	// 2. Update Nonce (Index 0.7)
	// Coin accounts might not exist in System if they never held native balance.
	let nonce = frame_system::Pallet::<Runtime>::account_nonce(&who_account);
	// base_tx_ext initializes nonce to 0, so we only update if non-zero.
	if !nonce.is_zero() {
		tx_ext.0 .7 = frame_system::CheckNonce::<Runtime>::from(nonce);
	}

	// 3. Calculate the signature payload (rest_ext) and update VerifySignature (Index 0.0.1)
	// The signature payload for VerifySignature includes all other extensions.

	let rest_ext = (
		(
			// tx_ext.0.0.1 is VerifySignature, excluded from its own signing payload
			tx_ext.0 .0 .2.clone(),
			tx_ext.0 .0 .3.clone(),
			tx_ext.0 .0 .4.clone(),
			tx_ext.0 .0 .5.clone(),
			tx_ext.0 .0 .6.clone(),
			tx_ext.0 .0 .7.clone(),
			tx_ext.0 .0 .8.clone(),
			tx_ext.0 .0 .9.clone(),
			tx_ext.0 .0 .10.clone(),
		),
		tx_ext.0 .1.clone(),
		tx_ext.0 .2.clone(),
		tx_ext.0 .3.clone(),
		tx_ext.0 .4.clone(),
		tx_ext.0 .5.clone(),
		tx_ext.0 .6.clone(),
		tx_ext.0 .7.clone(),
		tx_ext.0 .8.clone(),
		tx_ext.0 .9.clone(),
	);

	let msg = {
		let implication_base = (0u8, &call);
		let implication_explicit = &rest_ext;
		let implication_implicit = &rest_ext.implicit().unwrap();
		let encoded_implications =
			(implication_base, implication_explicit, implication_implicit).encode();
		sp_io::hashing::blake2_256(&encoded_implications)
	};

	// Sign the message with the coin's sr25519 key.
	let raw_sig = who_pair.sign(&msg);

	// Update VerifySignature
	tx_ext.0 .0 .1 = pallet_verify_signature::VerifySignature::<Runtime>::new_with_signature(
		MultiSignature::from(raw_sig),
		who_account,
	);

	finalize_uxt(call, tx_ext)
}

#[track_caller]
fn exec_as_coin(who_pair: &sr25519::Pair, call: RuntimeCall) {
	let uxt = build_as_coin_ext(who_pair, call);
	Executive::apply_extrinsic(uxt)
		.expect("transaction is valid")
		.expect("dispatch succeeds");
}

/// Advance the chain to `target_block`
fn advance_to_block(target_block: frame_system::pallet_prelude::BlockNumberFor<Runtime>) {
	loop {
		let current = frame_system::Pallet::<Runtime>::block_number();
		if current >= target_block {
			break;
		}

		// Execute previous block idle and offchain worker
		AllPalletsWithSystem::on_idle(current, Weight::MAX);
		AllPalletsWithSystem::offchain_worker(current);

		// Advance time by 2 seconds (2000 ms)
		let now_ms: u64 = pallet_timestamp::Now::<Runtime>::get();
		pallet_timestamp::Now::<Runtime>::put(now_ms.saturating_add(2_000));

		// Advance block number by 1
		let next = current.saturating_add(1u32);
		frame_system::Pallet::<Runtime>::initialize(
			&next,
			&Default::default(),
			&Default::default(),
		);

		// Run on_poll for pallets that drive state forward
		let mut wm_people_lite = WeightMeter::with_limit(Weight::MAX);
		indiv_pallet_people_lite::Pallet::<Runtime>::on_poll(next, &mut wm_people_lite);
		let mut wm_people = WeightMeter::with_limit(Weight::MAX);
		indiv_pallet_people::Pallet::<Runtime>::on_poll(next, &mut wm_people);
		let mut wm_game = WeightMeter::with_limit(Weight::MAX);
		indiv_pallet_game::Pallet::<Runtime>::on_poll(next, &mut wm_game);
		let mut wm_score = WeightMeter::with_limit(Weight::MAX);
		indiv_pallet_score::Pallet::<Runtime>::on_poll(next, &mut wm_score);

		// Run transaction from the transaction pool submitted from the offchain worker
		let transactions = {
			TRANSACTION_POOL.with_borrow_mut(|pool| std::mem::take(&mut pool.write().transactions))
		};
		for tx in transactions {
			let tx = Decode::decode(&mut &tx[..]).unwrap();
			Executive::apply_extrinsic(tx)
				.expect("transaction is valid")
				.expect("dispatch succeeds");
		}
	}
}

/// Advance exactly one block.
fn advance_block() {
	let next_block = frame_system::Pallet::<Runtime>::block_number().saturating_add(1u32);
	advance_to_block(next_block);
}

/// Advance until the on-chain unix time (seconds) reaches or exceeds `target_s`.
fn advance_until_time(target_s: u32) {
	loop {
		let now_ms: u64 = pallet_timestamp::Now::<Runtime>::get();
		if now_ms >= target_s as u64 * 1000 {
			break;
		}
		let next_block = frame_system::Pallet::<Runtime>::block_number().saturating_add(1u32);
		advance_to_block(next_block);
	}
}

/// Set the current time without executing blocks.
fn set_time(secs: u64) {
	pallet_timestamp::Now::<Runtime>::put(secs * 1000);
}

/// Setup the external asset used by many pallets.
fn setup_external_asset() {
	Assets::force_create(
		RuntimeOrigin::root(),
		ExternalAssetLocation::get(),
		pair_to_account_id(&Sr25519Keyring::Alice.pair()).into(),
		true,
		10,
	)
	.expect("create asset should work");

	// Set up the asset rate for native <-> external asset conversion.
	// Native has 10 decimals, external asset has 6 decimals.
	// 1 raw external asset ($10^-6) = 10^4 raw native ($10^-10), so rate = 10^4.
	AssetRate::create(
		RuntimeOrigin::root(),
		alloc::boxed::Box::new(ExternalAssetLocation::get()),
		sp_runtime::FixedU128::from_u32(10_000),
	)
	.expect("create asset rate should work");

	// That operation mirrors the post-upgrade sudo step that operators will run on live chains.
	Coinage::set_underlying_asset_id(RuntimeOrigin::root(), ExternalAssetLocation::get())
		.expect("set_underlying_asset_id should succeed");
}

/// Set up `count` people using `DummyDim`.
fn setup_people(count: u32) {
	let max_batch_size: u32 =
		<Runtime as indiv_pallet_dummy_dim::Config>::MaxPersonBatchSize::get();
	let batch_count = count.div_ceil(max_batch_size);

	// Recognize personhood in batches.
	for i in 0..batch_count {
		let batch_size = max_batch_size.min(count - i * max_batch_size);
		let start_id = indiv_pallet_people::NextPersonalId::<Runtime>::get();

		DummyDim::reserve_ids(RuntimeOrigin::root(), batch_size).unwrap();

		let ids_and_keys: Vec<_> = (start_id..start_id + batch_size as u64)
			.map(|id| {
				let secret = {
					let mut s = [0u8; 32];
					s[0..4].copy_from_slice(&(id as u32).to_le_bytes()[..]);
					Crypto::new_secret(s)
				};
				let key = Crypto::member_from_secret(&secret);
				(id, key)
			})
			.collect();

		DummyDim::recognize_personhood(RuntimeOrigin::root(), ids_and_keys.try_into().unwrap())
			.unwrap();
	}

	// Wait until all people are active.
	while indiv_pallet_members::ActiveMembers::<Runtime>::get(
		indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
	) < count
	{
		advance_block();
	}
}

const GAME_PHASE_SPEEDUP: u32 = 5;
const IN_BETWEEN_GAMES: u32 = 1_200 / GAME_PHASE_SPEEDUP; // 240 seconds
const PAYOUT_DURATION: u32 = 300 / GAME_PHASE_SPEEDUP; // 60 blocks (120 seconds)

/// Reduce game phase durations to speed up game integration tests.
fn reduce_game_phase_durations() {
	let durations = crate::people::GamePhaseDurations::get();

	Game::set_game_phases(
		RuntimeOrigin::root(),
		indiv_pallet_game::PhaseDurationValues {
			registration: durations.registration / GAME_PHASE_SPEEDUP,
			shuffle: durations.shuffle / GAME_PHASE_SPEEDUP,
			post_shuffle_margin: durations.post_shuffle_margin / GAME_PHASE_SPEEDUP,
			reporting: durations.reporting / GAME_PHASE_SPEEDUP,
			player_process: durations.player_process / GAME_PHASE_SPEEDUP,
			airdrop_claim_window: durations.airdrop_claim_window / GAME_PHASE_SPEEDUP,
		},
	)
	.unwrap();
}

/// Per-winner prize used by airdrop integration tests, in external-asset base units.
const AIRDROP_PRIZE_PER_WINNER: Balance = 1_000_000;

/// Enable the external asset for airdrop prizes and fund [`GameAirdropSource`] with enough of
/// it to cover `max_winners * AIRDROP_PRIZE_PER_WINNER`. `enable_asset` also debits the
/// asset's minimum balance from the funded source to seed the pallet pot's asset account.
fn setup_airdrop_prize_asset(max_winners: u32) {
	let min_balance = FungibleExternalAsset::minimum_balance();
	let prize_funding = (max_winners as Balance).saturating_mul(AIRDROP_PRIZE_PER_WINNER);
	let source = GameAirdropSource::get();
	// Mint enough external asset into the source to cover both the per-event prize allocation
	// and the one-off `enable_asset` seed transfer to the pot.
	FungibleExternalAsset::mint_into(&source, prize_funding.saturating_add(min_balance))
		.expect("funding the airdrop source must succeed");
	Airdrop::enable_asset(RuntimeOrigin::root(), ExternalAssetLocation::get(), source)
		.expect("enable_asset should succeed for the external asset");
}

/// Build the `airdrop_prize` value to attach to a `GameSchedule` for the integration tests.
fn airdrop_prize_for(
	max_winners: u32,
) -> AirdropPrize<<Runtime as pallet_assets::Config>::AssetId, Balance> {
	AirdropPrize {
		asset_id: ExternalAssetLocation::get(),
		asset_amount: AIRDROP_PRIZE_PER_WINNER,
		max_winners,
		winner_cap: sp_runtime::Permill::from_percent(100),
	}
}

/// Re-implementation of `Pallet::<T>::airdrop_event_id` for the integration tests: the
/// 32-byte event id is the 28-byte ASCII base concatenated with `game_index.to_be_bytes()`.
fn airdrop_event_id_for(game_index: u32) -> [u8; 32] {
	let mut event_id = [0u8; 32];
	event_id[..28].copy_from_slice(b"pop:game:airdrop:           ");
	event_id[28..].copy_from_slice(&game_index.to_be_bytes());
	event_id
}

/// Build an `AirdropVrf::Account` VRF for `pair` for the given event. The runtime
/// reinterprets `AccountId32` bytes as the sr25519 public key, so any sr25519 pair whose
/// account id matches the signer works here.
fn build_account_airdrop_vrf(
	pair: &sr25519::Pair,
	event_id: [u8; 32],
) -> AirdropVrf<indiv_pallet_airdrop::ProofOf<Runtime>> {
	use sp_core::crypto::VrfSecret as _;
	let public = pair.public();
	let sign_data = transcript_for_event(&event_id, &public).into_sign_data();
	let signature = pair.vrf_sign(&sign_data);
	AirdropVrf::Account(signature)
}

/// Build an `AirdropVrf::Alias` ring proof for a recognized player. The membership ring used is
/// the people ring, since alias-based signups go through `ensure_person`.
fn build_alias_airdrop_vrf(
	who_secret: &VrfSecret,
	event_id: [u8; 32],
	participant_origin: RegistrationEntry<AccountId32>,
) -> AirdropVrf<indiv_pallet_airdrop::ProofOf<Runtime>> {
	let member = Crypto::member_from_secret(who_secret);
	let identifier = indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER;
	let ring_index: RingIndex =
		indiv_pallet_members::Pallet::<Runtime>::member_status(identifier, &member)
			.expect("alias signer must be in the people ring")
			.ring_index()
			.expect("alias signer must have been onboarded into a ring");
	let revision: RevisionIndex =
		indiv_pallet_members::Root::<Runtime>::get(identifier, ring_index)
			.map(|root| root.revision)
			.unwrap_or_default();
	let members = indiv_pallet_members::RingKeys::<Runtime>::get((identifier, ring_index, 0u32));
	let commitment =
		Crypto::open(verifiable::ring::RingDomainSize::Domain11, &member, members.into_iter())
			.expect("opening the ring commitment for the alias signer must succeed");
	let context = context_for_event(&event_id);
	let msg = participant_origin.encode();
	let proof = Crypto::create(commitment, who_secret, &context[..], &msg[..])
		.expect("creating the alias airdrop proof must succeed")
		.0;
	AirdropVrf::Alias { proof, ring_index, revision }
}

/// Advance blocks until the airdrop event reaches `Status::Registering`. Tolerates the
/// event not yet existing — `do_schedule_airdrop` runs inside `Game::on_poll` once a slot
/// in `GameSchedules` becomes the active game, which takes at least one block after
/// `Game::schedule_games`.
fn drive_airdrop_to_registering(event_id: [u8; 32]) {
	const MAX_BLOCKS: u32 = 20_000;
	let mut seen = false;
	for _ in 0..MAX_BLOCKS {
		match indiv_pallet_airdrop::Events::<Runtime>::get(event_id).map(|e| e.status) {
			Some(Status::Registering { .. }) => return,
			Some(_) => {
				seen = true;
				advance_block();
			},
			None =>
				if seen {
					panic!("airdrop event {event_id:?} is gone before reaching Registering");
				} else {
					advance_block();
				},
		}
	}
	panic!("airdrop event {event_id:?} never reached Registering");
}

/// Advance blocks until the airdrop event reaches `Status::Claiming`. The OCW runs in
/// `advance_block` so phase transitions auto-progress as time advances.
fn drive_airdrop_to_claiming(event_id: [u8; 32]) {
	const MAX_BLOCKS: u32 = 20_000;
	for _ in 0..MAX_BLOCKS {
		match indiv_pallet_airdrop::Events::<Runtime>::get(event_id).map(|e| e.status) {
			Some(Status::Claiming { .. }) => return,
			None => panic!("airdrop event {event_id:?} is gone before reaching Claiming"),
			_ => advance_block(),
		}
	}
	panic!("airdrop event {event_id:?} never reached Claiming");
}
