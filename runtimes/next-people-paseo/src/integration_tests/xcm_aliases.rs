// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0

//! Origin alias permissions, storage deposits and the runtime query API.

use super::new_test_ext;
use crate::{
	xcm_config::{
		AssetHubLocation, AuthorizeAliasHoldReason, Barrier, CheapTrustedAliasers, NextAhLocation,
		XcmConfig,
	},
	AccountId, Balances, Block, PolkadotXcm, Runtime, RuntimeOrigin, System,
};
use codec::{Encode, MaxEncodedLen};
use frame_support::{
	assert_noop, assert_ok,
	traits::{fungible::InspectHold, ContainsPair},
};
use sp_keyring::Sr25519Keyring;
use xcm::latest::prelude::*;
use xcm_builder::AllowExplicitUnpaidExecutionFrom;
use xcm_executor::traits::{Properties, ShouldExecute};
use xcm_runtime_apis::authorized_aliases::{
	runtime_decl_for_authorized_aliasers_api::AuthorizedAliasersApi, OriginAliaser,
};

type Aliasers = <XcmConfig as xcm_executor::Config>::Aliasers;

fn local_account(account: AccountId) -> Location {
	Location::new(0, AccountId32 { network: None, id: account.into() })
}

fn remote_account(id: [u8; 32]) -> Location {
	Location::new(1, [Parachain(4242), AccountId32 { network: None, id }])
}

#[test]
fn cheap_alias_rules_allow_children_system_accounts_and_asset_hub_root() {
	let account = AccountId32 { network: None, id: [1; 32] };
	let cases = [
		(Location::new(1, Parachain(4242)), Location::new(1, [Parachain(4242), GeneralIndex(7)])),
		(Location::new(1, [Parachain(1000), account]), Location::new(0, account)),
		(AssetHubLocation::get(), Location::new(2, GlobalConsensus(Ethereum { chain_id: 1 }))),
	];
	for (origin, target) in cases {
		assert!(CheapTrustedAliasers::contains(&origin, &target));
		assert!(Aliasers::contains(&origin, &target));
	}
}

#[test]
fn cheap_alias_rules_reject_wrong_accounts_chains_and_asset_hub_children() {
	let account = AccountId32 { network: None, id: [1; 32] };
	let other = AccountId32 { network: None, id: [2; 32] };
	let cases = [
		(Location::new(1, Parachain(4242)), Location::new(1, [Parachain(4243), GeneralIndex(7)])),
		(Location::new(1, [Parachain(1000), account]), Location::new(0, other)),
		(remote_account([1; 32]), Location::new(0, account)),
		(Location::new(1, [Parachain(1000), GeneralIndex(7)]), Location::parent()),
		(Location::new(1, Parachain(1001)), Location::parent()),
	];
	new_test_ext().execute_with(|| {
		for (origin, target) in cases {
			assert!(!CheapTrustedAliasers::contains(&origin, &target));
			assert!(!Aliasers::contains(&origin, &target));
		}
	});
}

#[test]
fn authorizations_hold_deposits_and_queries_follow_addition_and_removal() {
	new_test_ext().execute_with(|| {
		let alice = Sr25519Keyring::Alice.to_account_id();
		let target = local_account(alice.clone());
		let origin = remote_account([1; 32]);
		let other = remote_account([2; 32]);
		let initial_balance = Balances::free_balance(&alice);
		let deposit = crate::deposit(1, OriginAliaser::max_encoded_len() as u32);
		let reason = AuthorizeAliasHoldReason::get();
		assert!(deposit > 0);
		assert!(!Aliasers::contains(&origin, &target));
		assert!(<Runtime as AuthorizedAliasersApi<Block>>::authorized_aliasers(
			target.clone().into()
		)
		.unwrap()
		.is_empty());

		assert_ok!(PolkadotXcm::add_authorized_alias(
			RuntimeOrigin::signed(alice.clone()),
			Box::new(origin.clone().into()),
			None
		));
		assert_eq!(Balances::balance_on_hold(&reason, &alice), deposit);
		assert_eq!(Balances::free_balance(&alice), initial_balance - deposit);
		assert!(Aliasers::contains(&origin, &target));
		assert!(!CheapTrustedAliasers::contains(&origin, &target));
		assert!(!Aliasers::contains(&other, &target));
		assert!(!Aliasers::contains(&origin, &local_account(Sr25519Keyring::Bob.to_account_id())));
		assert_eq!(
			<Runtime as AuthorizedAliasersApi<Block>>::is_authorized_alias(
				origin.clone().into(),
				target.clone().into()
			),
			Ok(true)
		);
		assert_eq!(
			<Runtime as AuthorizedAliasersApi<Block>>::is_authorized_alias(
				other.clone().into(),
				target.clone().into()
			),
			Ok(false)
		);
		let entries =
			<Runtime as AuthorizedAliasersApi<Block>>::authorized_aliasers(target.clone().into())
				.unwrap();
		assert_eq!(entries.len(), 1);
		assert_eq!(entries[0].location, origin.clone().into());
		assert_eq!(entries[0].expiry, None);

		assert_ok!(PolkadotXcm::add_authorized_alias(
			RuntimeOrigin::signed(alice.clone()),
			Box::new(other.clone().into()),
			None
		));
		assert_eq!(
			Balances::balance_on_hold(&reason, &alice),
			crate::deposit(1, 2 * OriginAliaser::max_encoded_len() as u32)
		);
		assert_ok!(PolkadotXcm::remove_authorized_alias(
			RuntimeOrigin::signed(alice.clone()),
			Box::new(origin.clone().into())
		));
		assert_eq!(Balances::balance_on_hold(&reason, &alice), deposit);
		assert!(!Aliasers::contains(&origin, &target));
		assert!(Aliasers::contains(&other, &target));
		assert_eq!(
			<Runtime as AuthorizedAliasersApi<Block>>::is_authorized_alias(
				origin.into(),
				target.clone().into()
			),
			Ok(false)
		);

		assert_ok!(PolkadotXcm::remove_all_authorized_aliases(RuntimeOrigin::signed(
			alice.clone()
		)));
		assert_eq!(Balances::balance_on_hold(&reason, &alice), 0);
		assert_eq!(Balances::free_balance(&alice), initial_balance);
		assert!(!Aliasers::contains(&other, &target));
		assert!(<Runtime as AuthorizedAliasersApi<Block>>::authorized_aliasers(target.into())
			.unwrap()
			.is_empty());
	});
}

