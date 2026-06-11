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

//! Integration tests for `ProtectedAssetTransactor` — the XCM-level protected-asset
//! chokepoint.
//!
//! Verifies that a protected asset cannot leak via XCM when the global block flag is set:
//! every mutating `TransactAsset` method returns `XcmError::NoPermission` for
//! a protected-asset `Location` while `block_flag::is_blocked()` is true.

use frame_support::{
	parameter_types,
	traits::{
		tokens::imbalance::{
			ImbalanceAccounting, UnsafeConstructorDestructor, UnsafeManualAccounting,
		},
		Contains,
	},
};
use indiv_pallet_value_transfer_auth::{
	allow_only_siblings::AllowOnlySiblings, extension::block_flag, ProtectedAssetTransactor,
};
use std::sync::Mutex;
use xcm::latest::{
	Asset, AssetId, Error as XcmError, Fungibility, Junction::Parachain, Location, XcmContext,
	XcmHash,
};
use xcm_executor::{traits::TransactAsset, AssetsInHolding};

static TEST_LOCK: Mutex<()> = Mutex::new(());

parameter_types! {
	pub ProtectedAssetLocation: Location = Location::new(1, [Parachain(1500)]);
	pub TrustedAh: u32 = 1500;
	pub TrustedPeople: u32 = 1502;
}

struct TrustedNone;
impl Contains<Location> for TrustedNone {
	fn contains(_: &Location) -> bool {
		false
	}
}

type Trusted = AllowOnlySiblings<TrustedAh, TrustedPeople>;

type Guarded = ProtectedAssetTransactor<AlwaysOkTransactor, ProtectedAssetLocation, TrustedNone>;
type GuardedWithTrusted =
	ProtectedAssetTransactor<AlwaysOkTransactor, ProtectedAssetLocation, Trusted>;

struct AlwaysOkTransactor;

impl TransactAsset for AlwaysOkTransactor {
	fn can_check_in(
		_origin: &Location,
		_what: &Asset,
		_context: &XcmContext,
	) -> xcm::latest::Result {
		Ok(())
	}

	fn check_in(_origin: &Location, _what: &Asset, _context: &XcmContext) {}

	fn can_check_out(
		_dest: &Location,
		_what: &Asset,
		_context: &XcmContext,
	) -> xcm::latest::Result {
		Ok(())
	}

	fn check_out(_dest: &Location, _what: &Asset, _context: &XcmContext) {}

	fn deposit_asset(
		_what: AssetsInHolding,
		_who: &Location,
		_context: Option<&XcmContext>,
	) -> Result<(), (AssetsInHolding, XcmError)> {
		Ok(())
	}

	fn withdraw_asset(
		_what: &Asset,
		_who: &Location,
		_context: Option<&XcmContext>,
	) -> Result<AssetsInHolding, XcmError> {
		Ok(AssetsInHolding::new())
	}

	fn internal_transfer_asset(
		what: &Asset,
		_from: &Location,
		_to: &Location,
		_context: &XcmContext,
	) -> Result<Asset, XcmError> {
		Ok(what.clone())
	}

	fn mint_asset(_what: &Asset, _context: &XcmContext) -> Result<AssetsInHolding, XcmError> {
		Ok(AssetsInHolding::new())
	}
}

struct MockCredit(u128);

impl UnsafeConstructorDestructor<u128> for MockCredit {
	fn unsafe_clone(&self) -> Box<dyn ImbalanceAccounting<u128>> {
		Box::new(MockCredit(self.0))
	}

	fn forget_imbalance(&mut self) -> u128 {
		let amount = self.0;
		self.0 = 0;
		amount
	}
}

impl UnsafeManualAccounting<u128> for MockCredit {
	fn saturating_subsume(&mut self, mut other: Box<dyn ImbalanceAccounting<u128>>) {
		self.0 = self.0.saturating_add(other.forget_imbalance());
	}
}

impl ImbalanceAccounting<u128> for MockCredit {
	fn amount(&self) -> u128 {
		self.0
	}

	fn saturating_take(&mut self, amount: u128) -> Box<dyn ImbalanceAccounting<u128>> {
		let taken = self.0.min(amount);
		self.0 -= taken;
		Box::new(MockCredit(taken))
	}
}

