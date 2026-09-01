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
use indiv_pallet_airdrop::types::EventInfo;
use sp_runtime::DispatchError;

/// Registration opens this many seconds after the draw is scheduled.
const REGISTRATION_DELAY: u64 = 10;
/// Registration stays open for this many seconds.
const REGISTRATION_WINDOW: u64 = 30;
/// Winners can claim for this many seconds after the draw.
const CLAIM_WINDOW: u64 = 120;
/// Winners drawn per scheduled draw.
const MAX_WINNERS: u32 = 4;
/// Upper bound on the blocks a newly recognized person needs to land in a built ring.
const MAX_RING_BUILD_BLOCKS: u32 = 1_000;

fn people_airdrops_context() -> Context {
	indiv_pallet_people_airdrops::Pallet::<Runtime>::people_airdrops_context()
}

fn prize_source() -> AccountId32 {
	<Runtime as indiv_pallet_people_airdrops::Config>::PrizeSource::get()
}

/// Recognize one person and wait until their key is in a built ring.
fn recognize_person(seed: [u8; 32]) -> VrfSecret {
	let secret = Crypto::new_secret(seed);
	let member = Crypto::member_from_secret(&secret);
	let id = indiv_pallet_people::NextPersonalId::<Runtime>::get();

	DummyDim::reserve_ids(RuntimeOrigin::root(), 1).expect("root can reserve a personal id");
	DummyDim::recognize_personhood(
		RuntimeOrigin::root(),
		vec![(id, member)].try_into().expect("one person fits the batch"),
	)
	.expect("root can recognize a person");

	// Onboarding and ring building happen in later blocks. A ring proof needs both the member's
	// ring assignment and that ring's built root.
	for _ in 0..MAX_RING_BUILD_BLOCKS {
		let ring_index = indiv_pallet_members::Pallet::<Runtime>::member_status(
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			&member,
		)
		.and_then(|status| status.ring_index());
		if let Some(ring_index) = ring_index {
			if indiv_pallet_members::Root::<Runtime>::get(
				indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
				ring_index,
			)
			.is_some()
			{
				return secret;
			}
		}
		advance_block();
	}
	panic!("the recognized person never joined a built ring");
}

/// Bind `account` to the person's alias in `context`, the one-off ring-proof step every
/// context-scoped person origin needs before an account can act for it.
fn bind_alias_account(secret: &VrfSecret, context: Context, account: &sr25519::Pair) {
	let set_alias = indiv_pallet_people::Call::<Runtime>::set_alias_account {
		account: pair_to_account_id(account),
		call_valid_at: frame_system::Pallet::<Runtime>::block_number(),
	};
	exec_as_alias_with_proof(secret, context, set_alias.into());
}

/// Bind `account` to the lite person's alias in `context`, the lite-tier counterpart of
/// [`bind_alias_account`].
fn bind_lite_alias_account(secret: &VrfSecret, context: Context, account: &sr25519::Pair) {
	let set_alias = indiv_pallet_people_lite::Call::<Runtime>::set_alias_account {
		account: pair_to_account_id(account),
		valid_at_block: frame_system::Pallet::<Runtime>::block_number(),
	};
	let uxt = build_as_lite_alias_with_proof_ext(secret, context, set_alias.into());
	Executive::apply_extrinsic(uxt)
		.expect("the lite alias setup transaction is valid")
		.expect("the lite alias setup dispatch succeeds");
}

/// Build a transaction dispatching `call` from the lite alias the account `who` is bound to,
/// authenticated by `who`'s signature and nonce.
fn build_as_lite_alias_with_account_ext(
	who: &sr25519::Pair,
	call: RuntimeCall,
) -> UncheckedExtrinsic {
	build_people_lite_auth_ext(
		who,
		indiv_pallet_people_lite::PeopleLiteAuthData::AsLiteAliasWithAccount,
		call,
	)
}

/// Dispatch `call` from the lite alias the account `who` is bound to, expecting success.
fn exec_as_lite_alias_with_account(who: &sr25519::Pair, call: RuntimeCall) {
	let uxt = build_as_lite_alias_with_account_ext(who, call);
	Executive::apply_extrinsic(uxt)
		.expect("the lite alias transaction is valid")
		.expect("the lite alias dispatch succeeds");
}

