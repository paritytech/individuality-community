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

//! The receiving end of a credit-root deletion. These tests cover the call the claims chain
//! addresses and the origin it has to arrive under.

use super::*;
use codec::{Compact, Decode};
use frame_support::{assert_noop, assert_ok, BoundedVec};
use indiv_pallet_nft_credits::{NftClaimCreditRoots, RootExpiries};
use indiv_support::credit_trees::{CreditProofNode, ExpiryTimestamp, NftClaimCreditTree};
use paseo_runtime_constants::system_parachain::NextAssetHubParaId;
use sp_runtime::DispatchError;

const TIMESTAMP: u32 = 1_000_000;

const BLOCK: BlockNumber = 42;

/// Records a root for [`BLOCK`], as `build_credit_tree` does once a block has awarded credits.
fn record_root() {
	indiv_pallet_nft_credits::Pallet::<Runtime>::record_credit_root_for_tests(
		BLOCK,
		NftClaimCreditTree {
			game_index: 0,
			root: CreditProofNode([1u8; 32]),
			leaf_count: 3,
			timestamp: TIMESTAMP,
		},
	);
}

fn claims_chain_origin() -> RuntimeOrigin {
	cumulus_pallet_xcm::Origin::SiblingParachain(NextAssetHubParaId::get()).into()
}

/// The bytes the claims chain builds its `Transact` from name a pallet and a call by index only.
/// Decoding them here holds its `GameChainPalletIndex` and call index to this runtime's own
/// dispatchable.
#[test]
fn the_claims_chains_encoded_call_is_this_runtimes_deletion_call() {
	let encoded = (57u8, 20u8, Compact(1u32), BLOCK).encode();

	let call = RuntimeCall::decode(&mut &encoded[..]).expect("the claims chain's call decodes");

	assert_eq!(
		call,
		RuntimeCall::NftCredits(indiv_pallet_nft_credits::Call::receive_tree_deletions {
			blocks: BoundedVec::truncate_from(vec![BLOCK]),
		}),
	);
}

#[test]
fn the_claims_chain_deletes_a_root_and_its_expiry_entry() {
	new_test_ext().execute_with(|| {
		record_root();

		assert_ok!(indiv_pallet_nft_credits::Pallet::<Runtime>::receive_tree_deletions(
			claims_chain_origin(),
			BoundedVec::truncate_from(vec![BLOCK])
		));

		assert!(!NftClaimCreditRoots::<Runtime>::contains_key(BLOCK));
		assert!(!RootExpiries::<Runtime>::contains_key(ExpiryTimestamp::from(TIMESTAMP), BLOCK));
	});
}

#[test]
fn another_sibling_cannot_delete_a_root() {
	new_test_ext().execute_with(|| {
		record_root();

		// This refuses another sibling and the relay chain. Either one could otherwise strand a
		// credit that is still inside its claim deadline.
		assert_noop!(
			indiv_pallet_nft_credits::Pallet::<Runtime>::receive_tree_deletions(
				cumulus_pallet_xcm::Origin::SiblingParachain(
					(u32::from(NextAssetHubParaId::get()) + 1).into()
				)
				.into(),
				BoundedVec::truncate_from(vec![BLOCK])
			),
			DispatchError::BadOrigin
		);
		assert_noop!(
			indiv_pallet_nft_credits::Pallet::<Runtime>::receive_tree_deletions(
				RuntimeOrigin::root(),
				BoundedVec::truncate_from(vec![BLOCK])
			),
			DispatchError::BadOrigin
		);

		assert!(NftClaimCreditRoots::<Runtime>::contains_key(BLOCK));
	});
}
