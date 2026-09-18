// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0

//! Setup for the most expensive stored XCM authorization lookup.

use crate::{AccountId, Balance, Balances, PolkadotXcm, RuntimeOrigin};
use alloc::boxed::Box;
use codec::{Encode, MaxEncodedLen};
use frame_support::{assert_ok, traits::Currency};
use xcm::latest::prelude::*;

fn max_sized_aliaser(index: u32) -> Location {
	let network = Some(NetworkId::ByFork { block_number: u64::MAX, block_hash: [255; 32] });
	let shared = AccountId32 { network, id: [255; 32] };
	let mut id = [0; 32];
	id[..4].copy_from_slice(&index.to_le_bytes());
	let last = AccountId32 { network, id };
	let location =
		Location::new(255, [shared, shared, shared, shared, shared, shared, shared, last]);
	assert_eq!(location.encoded_size(), Location::max_encoded_len());
	location
}

/// Fills the authorization list with maximum-sized locations and matches its final entry.
/// All entries require an expiry check and bypass the cheap alias filters.
pub fn set_up_worst_case_authorized_alias() -> (Location, Location) {
	let account: AccountId = [42; 32].into();
	let target = Location::new(0, AccountId32 { network: None, id: account.clone().into() });
	let _ = Balances::make_free_balance_be(&account, Balance::MAX / 2);
	let count = pallet_xcm::MaxAuthorizedAliases::get();
	for index in 0..count {
		assert_ok!(PolkadotXcm::add_authorized_alias(
			RuntimeOrigin::signed(account.clone()),
			Box::new(max_sized_aliaser(index).into()),
			Some(u64::MAX)
		));
	}
	(max_sized_aliaser(count - 1), target)
}