/// Enable the external asset for airdrops and fund the people-airdrops prize source with
/// `draws * MAX_WINNERS` prizes. `enable_asset` also debits the asset's minimum balance from
/// the source to seed the pot's asset account.
fn setup_people_airdrops_prize_asset(draws: u32) {
	let min_balance = FungibleExternalAsset::minimum_balance();
	let allocation = (draws as Balance)
		.saturating_mul(MAX_WINNERS as Balance)
		.saturating_mul(AIRDROP_PRIZE_PER_WINNER);
	let source = prize_source();
	FungibleExternalAsset::mint_into(&source, allocation.saturating_add(min_balance))
		.expect("funding the people-airdrops prize source must succeed");
	Airdrop::enable_asset(RuntimeOrigin::root(), ExternalAssetLocation::get(), source)
		.expect("enable_asset should succeed for the external asset");
}

/// Schedule one draw and return its event id.
fn schedule_one_draw() -> [u8; 32] {
	let now = pallet_timestamp::Now::<Runtime>::get() / 1000;
	let registration_starts = now.saturating_add(REGISTRATION_DELAY);
	let draw_time = registration_starts.saturating_add(REGISTRATION_WINDOW);
	let draw_index = indiv_pallet_people_airdrops::NextDrawIndex::<Runtime>::get();

	PeopleAirdrops::schedule_draws(
		RuntimeOrigin::root(),
		vec![EventInfo {
			prize: airdrop_prize_for(MAX_WINNERS),
			registration_starts,
			draw_time,
			end_time: draw_time.saturating_add(CLAIM_WINDOW),
		}]
		.try_into()
		.expect("one draw fits the batch"),
	)
	.expect("root can schedule a draw");

	indiv_pallet_people_airdrops::Pallet::<Runtime>::draw_event_id(draw_index)
}

/// A person registers for a draw through their airdrop alias account and claims the prize to a
/// destination that has no personhood of its own.
#[test]
fn person_registers_and_claims_a_draw() {
	new_test_ext().execute_with(|| {
		let secret = recognize_person([10u8; 32]);
		let alias_account_pair = sr25519::Pair::from_seed(&[11u8; 32]);
		bind_alias_account(&secret, people_airdrops_context(), &alias_account_pair);

		setup_people_airdrops_prize_asset(1);
		let event_id = schedule_one_draw();
		drive_airdrop_to_registering(event_id);

		let register = indiv_pallet_people_airdrops::Call::<Runtime>::register {
			event_ids: vec![event_id].try_into().expect("one draw fits the batch"),
		};
		exec_signed_as_alias_with_account(&alias_account_pair, register.into());

		let alias = Crypto::alias_in_context(&secret, &people_airdrops_context()[..])
			.expect("the person can derive their airdrop alias");
		let entry = RegistrationEntry::Alias { alias };
		let slot = indiv_pallet_people_airdrops::Pallet::<Runtime>::slot_for(
			&event_id,
			&indiv_pallet_people_airdrops::DrawSalts::<Runtime>::get(event_id)
				.expect("a scheduled draw has a salt")
				.0,
			&alias,
		);
		assert_eq!(
			indiv_pallet_airdrop::Registrations::<Runtime>::get(event_id, slot),
			Some(entry.clone())
		);

		drive_airdrop_to_claiming(event_id);
		assert!(indiv_pallet_airdrop::Winners::<Runtime>::contains_key(event_id, &entry));

		// The prize goes to an account with no personhood and no alias binding.
		let destination = Sr25519Keyring::Bob.to_account_id();
		assert_eq!(FungibleExternalAsset::balance(&destination), 0);
		let claim = indiv_pallet_people_airdrops::Call::<Runtime>::claim {
			event_id,
			destination: destination.clone(),
		};
		exec_signed_as_alias_with_account(&alias_account_pair, claim.into());

		assert_eq!(FungibleExternalAsset::balance(&destination), AIRDROP_PRIZE_PER_WINNER);
		assert!(!indiv_pallet_airdrop::Winners::<Runtime>::contains_key(event_id, &entry));
	});
}

