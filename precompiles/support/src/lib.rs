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

//! Shared plumbing for this workspace's `pallet-revive` precompiles.
//!
//! Every precompile in this workspace rejects native value, rejects delegate calls, resolves the
//! EVM caller to a substrate account and charges worst-case read weight the same way. This crate
//! holds that plumbing so every precompile applies the identical guards.
//!
//! # Reverts are strings, not typed errors
//!
//! A `pallet-revive` precompile can only fail in three ways ([`Error::Revert`] encoding
//! `Error(string)`, [`Error::Panic`], or a bare trap): the framework exposes no path for a
//! precompile to return a typed Solidity custom error, because there is no `Error` variant that
//! carries a raw selector and payload. Precompiles therefore report caller-correctable failures
//! as string reverts through [`revert`], even though hand-written Solidity would prefer a typed
//! `error`. This is a framework constraint, not a style choice; revisit it only if a future
//! `pallet-revive` adds a raw-revert variant.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use frame_support::traits::Get;
use pallet_revive::{
	precompiles::{
		alloy::{primitives::Address, sol_types::Revert},
		AddressMapper, Error, Ext,
	},
	sp_runtime::Weight,
};

/// Reason returned when a caller attaches native value to a non-payable precompile.
pub const ERR_VALUE_NOT_ACCEPTED: &str = "this precompile does not accept value";
/// Reason returned when the EVM caller does not resolve to a substrate account.
pub const ERR_INVALID_CALLER: &str = "invalid caller";

/// Proof size charged per storage read.
///
/// `DbWeight` carries only `ref_time`, but on a parachain every read also pulls trie nodes into
/// the proof. The values these precompiles read are bounded far below this headroom; a crate
/// benchmark can replace the estimate.
pub const PROOF_SIZE_PER_READ: u64 = 4 * 1024;

/// Build a catchable revert carrying `reason` as its `Error(string)` payload.
///
/// This is the only failure shape a precompile can hand back to a Solidity `try`/`catch`; see the
/// module documentation on why typed custom errors are unavailable.
pub fn revert(reason: &str) -> Error {
	Error::Revert(Revert { reason: reason.into() })
}

/// Reject a call that reached the precompile through a delegate call.
///
/// A precompile executes with the delegator's address and storage under delegate call, so any
/// precompile that derives identity or a collection id from its own address must refuse it. The
/// rejection is a revert rather than a trap, mirroring `pallet-revive`'s own precompiles, so a
/// mistaken caller keeps its forwarded gas. Call this before charging weight or reading state.
pub fn ensure_not_delegate<T: pallet_revive::Config>(env: &impl Ext<T = T>) -> Result<(), Error> {
	if env.is_delegate_call() {
		return Err(Error::try_to_revert::<T>(
			pallet_revive::Error::<T>::PrecompileDelegateDenied.into(),
		));
	}
	Ok(())
}

/// Reject a call carrying native value.
///
/// These precompiles are all ABI-`nonpayable`, and their addresses have no owner, code or
/// withdrawal path, so attached value would be stranded. Rejecting is a revert because it is a
/// caller mistake the caller can correct.
pub fn ensure_no_value<T: pallet_revive::Config>(env: &impl Ext<T = T>) -> Result<(), Error> {
	if env.value_transferred().is_zero() {
		return Ok(());
	}
	Err(revert(ERR_VALUE_NOT_ACCEPTED))
}

/// Resolve the substrate account behind the EVM caller.
///
/// Reverts with [`ERR_INVALID_CALLER`] when the caller has no mapped account id.
pub fn caller_account<T: pallet_revive::Config>(
	env: &mut impl Ext<T = T>,
) -> Result<T::AccountId, Error> {
	env.caller().account_id().cloned().map_err(|_| revert(ERR_INVALID_CALLER))
}

/// The EVM address of a substrate account under the runtime's address mapper.
pub fn address_of<T: pallet_revive::Config>(account: &T::AccountId) -> Address {
	Address::from(<T as pallet_revive::Config>::AddressMapper::to_address(account).0)
}

/// Charge `n` worst-case database reads before performing them.
///
/// Prices each read at one `DbWeight` read plus [`PROOF_SIZE_PER_READ`] of proof size.
pub fn charge_reads<T: pallet_revive::Config>(
	env: &mut impl Ext<T = T>,
	n: u64,
) -> Result<(), Error> {
	let ref_time = <T as frame_system::Config>::DbWeight::get().reads(n).ref_time();
	env.charge(Weight::from_parts(ref_time, n.saturating_mul(PROOF_SIZE_PER_READ)))?;
	Ok(())
}

/// Runtime-agnostic helpers shared by the precompile crates' test mocks. Enabled through the
/// `test-helpers` feature, which the precompile crates turn on as a dev-dependency.
#[cfg(feature = "test-helpers")]
pub mod test_helpers {
	use frame_support::{
		parameter_types,
		traits::{Currency, UnixTime},
	};
	use pallet_revive::precompiles::{alloy::primitives::Address, AddressMapper, Precompile, H160};
	use sp_runtime::AccountId32;

	/// A test account whose first eight bytes encode `id`.
	pub fn id_to_account(id: u64) -> AccountId32 {
		let mut bytes = [0u8; 32];
		bytes[..8].copy_from_slice(&id.to_le_bytes());
		AccountId32::new(bytes)
	}

	/// Fund `account` and register its H160-to-AccountId32 mapping with `pallet-revive`.
	pub fn map_account<T>(account: &AccountId32)
	where
		T: pallet_revive::Config
			+ pallet_balances::Config
			+ frame_system::Config<AccountId = AccountId32>,
		pallet_balances::Pallet<T>: Currency<AccountId32, Balance = u64>,
	{
		pallet_balances::Pallet::<T>::make_free_balance_be(account, u64::MAX / 2);
		let _ = <T as pallet_revive::Config>::AddressMapper::map(account);
	}

	parameter_types! {
		/// Test-controlled Unix time, in seconds.
		pub static MockNow: u64 = 0;
	}

	/// Unix time source reading [`MockNow`], for a mock's `UnixTime` config.
	pub struct MockUnixTime;
	impl UnixTime for MockUnixTime {
		fn now() -> core::time::Duration {
			core::time::Duration::from_secs(MockNow::get())
		}
	}

	/// The base address of a precompile, derived from its own `AddressMatcher`.
	pub fn precompile_address<P: Precompile>() -> H160 {
		H160(P::MATCHER.base_address())
	}

	/// The same 20-byte address as an alloy [`Address`], for encoding ABI call arguments.
	pub fn alloy_address(address: H160) -> Address {
		Address::from(address.0)
	}
}