#[test]
fn authorizations_expire_at_the_selected_block() {
	new_test_ext().execute_with(|| {
		let alice = Sr25519Keyring::Alice.to_account_id();
		let target = local_account(alice.clone());
		let origin = remote_account([1; 32]);
		assert_ok!(PolkadotXcm::add_authorized_alias(
			RuntimeOrigin::signed(alice),
			Box::new(origin.clone().into()),
			Some(10)
		));
		System::set_block_number(9);
		assert!(Aliasers::contains(&origin, &target));
		assert_eq!(
			<Runtime as AuthorizedAliasersApi<Block>>::is_authorized_alias(
				origin.clone().into(),
				target.clone().into()
			),
			Ok(true)
		);
		System::set_block_number(10);
		assert!(!Aliasers::contains(&origin, &target));
		assert_eq!(
			<Runtime as AuthorizedAliasersApi<Block>>::is_authorized_alias(
				origin.into(),
				target.into()
			),
			Ok(false)
		);
	});
}

#[test]
fn authorization_requires_a_funded_signed_account_and_future_expiry() {
	new_test_ext().execute_with(|| {
		let origin = remote_account([1; 32]);
		assert_noop!(
			PolkadotXcm::add_authorized_alias(
				RuntimeOrigin::root(),
				Box::new(origin.clone().into()),
				None
			),
			sp_runtime::DispatchError::BadOrigin
		);
		assert_noop!(
			PolkadotXcm::add_authorized_alias(
				RuntimeOrigin::signed(Sr25519Keyring::Bob.to_account_id()),
				Box::new(origin.clone().into()),
				None
			),
			sp_runtime::TokenError::FundsUnavailable
		);
		assert_noop!(
			PolkadotXcm::add_authorized_alias(
				RuntimeOrigin::signed(Sr25519Keyring::Alice.to_account_id()),
				Box::new(origin.into()),
				Some(System::block_number().into())
			),
			pallet_xcm::Error::<Runtime>::ExpiresInPast
		);
	});
}

#[test]
fn barrier_accepts_cheap_aliases_only_when_the_result_has_unpaid_permission() {
	let parent = Location::parent();
	let governance = Location::new(1, Plurality { id: BodyId::Executive, part: BodyPart::Voice });
	for (target, expected) in [
		(governance, true),
		(Location::new(1, Parachain(4242)), false),
		(NextAhLocation::get(), false),
	] {
		let mut instructions =
			[AliasOrigin(target), UnpaidExecution { weight_limit: Unlimited, check_origin: None }];
		let mut properties = Properties { weight_credit: Weight::zero(), message_id: None };
		assert_eq!(
			Barrier::should_execute::<crate::RuntimeCall>(
				&parent,
				&mut instructions,
				Weight::from_parts(1_000_000, 1000),
				&mut properties
			)
			.is_ok(),
			expected
		);
	}
}