/// A lite person registers for a draw through their airdrop alias account and claims the prize,
/// the same journey as a full person: draws are open to every proven person, whichever tier.
#[test]
fn lite_person_registers_and_claims_a_draw() {
	new_test_ext().execute_with(|| {
		let lite_pair = sr25519::Pair::from_seed(&[30u8; 32]);
		let lite_secret = register_lite_person_for_integration(&lite_pair);
		// The lite helper onboards and builds the ring synchronously, so no block has executed
		// yet and the relay randomness that salts a scheduled draw is still empty.
		advance_block();
		let alias_account_pair = sr25519::Pair::from_seed(&[31u8; 32]);
		bind_lite_alias_account(&lite_secret, people_airdrops_context(), &alias_account_pair);

		setup_people_airdrops_prize_asset(1);
		let event_id = schedule_one_draw();
		drive_airdrop_to_registering(event_id);

		let register = indiv_pallet_people_airdrops::Call::<Runtime>::register {
			event_ids: vec![event_id].try_into().expect("one draw fits the batch"),
		};
		exec_as_lite_alias_with_account(&alias_account_pair, register.into());

		let alias = Crypto::alias_in_context(&lite_secret, &people_airdrops_context()[..])
			.expect("the lite person can derive their airdrop alias");
		let entry = RegistrationEntry::Alias { alias };
		let slot = indiv_pallet_people_airdrops::Pallet::<Runtime>::slot_for(
			&event_id,
			&indiv_pallet_people_airdrops::DrawSalts::<Runtime>::get(event_id)
				.expect("a scheduled draw has a salt")
				.0,
			&alias,
		);
		assert_eq!(
			indiv_pallet_airdrop::Registrations::<Runtime>::get(event_id, slot),
			Some(entry.clone())
		);

		drive_airdrop_to_claiming(event_id);
		assert!(indiv_pallet_airdrop::Winners::<Runtime>::contains_key(event_id, &entry));

		// The prize goes to an account with no personhood and no alias binding.
		let destination = Sr25519Keyring::Charlie.to_account_id();
		assert_eq!(FungibleExternalAsset::balance(&destination), 0);
		let claim = indiv_pallet_people_airdrops::Call::<Runtime>::claim {
			event_id,
			destination: destination.clone(),
		};
		exec_as_lite_alias_with_account(&alias_account_pair, claim.into());

		assert_eq!(FungibleExternalAsset::balance(&destination), AIRDROP_PRIZE_PER_WINNER);
		assert!(!indiv_pallet_airdrop::Winners::<Runtime>::contains_key(event_id, &entry));
	});
}

/// A lite alias account bound in another context resolves to a lite person, but not to one in
/// the people-airdrops context, so it cannot register.
#[test]
fn lite_alias_account_in_another_context_cannot_register() {
	new_test_ext().execute_with(|| {
		let lite_pair = sr25519::Pair::from_seed(&[40u8; 32]);
		let lite_secret = register_lite_person_for_integration(&lite_pair);
		// The lite helper onboards and builds the ring synchronously, so no block has executed
		// yet and the relay randomness that salts a scheduled draw is still empty.
		advance_block();
		let alias_account_pair = sr25519::Pair::from_seed(&[41u8; 32]);
		bind_lite_alias_account(&lite_secret, score_context(), &alias_account_pair);

		setup_people_airdrops_prize_asset(1);
		let event_id = schedule_one_draw();
		drive_airdrop_to_registering(event_id);

		let register = indiv_pallet_people_airdrops::Call::<Runtime>::register {
			event_ids: vec![event_id].try_into().expect("one draw fits the batch"),
		};
		let uxt = build_as_lite_alias_with_account_ext(&alias_account_pair, register.into());

		assert_eq!(
			Executive::apply_extrinsic(uxt).expect("transaction is valid"),
			Err(DispatchError::BadOrigin)
		);
	});
}

/// An alias account bound in another context resolves to a person, but not to a person in the
/// people-airdrops context, so it cannot register.
#[test]
fn alias_account_in_another_context_cannot_register() {
	new_test_ext().execute_with(|| {
		let secret = recognize_person([20u8; 32]);
		let alias_account_pair = sr25519::Pair::from_seed(&[21u8; 32]);
		bind_alias_account(&secret, score_context(), &alias_account_pair);

		setup_people_airdrops_prize_asset(1);
		let event_id = schedule_one_draw();
		drive_airdrop_to_registering(event_id);

		let register = indiv_pallet_people_airdrops::Call::<Runtime>::register {
			event_ids: vec![event_id].try_into().expect("one draw fits the batch"),
		};
		let uxt = build_signed_as_alias_with_account_ext(&alias_account_pair, register.into());

		assert_eq!(
			Executive::apply_extrinsic(uxt).expect("transaction is valid"),
			Err(DispatchError::BadOrigin)
		);
	});
}
