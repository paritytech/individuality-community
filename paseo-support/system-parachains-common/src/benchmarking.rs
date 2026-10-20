// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0

//! Setup for the most expensive stored XCM authorization lookup.

extern crate alloc;

use alloc::boxed::Box;
use codec::{Encode, MaxEncodedLen};
use frame_support::{
	assert_ok,
	traits::{Currency, Get},
};
use frame_system::RawOrigin;
use xcm::latest::{Junction::AccountId32, Location, NetworkId};

fn max_sized_aliaser(discriminator: [u8; 32]) -> Location {
	let network = Some(NetworkId::ByFork { block_number: u64::MAX, block_hash: [255; 32] });
	let shared = AccountId32 { network, id: [255; 32] };
	let last = AccountId32 { network, id: discriminator };
	let location =
		Location::new(255, [shared, shared, shared, shared, shared, shared, shared, last]);
	assert_eq!(location.encoded_size(), Location::max_encoded_len());
	location
}

/// Fills the authorization list with maximum-sized locations and matches its final entry.
/// Locations differ at the final junction and bypass the cheap alias filters.
/// The matching entry has an expiry so the lookup also checks the current block.
pub fn set_up_worst_case_authorized_alias<Runtime>() -> (Location, Location)
where
	Runtime: pallet_xcm::Config + pallet_balances::Config,
	<Runtime as frame_system::Config>::AccountId: From<[u8; 32]>,
{
	let target_id = [42; 32];
	let account: <Runtime as frame_system::Config>::AccountId = target_id.into();
	let target = Location::new(0, AccountId32 { network: None, id: target_id });
	let balance =
		<Runtime as pallet_balances::Config>::ExistentialDeposit::get() * 1_000_000u32.into();
	let _ = pallet_balances::Pallet::<Runtime>::make_free_balance_be(&account, balance);
	let target_origin: <Runtime as frame_system::Config>::RuntimeOrigin =
		RawOrigin::Signed(account).into();
	let origin = max_sized_aliaser([170; 32]);
	for index in 1..pallet_xcm::MaxAuthorizedAliases::get() {
		let mut id = [0; 32];
		id[..4].copy_from_slice(&index.to_le_bytes());
		assert_ok!(pallet_xcm::Pallet::<Runtime>::add_authorized_alias(
			target_origin.clone(),
			Box::new(max_sized_aliaser(id).into()),
			Some(u64::MAX)
		));
	}
	assert_ok!(pallet_xcm::Pallet::<Runtime>::add_authorized_alias(
		target_origin,
		Box::new(origin.clone().into()),
		Some(u64::MAX)
	));
	(origin, target)
}