fn reset() -> std::sync::MutexGuard<'static, ()> {
	let guard = TEST_LOCK.lock().expect("test lock is not poisoned");
	block_flag::block();
	guard
}

fn context() -> XcmContext {
	XcmContext { origin: None, message_id: XcmHash::default(), topic: None }
}

fn context_with_origin(origin: Location) -> XcmContext {
	XcmContext { origin: Some(origin), message_id: XcmHash::default(), topic: None }
}

fn protected_asset() -> Asset {
	Asset { id: AssetId(ProtectedAssetLocation::get()), fun: Fungibility::Fungible(100) }
}

fn non_protected_asset() -> Asset {
	Asset { id: AssetId(Location::new(1, [Parachain(3000)])), fun: Fungibility::Fungible(100) }
}

fn protected_asset_holding() -> AssetsInHolding {
	AssetsInHolding::new_from_fungible_credit(
		AssetId(ProtectedAssetLocation::get()),
		Box::new(MockCredit(100)),
	)
}

#[test]
fn protected_asset_withdraw_blocked_when_flag_set() {
	let _guard = reset();

	assert_eq!(
		Guarded::withdraw_asset(&protected_asset(), &Location::here(), None),
		Err(XcmError::NoPermission)
	);
}

#[test]
fn protected_asset_deposit_blocked_when_flag_set() {
	let _guard = reset();

	let result = Guarded::deposit_asset(protected_asset_holding(), &Location::here(), None);
	assert!(matches!(result, Err((_, XcmError::NoPermission))));
}

#[test]
fn protected_asset_internal_transfer_blocked_when_flag_set() {
	let _guard = reset();

	assert_eq!(
		Guarded::internal_transfer_asset(
			&protected_asset(),
			&Location::here(),
			&Location::parent(),
			&context()
		),
		Err(XcmError::NoPermission)
	);
}

#[test]
fn protected_asset_mint_blocked_when_flag_set() {
	let _guard = reset();

	assert_eq!(Guarded::mint_asset(&protected_asset(), &context()), Err(XcmError::NoPermission));
}

#[test]
fn protected_asset_transfer_blocked_when_flag_set() {
	let _guard = reset();

	assert_eq!(
		Guarded::transfer_asset(
			&protected_asset(),
			&Location::here(),
			&Location::parent(),
			&context()
		),
		Err(XcmError::NoPermission)
	);
}

#[test]
fn protected_asset_withdraw_passes_when_flag_unblocked() {
	let _guard = reset();
	block_flag::unblock();

	assert!(Guarded::withdraw_asset(&protected_asset(), &Location::here(), None).is_ok());

	block_flag::block();
}

#[test]
fn non_protected_asset_withdraw_unaffected_when_flag_set() {
	let _guard = reset();

	assert!(
		Guarded::withdraw_asset(&non_protected_asset(), &Location::here(), None).is_ok(),
		"non-protected asset should pass through regardless of flag state"
	);
}

#[test]
fn protected_asset_deposit_from_trusted_sibling_passes_when_flag_blocked() {
	let _guard = reset();
	let ctx = context_with_origin(Location::new(1, [Parachain(1500)]));

	assert!(GuardedWithTrusted::deposit_asset(
		protected_asset_holding(),
		&Location::here(),
		Some(&ctx)
	)
	.is_ok());
}

#[test]
fn protected_asset_deposit_from_untrusted_origin_rejected_when_flag_blocked() {
	let _guard = reset();
	let ctx = context_with_origin(Location::new(1, [Parachain(9999)]));

	let result =
		GuardedWithTrusted::deposit_asset(protected_asset_holding(), &Location::here(), Some(&ctx));
	assert!(matches!(result, Err((_, XcmError::NoPermission))));
}

#[test]
fn protected_asset_mint_from_trusted_sibling_passes_when_flag_blocked() {
	let _guard = reset();
	let ctx = context_with_origin(Location::new(1, [Parachain(1502)]));

	assert!(GuardedWithTrusted::mint_asset(&protected_asset(), &ctx).is_ok());
}