#[test]
fn stored_authorizations_do_not_grant_unpaid_admission() {
	new_test_ext().execute_with(|| {
		let alice = Sr25519Keyring::Alice.to_account_id();
		let target = local_account(alice.clone());
		let origin = remote_account([1; 32]);
		assert_ok!(PolkadotXcm::add_authorized_alias(
			RuntimeOrigin::signed(alice),
			Box::new(origin.clone().into()),
			None
		));
		assert!(Aliasers::contains(&origin, &target));
		let mut instructions =
			[AliasOrigin(target), UnpaidExecution { weight_limit: Unlimited, check_origin: None }];
		let mut properties = Properties { weight_credit: Weight::zero(), message_id: None };
		// Accept every resulting location to isolate the alias filter from unpaid origin trust.
		type Admission = AllowExplicitUnpaidExecutionFrom<
			frame_support::traits::Everything,
			CheapTrustedAliasers,
		>;
		assert!(Admission::should_execute::<crate::RuntimeCall>(
			&origin,
			&mut instructions,
			Weight::from_parts(1_000_000, 1000),
			&mut properties
		)
		.is_err());
		assert!(Barrier::should_execute::<crate::RuntimeCall>(
			&origin,
			&mut instructions,
			Weight::from_parts(1_000_000, 1000),
			&mut properties
		)
		.is_err());
	});
}

#[test]
fn metered_execution_applies_authorized_aliases_and_rejects_other_origins() {
	new_test_ext().execute_with(|| {
		let alice = Sr25519Keyring::Alice.to_account_id();
		let target = local_account(alice.clone());
		let authorized = remote_account([1; 32]);
		assert_ok!(PolkadotXcm::add_authorized_alias(
			RuntimeOrigin::signed(alice),
			Box::new(authorized.clone().into()),
			None
		));
		let limit = Weight::from_parts(1_000_000_000, 100_000);
		for origin in [authorized.clone(), remote_account([2; 32])] {
			let message =
				Xcm(vec![AliasOrigin(target.clone()), ExpectOrigin(Some(target.clone()))]);
			let outcome = xcm_executor::XcmExecutor::<XcmConfig>::prepare_and_execute(
				origin.clone(),
				message,
				&mut [0; 32],
				limit,
				limit,
			);
			if origin == authorized {
				assert!(
					matches!(outcome, Outcome::Complete { used } if used.all_lt(limit)),
					"{outcome:?}"
				);
			} else {
				assert!(
					matches!(
						outcome,
						Outcome::Incomplete {
							error: InstructionError { index: 0, error: XcmError::NoPermission },
							..
						}
					),
					"{outcome:?}"
				);
			}
		}
	});
}

#[test]
fn aliases_cannot_grant_root_dispatch() {
	for origin in [AssetHubLocation::get(), Location::parent(), NextAhLocation::get()] {
		new_test_ext().execute_with(|| {
			let message = Xcm(vec![AliasOrigin(NextAhLocation::get()), root_storage_write()]);
			let outcome = execute_with_credit(origin, message);
			assert!(
				matches!(
					outcome,
					Outcome::Incomplete {
						error: InstructionError { index: 0, error: XcmError::NoPermission },
						..
					}
				),
				"{outcome:?}"
			);
			assert_eq!(sp_io::storage::get(b"alias-root-test"), None);
		});
	}
}

#[test]
fn next_asset_hub_can_still_dispatch_as_root_without_aliasing() {
	new_test_ext().execute_with(|| {
		let message =
			Xcm(vec![root_storage_write(), ExpectTransactStatus(MaybeErrorCode::Success)]);
		let outcome = execute_with_credit(NextAhLocation::get(), message);
		assert!(matches!(outcome, Outcome::Complete { .. }), "{outcome:?}");
		assert_eq!(sp_io::storage::get(b"alias-root-test").unwrap().as_ref(), b"written");
	});
}

fn root_storage_write() -> Instruction<crate::RuntimeCall> {
	Transact {
		origin_kind: OriginKind::Superuser,
		fallback_max_weight: None,
		call: crate::RuntimeCall::System(frame_system::Call::set_storage {
			items: vec![(b"alias-root-test".to_vec(), b"written".to_vec())],
		})
		.encode()
		.into(),
	}
}

fn execute_with_credit(origin: Location, message: Xcm<crate::RuntimeCall>) -> Outcome {
	let limit = Weight::from_parts(1_000_000_000, 100_000);
	xcm_executor::XcmExecutor::<XcmConfig>::prepare_and_execute(
		origin,
		message,
		&mut [0; 32],
		limit,
		limit,
	)
}

#[cfg(feature = "runtime-benchmarks")]
#[test]
fn benchmark_authorization_allows_metered_alias_execution() {
	new_test_ext().execute_with(|| {
		let (origin, target) =
			system_parachains_common::benchmarking::set_up_worst_case_authorized_alias::<Runtime>();
		assert!(!CheapTrustedAliasers::contains(&origin, &target));
		let message = Xcm(vec![AliasOrigin(target.clone()), ExpectOrigin(Some(target))]);
		let outcome = execute_with_credit(origin, message);
		assert!(matches!(outcome, Outcome::Complete { .. }), "{outcome:?}");
	});
}
