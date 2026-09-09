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

//! Tests for the nft-claims pallet.

use crate::{
	mock::*, ClaimantKind, Config, CreditTrees, Event, NextExpectedSequence, PendingTreeDeletions,
	TreeExpiries, WeightInfo,
};
use frame_support::{assert_noop, assert_ok, dispatch::GetDispatchInfo, BoundedVec};
use indiv_support::credit_trees::{
	AwardBlock, CreditProofNode, CreditTreeDelivery, ExpiryTimestamp, NftClaimCreditTree,
};
use sp_runtime::DispatchError;

#[test]
fn receive_credit_trees_stores_the_batch() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(0, 10), update(1, 14)])
		));

		assert_eq!(CreditTrees::<Test>::get(10), Some(tree(10)));
		assert_eq!(CreditTrees::<Test>::get(14), Some(tree(14)));
		assert_eq!(NextExpectedSequence::<Test>::get(), 2);
		assert_eq!(nft_claims_events(), vec![Event::CreditTreesReceived { count: 2, stored: 2 }]);
	});
}

#[test]
fn a_stored_tree_is_readable_by_the_claim() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(0, 10)])
		));

		assert_eq!(NftClaims::credit_tree(10), Some(tree(10)));
		assert_eq!(NftClaims::credit_tree(11), None);
	});
}

#[test]
fn receive_credit_trees_rejects_a_foreign_origin() {
	new_test_ext().execute_with(|| {
		assert_noop!(
			NftClaims::receive_credit_trees(
				RuntimeOrigin::signed(GAME_CHAIN + 1),
				batch(vec![update(0, 10)])
			),
			DispatchError::BadOrigin
		);
		assert_noop!(
			NftClaims::receive_credit_trees(RuntimeOrigin::root(), batch(vec![update(0, 10)])),
			DispatchError::BadOrigin
		);

		assert!(!CreditTrees::<Test>::contains_key(10));
	});
}

#[test]
fn a_replayed_tree_that_is_already_stored_changes_nothing() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(0, 10)])
		));
		System::reset_events();

		assert_ok!(NftClaims::receive_credit_trees(game_chain_origin(), batch(vec![replay(10)])));

		assert_eq!(CreditTrees::<Test>::get(10), Some(tree(10)));
		assert_eq!(NextExpectedSequence::<Test>::get(), 1);
		assert_eq!(nft_claims_events(), vec![Event::CreditTreesReceived { count: 1, stored: 0 }]);
	});
}

#[test]
fn a_conflicting_root_keeps_the_stored_tree() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(0, 10)])
		));
		System::reset_events();

		let conflicting = NftClaimCreditTree { root: CreditProofNode([0xff; 32]), ..tree(10) };
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![CreditTreeDelivery { sequence: None, block: 10, tree: conflicting }])
		));

		assert_eq!(CreditTrees::<Test>::get(10), Some(tree(10)));
		assert_eq!(
			nft_claims_events(),
			vec![
				Event::CreditTreeConflict { block: 10 },
				Event::CreditTreesReceived { count: 1, stored: 0 },
			]
		);
	});
}

#[test]
fn an_empty_or_zero_rooted_tree_is_skipped() {
	new_test_ext().execute_with(|| {
		let empty = NftClaimCreditTree { leaf_count: 0, ..tree(10) };
		let zero_rooted = NftClaimCreditTree { root: CreditProofNode([0u8; 32]), ..tree(11) };
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![
				CreditTreeDelivery { sequence: Some(0), block: 10, tree: empty },
				CreditTreeDelivery { sequence: Some(1), block: 11, tree: zero_rooted },
				update(2, 12),
			])
		));

		assert!(!CreditTrees::<Test>::contains_key(10));
		assert!(!CreditTrees::<Test>::contains_key(11));
		assert_eq!(CreditTrees::<Test>::get(12), Some(tree(12)));
		assert_eq!(nft_claims_events(), vec![Event::CreditTreesReceived { count: 3, stored: 1 }]);
	});
}

#[test]
fn a_sequence_gap_is_reported() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(0, 10)])
		));
		System::reset_events();

		// Sequences 1 and 2 were lost on the way.
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(3, 20), update(4, 21)])
		));

		assert_eq!(NextExpectedSequence::<Test>::get(), 5);
		assert_eq!(
			nft_claims_events(),
			vec![
				Event::CreditTreesMissing { from_sequence: 1, to_sequence: 2 },
				Event::CreditTreesReceived { count: 2, stored: 2 },
			]
		);
	});
}

#[test]
fn a_contiguous_stream_reports_no_gap() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(0, 10), update(1, 11)])
		));
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(2, 12)])
		));

		assert_eq!(NextExpectedSequence::<Test>::get(), 3);
		assert!(!nft_claims_events()
			.iter()
			.any(|event| matches!(event, Event::CreditTreesMissing { .. })));
	});
}

#[test]
fn a_replay_only_batch_leaves_the_expected_sequence_alone() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(0, 10), update(1, 11)])
		));

		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![replay(30), replay(31)])
		));

		assert_eq!(NextExpectedSequence::<Test>::get(), 2);
		assert_eq!(CreditTrees::<Test>::get(30), Some(tree(30)));
		assert!(!nft_claims_events()
			.iter()
			.any(|event| matches!(event, Event::CreditTreesMissing { .. })));
	});
}

#[test]
fn a_late_batch_below_the_expected_sequence_stores_but_reports_no_gap() {
	new_test_ext().execute_with(|| {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(5, 20)])
		));
		System::reset_events();

		// A sequenced tree that arrives out of order must not rewind the expectation.
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![update(2, 15)])
		));

		assert_eq!(NextExpectedSequence::<Test>::get(), 6);
		assert_eq!(CreditTrees::<Test>::get(15), Some(tree(15)));
		assert_eq!(nft_claims_events(), vec![Event::CreditTreesReceived { count: 1, stored: 1 }]);
	});
}

mod claim {
	use super::*;
	use crate::{
		runtime_api::{
			BatchError, PreviewFailure, PreviewOutcome, PreviewQuery, SelectionKind,
			MAX_PREVIEW_QUERIES,
		},
		ClaimedLeaves, CollectionMinter, CollectionMinters, Error, ItemSelection,
	};
	use indiv_pallet_scarcity::CollectionId;
	use indiv_support::{credit_trees::credit_leaf, identity::AccountOrPerson};
	use sp_core::H160;

	/// Like [`assert_noop!`], but for `claim`: asserts the error kind and that no storage
	/// changed, ignoring the `actual_weight` the selector-reservation refund attaches to every
	/// failure. The refunded weight itself is asserted in
	/// [`a_failed_claim_refunds_the_selector_reservation_to_the_branch`].
	macro_rules! assert_claim_noop {
		($call:expr, $err:expr $(,)?) => {{
			let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
			assert_eq!($call.map(|_| ()).map_err(|e| e.error), Err($err.into()));
			assert_eq!(
				root,
				sp_io::storage::root(sp_runtime::StateVersion::V1),
				"storage has been mutated"
			);
		}};
	}

	const ALICE: u64 = 1;
	const BOB: u64 = 2;
	const PURSE: u64 = 100;

	/// The block whose tree the tests claim against.
	const BLOCK: AwardBlock = 10;

	/// The collection the tests mint into.
	const COLLECTION: CollectionId = 3;
	/// The account owning [`COLLECTION`].
	const COLLECTION_OWNER: u64 = 50;
	/// The minter contract of [`COLLECTION`] when it is registered with a contract selection.
	const CONTRACT: H160 = H160::repeat_byte(0xcd);

	/// Make [`COLLECTION`] exist with two items and register it for claims with `selection`.
	///
	/// Two items keep the [`ItemSelection::Random`] draw meaningful: the item minted for a
	/// credit is its first four bytes modulo two.
	fn register_collection(selection: ItemSelection) {
		register_named_collection(COLLECTION, 2, selection);
	}

	/// Make a collection exist with `next_item_index` and register its claim selection.
	fn register_named_collection(
		collection: CollectionId,
		next_item_index: u32,
		selection: ItemSelection,
	) {
		add_collection(collection, COLLECTION_OWNER, next_item_index);
		CollectionMinters::<Test>::insert(
			collection,
			CollectionMinter { owner: COLLECTION_OWNER, selection },
		);
	}

	/// Three awards in one block: two of Alice's and one of the person's, in award order.
	fn awards() -> Vec<Award> {
		vec![
			(AccountOrPerson::Account(ALICE), [1u8; 32]),
			(AccountOrPerson::Person(PERSON_ALIAS), [2u8; 32]),
			(AccountOrPerson::Account(ALICE), [3u8; 32]),
		]
	}

	/// Stores the tree of [`awards`] under [`BLOCK`] with its expiry entry, as a delivery from the
	/// game chain does. Also registers [`COLLECTION`] for claims with [`ItemSelection::Random`].
	fn store_tree(awards: &[Award]) {
		let tree = tree_of(BLOCK, awards);
		CreditTrees::<Test>::insert(BLOCK, tree);
		TreeExpiries::<Test>::insert(ExpiryTimestamp::from(tree.timestamp), BLOCK, ());
		register_collection(ItemSelection::Random);
	}

	#[test]
	fn an_account_claims_its_credit() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));

			let leaf = credit_leaf(&AccountOrPerson::Account(ALICE), &[1u8; 32]);
			assert!(leaf_is_claimed(BLOCK, 0));
			assert_eq!(claimed_leaves(BLOCK), 1);
			assert_eq!(MintedInstances::get(), vec![(3, 1, PURSE)]);
			assert_eq!(
				nft_claims_events(),
				vec![Event::CreditClaimed {
					block: BLOCK,
					leaf,
					collection: COLLECTION,
					item: 1,
					owner: PURSE,
					instance: 1
				}]
			);
		});
	}

	#[test]
	fn a_person_claims_the_credit_of_their_alias() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(PERSON),
				ClaimantKind::Person,
				BLOCK,
				[2u8; 32],
				1,
				proof_of(&awards, 1),
				COLLECTION,
				PURSE
			));

			assert!(leaf_is_claimed(BLOCK, 1));
		});
	}

	/// The alias lookup a person claimant needs is charged only to that kind.
	#[test]
	fn the_declared_weight_follows_the_claimant_kind() {
		let weight_of = |claimant| {
			crate::Call::<Test>::claim {
				claimant,
				block: BLOCK,
				credit: [1u8; 32],
				leaf_index: 0,
				proof: BoundedVec::truncate_from(vec![CreditProofNode([7u8; 32]); 4]),
				collection: COLLECTION,
				mint_to: PURSE,
			}
			.get_dispatch_info()
			.call_weight
		};

		// Both kinds are charged the draining branch, and reserve the selector's ceiling and the
		// mint hooks on top of it.
		assert_eq!(
			weight_of(ClaimantKind::Account),
			<Test as Config>::WeightInfo::claim_last_account(4)
				.saturating_add(SELECTOR_MAX_WEIGHT)
				.saturating_add(MINT_HOOK_WEIGHT)
		);
		assert_eq!(
			weight_of(ClaimantKind::Person),
			<Test as Config>::WeightInfo::claim_last_person(4)
				.saturating_add(SELECTOR_MAX_WEIGHT)
				.saturating_add(MINT_HOOK_WEIGHT)
		);
		assert!(weight_of(ClaimantKind::Account).all_lt(weight_of(ClaimantKind::Person)));
	}

	#[test]
	fn every_leaf_of_a_tree_can_be_claimed_and_the_last_reports_the_tree_done() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(PERSON),
				ClaimantKind::Person,
				BLOCK,
				[2u8; 32],
				1,
				proof_of(&awards, 1),
				COLLECTION,
				PURSE + 1
			));
			System::reset_events();
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[3u8; 32],
				2,
				proof_of(&awards, 2),
				COLLECTION,
				PURSE + 2
			));

			// The claimed leaves reached the tree's `leaf_count`, which removed the tree.
			assert_eq!(claimed_leaves(BLOCK), 3);
			assert!(!CreditTrees::<Test>::contains_key(BLOCK));
			assert!(nft_claims_events().contains(&Event::TreeFullyClaimed { block: BLOCK }));
		});
	}

	#[test]
	fn the_last_claim_of_a_tree_removes_it_and_queues_its_deletion() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			let tree = tree_of(BLOCK, &awards);
			store_tree(&awards);

			for (leaf_index, (claimant, credit)) in awards.iter().enumerate() {
				let origin = match claimant {
					AccountOrPerson::Person(_) => RuntimeOrigin::signed(PERSON),
					AccountOrPerson::Account(who) => RuntimeOrigin::signed(*who),
				};
				let kind = match claimant {
					AccountOrPerson::Person(_) => ClaimantKind::Person,
					AccountOrPerson::Account(_) => ClaimantKind::Account,
				};
				assert_ok!(NftClaims::claim(
					origin,
					kind,
					BLOCK,
					*credit,
					leaf_index as u32,
					proof_of(&awards, leaf_index as u32),
					COLLECTION,
					PURSE + leaf_index as u64
				));
			}

			assert!(!CreditTrees::<Test>::contains_key(BLOCK), "the fully claimed tree is removed");
			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![BLOCK]);

			// The spent leaves outlive the tree, so a replay that puts it back mints nothing. The
			// expiry entry stays with them, and its sweep is what removes them at the deadline.
			for leaf_index in 0..awards.len() as u32 {
				assert!(leaf_is_claimed(BLOCK, leaf_index));
			}
			assert!(TreeExpiries::<Test>::contains_key(
				ExpiryTimestamp::from(tree.timestamp),
				BLOCK
			));
		});
	}

	#[test]
	fn a_sweep_drops_a_partly_claimed_trees_count_and_keeps_its_leaves() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			let tree = tree_of(BLOCK, &awards);
			store_tree(&awards);
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_eq!(claimed_leaves(BLOCK), 1);

			set_now(due_at(tree.timestamp));
			assert_ok!(NftClaims::sweep_expired_trees(
				RuntimeOrigin::from(frame_system::RawOrigin::Authorized),
				tree.timestamp,
				1
			));

			assert!(!CreditTrees::<Test>::contains_key(BLOCK), "the expired tree is removed");
			// The deadline has passed, so no replay delivers the tree again and the bitmap goes
			// with it.
			assert!(!ClaimedLeaves::<Test>::contains_key(BLOCK));
		});
	}

	#[test]
	fn a_leaf_past_the_first_byte_of_the_bitmap_claims() {
		new_test_ext().execute_with(|| {
			// Nine awards, so leaf eight sits in the bitmap's second byte.
			let awards = (1..=9u8)
				.map(|i| (AccountOrPerson::Account(ALICE), [i; 32]))
				.collect::<Vec<_>>();
			store_tree(&awards);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[9u8; 32],
				8,
				proof_of(&awards, 8),
				COLLECTION,
				PURSE
			));

			assert!(leaf_is_claimed(BLOCK, 8));
			assert_eq!(claimed_leaves(BLOCK), 1);
			assert!(!leaf_is_claimed(BLOCK, 0), "the first byte is untouched");
		});
	}

	#[test]
	fn the_sweep_of_a_fully_claimed_block_drops_its_leaves_and_sends_no_second_deletion() {
		new_test_ext().execute_with(|| {
			// A one-leaf tree, so the first claim is its last and removes the tree.
			let awards = vec![(AccountOrPerson::Account(ALICE), [1u8; 32])];
			let tree = tree_of(BLOCK, &awards);
			store_tree(&awards);
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![BLOCK]);
			System::reset_events();

			set_now(due_at(tree.timestamp));
			assert_ok!(NftClaims::sweep_expired_trees(
				RuntimeOrigin::from(frame_system::RawOrigin::Authorized),
				tree.timestamp,
				1
			));

			assert!(!ClaimedLeaves::<Test>::contains_key(BLOCK));
			assert_eq!(
				PendingTreeDeletions::<Test>::get().to_vec(),
				vec![BLOCK],
				"the deletion was queued by the claim, and the sweep holds no tree to queue again"
			);
			assert!(!nft_claims_events().contains(&Event::CreditTreesExpired { count: 1 }));
		});
	}

	#[test]
	fn a_claim_that_leaves_credits_behind_is_refunded_to_the_cheaper_branch() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			let proof = proof_of(&awards, 0);
			let charged = crate::Call::<Test>::claim {
				claimant: ClaimantKind::Account,
				block: BLOCK,
				credit: [1u8; 32],
				leaf_index: 0,
				proof: proof.clone(),
				collection: COLLECTION,
				mint_to: PURSE,
			}
			.get_dispatch_info()
			.call_weight;

			let nodes = proof.len() as u32;
			let post = NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof,
				COLLECTION,
				PURSE,
			)
			.expect("the claim goes through");

			// The mint ran, so this branch pays for its runtime hooks as well.
			let actual = post.actual_weight.expect("a partial claim reports its weight");
			assert_eq!(
				actual,
				<MockWeightInfo as crate::WeightInfo>::claim_account(nodes)
					.saturating_add(MINT_HOOK_WEIGHT)
			);
			assert!(actual.all_lt(charged), "the refund is below what a draining claim is charged");
		});
	}

	#[test]
	fn the_last_claim_of_a_tree_pays_the_draining_branch() {
		new_test_ext().execute_with(|| {
			// A one-leaf tree, so the first claim is also the last.
			let awards = vec![(AccountOrPerson::Account(ALICE), [1u8; 32])];
			store_tree(&awards);
			let proof = proof_of(&awards, 0);
			let nodes = proof.len() as u32;
			let charged = crate::Call::<Test>::claim {
				claimant: ClaimantKind::Account,
				block: BLOCK,
				credit: [1u8; 32],
				leaf_index: 0,
				proof: proof.clone(),
				collection: COLLECTION,
				mint_to: PURSE,
			}
			.get_dispatch_info()
			.call_weight;

			let post = NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof,
				COLLECTION,
				PURSE,
			)
			.expect("the claim goes through");

			// A random selection consumes nothing, so the refund covers the selector's reservation
			// only. The draining branch itself is charged in full.
			let actual = post.actual_weight.expect("a claim reports its weight");
			assert_eq!(
				actual,
				<MockWeightInfo as crate::WeightInfo>::claim_last_account(nodes)
					.saturating_add(MINT_HOOK_WEIGHT)
			);
			assert!(actual.all_lt(charged), "the selector's reservation is refunded");
		});
	}

	#[test]
	fn a_credit_is_claimed_once() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE + 1
				),
				Error::<Test>::AlreadyClaimed
			);
			assert_eq!(claimed_leaves(BLOCK), 1);
		});
	}

	#[test]
	fn a_claim_of_another_claimants_credit_proves_nothing() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			// Bob presents Alice's credit, with the proof of her leaf: his own leaf is not in the
			// tree, so the proof cannot rehash to the root.
			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(BOB),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::InvalidProof
			);
			// So does the person whose alias holds a different credit of the same tree.
			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(PERSON),
					ClaimantKind::Person,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::InvalidProof
			);
			assert!(MintedInstances::get().is_empty());
		});
	}

	#[test]
	fn a_claim_at_the_wrong_leaf_index_or_with_a_foreign_proof_fails() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			// Right credit, wrong position in the tree.
			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					2,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::InvalidProof
			);
			// Right position, but the proof belongs to another leaf.
			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 2),
					COLLECTION,
					PURSE
				),
				Error::<Test>::InvalidProof
			);
		});
	}

	#[test]
	fn a_claim_beyond_the_trees_leaves_fails() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					3,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::LeafIndexOutOfBounds
			);
		});
	}

	#[test]
	fn a_claim_against_a_tree_that_has_not_arrived_fails() {
		new_test_ext().execute_with(|| {
			let awards = awards();

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::UnknownAwardBlock
			);
		});
	}

	#[test]
	fn a_claim_against_another_blocks_root_fails() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			// A different set of awards is committed under the block being claimed against, so
			// the proof is well-formed but rehashes to a root this block does not hold.
			store_tree(&[(AccountOrPerson::Account(BOB), [7u8; 32])]);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::InvalidProof
			);
		});
	}

	#[test]
	fn a_claim_rejects_an_origin_that_is_neither_an_account_nor_a_person() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::root(),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn a_person_claiming_under_their_account_proves_nothing() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			// The credit is the person's, but the account the signer resolves to under
			// `ClaimantKind::Account` hashes into a leaf that is in no tree.
			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(PERSON),
					ClaimantKind::Account,
					BLOCK,
					[2u8; 32],
					1,
					proof_of(&awards, 1),
					COLLECTION,
					PURSE
				),
				Error::<Test>::InvalidProof
			);
			assert!(MintedInstances::get().is_empty());
		});
	}

	#[test]
	fn a_signer_with_no_alias_binding_cannot_claim_as_a_person() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Person,
					BLOCK,
					[2u8; 32],
					1,
					proof_of(&awards, 1),
					COLLECTION,
					PURSE
				),
				DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn a_failing_mint_leaves_the_credit_unspent() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));

			// The purse key already holds an NFT, so the mint fails and the second credit stays
			// claimable at another key.
			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[3u8; 32],
					2,
					proof_of(&awards, 2),
					COLLECTION,
					PURSE
				),
				DispatchError::Other("AddressOccupied")
			);
			assert_eq!(claimed_leaves(BLOCK), 1);
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[3u8; 32],
				2,
				proof_of(&awards, 2),
				COLLECTION,
				PURSE + 1
			));
		});
	}

	#[test]
	fn a_claim_into_an_unregistered_collection_fails() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			CollectionMinters::<Test>::remove(COLLECTION);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::CollectionNotRegistered
			);
			assert!(MintedInstances::get().is_empty());
			assert_eq!(claimed_leaves(BLOCK), 0);
		});
	}

	#[test]
	fn a_new_collection_owner_must_register_before_claims_resume() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			let new_owner = COLLECTION_OWNER + 1;
			add_collection(COLLECTION, new_owner, 2);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::CollectionOwnerChanged
			);
			assert!(MintedInstances::get().is_empty());
			assert_eq!(claimed_leaves(BLOCK), 0);

			assert_ok!(NftClaims::set_collection_minter(
				RuntimeOrigin::signed(new_owner),
				COLLECTION,
				Some(ItemSelection::Random)
			));
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_eq!(MintedInstances::get(), vec![(COLLECTION, 1, PURSE)]);
		});
	}

	#[test]
	fn a_random_selection_draws_the_item_from_the_credit() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			// The draw is the credit's first four bytes modulo the two items: `[1u8; 32]` is
			// odd, `[2u8; 32]` even.
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(PERSON),
				ClaimantKind::Person,
				BLOCK,
				[2u8; 32],
				1,
				proof_of(&awards, 1),
				COLLECTION,
				PURSE + 1
			));

			assert_eq!(
				MintedInstances::get(),
				vec![(COLLECTION, 1, PURSE), (COLLECTION, 0, PURSE + 1)]
			);
			assert!(SelectorCalls::get().is_empty());
		});
	}

	#[test]
	fn preview_and_claim_share_random_pure_and_stateful_selection() {
		new_test_ext().execute_with(|| {
			const PURE_COLLECTION: CollectionId = 4;
			const STATEFUL_COLLECTION: CollectionId = 5;
			const PURE_CONTRACT: H160 = H160::repeat_byte(0xaa);
			const STATEFUL_CONTRACT: H160 = H160::repeat_byte(0xbb);

			let awards = awards();
			CreditTrees::<Test>::insert(BLOCK, tree_of(BLOCK, &awards));
			register_named_collection(COLLECTION, 4, ItemSelection::Random);
			register_named_collection(PURE_COLLECTION, 8, ItemSelection::Contract(PURE_CONTRACT));
			register_named_collection(
				STATEFUL_COLLECTION,
				8,
				ItemSelection::Contract(STATEFUL_CONTRACT),
			);
			SelectorItem::set(&5);
			StatefulSelectorContract::set(&Some(STATEFUL_CONTRACT));
			StatefulSelectorItem::set(&2);

			let preview = frame_support::storage::with_transaction(|| {
				frame_support::storage::TransactionOutcome::Rollback(
					Result::<_, DispatchError>::Ok(NftClaims::preview_mints(vec![
						PreviewQuery { credit: [1u8; 32], collection: COLLECTION },
						PreviewQuery { credit: [2u8; 32], collection: PURE_COLLECTION },
						PreviewQuery { credit: [3u8; 32], collection: STATEFUL_COLLECTION },
					])),
				)
			})
			.expect("the preview transaction starts")
			.expect("three queries fit the preview cap");
			assert_eq!(
				preview,
				vec![
					PreviewOutcome::Mints { item: 1, via: SelectionKind::Random },
					PreviewOutcome::Mints { item: 5, via: SelectionKind::Contract(PURE_CONTRACT) },
					PreviewOutcome::Mints {
						item: 2,
						via: SelectionKind::Contract(STATEFUL_CONTRACT),
					},
				]
			);
			assert_eq!(StatefulSelectorItem::get(), 2);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(PERSON),
				ClaimantKind::Person,
				BLOCK,
				[2u8; 32],
				1,
				proof_of(&awards, 1),
				PURE_COLLECTION,
				PURSE + 1
			));
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[3u8; 32],
				2,
				proof_of(&awards, 2),
				STATEFUL_COLLECTION,
				PURSE + 2
			));

			let previewed_items = preview
				.into_iter()
				.map(|outcome| match outcome {
					PreviewOutcome::Mints { item, .. } => item,
					PreviewOutcome::Fails { reason } => panic!("preview failed: {reason:?}"),
				})
				.collect::<Vec<_>>();
			let minted_items =
				MintedInstances::get().into_iter().map(|(_, item, _)| item).collect::<Vec<_>>();
			assert_eq!(previewed_items, minted_items);
		});
	}

	#[test]
	fn preview_batch_compounds_repeated_stateful_selections() {
		new_test_ext().execute_with(|| {
			const STATEFUL_COLLECTION: CollectionId = 5;
			const STATEFUL_CONTRACT: H160 = H160::repeat_byte(0xbb);

			register_named_collection(
				STATEFUL_COLLECTION,
				8,
				ItemSelection::Contract(STATEFUL_CONTRACT),
			);
			StatefulSelectorContract::set(&Some(STATEFUL_CONTRACT));
			StatefulSelectorItem::set(&2);

			let preview = frame_support::storage::with_transaction(|| {
				frame_support::storage::TransactionOutcome::Rollback(
					Result::<_, DispatchError>::Ok(NftClaims::preview_mints(vec![
						PreviewQuery { credit: [1u8; 32], collection: STATEFUL_COLLECTION },
						PreviewQuery { credit: [2u8; 32], collection: STATEFUL_COLLECTION },
					])),
				)
			})
			.expect("the preview transaction starts")
			.expect("two queries fit the preview cap");

			// The second query sees the first query's contract state, exactly as claiming in
			// that order would; the shared overlay is then discarded whole.
			assert_eq!(
				preview,
				vec![
					PreviewOutcome::Mints {
						item: 2,
						via: SelectionKind::Contract(STATEFUL_CONTRACT),
					},
					PreviewOutcome::Mints {
						item: 3,
						via: SelectionKind::Contract(STATEFUL_CONTRACT),
					},
				]
			);
			assert_eq!(StatefulSelectorItem::get(), 2);
		});
	}

	#[test]
	fn preview_reports_selection_failures_without_failing_the_batch() {
		new_test_ext().execute_with(|| {
			const UNREGISTERED: CollectionId = 10;
			const OWNER_CHANGED: CollectionId = 11;
			const DELETED_ITEM: CollectionId = 12;
			const CONTRACT_FAILURE: CollectionId = 13;
			const NO_ITEMS: CollectionId = 14;
			const DELETED_COLLECTION: CollectionId = 15;

			add_collection(UNREGISTERED, COLLECTION_OWNER, 2);
			register_named_collection(OWNER_CHANGED, 2, ItemSelection::Random);
			add_collection(OWNER_CHANGED, COLLECTION_OWNER + 1, 2);
			register_named_collection(DELETED_ITEM, 2, ItemSelection::Random);
			MissingItems::set(&vec![(DELETED_ITEM, 1)]);
			register_named_collection(CONTRACT_FAILURE, 2, ItemSelection::Contract(CONTRACT));
			register_named_collection(NO_ITEMS, 0, ItemSelection::Random);
			register_named_collection(DELETED_COLLECTION, 2, ItemSelection::Random);
			let mut collections = MockCollections::get();
			collections.retain(|(collection, _, _)| *collection != DELETED_COLLECTION);
			MockCollections::set(&collections);
			SelectorFails::set(&true);

			assert_eq!(
				NftClaims::preview_mints(vec![
					PreviewQuery { credit: [1u8; 32], collection: UNREGISTERED },
					PreviewQuery { credit: [1u8; 32], collection: OWNER_CHANGED },
					PreviewQuery { credit: [1u8; 32], collection: DELETED_ITEM },
					PreviewQuery { credit: [1u8; 32], collection: CONTRACT_FAILURE },
					PreviewQuery { credit: [1u8; 32], collection: NO_ITEMS },
					PreviewQuery { credit: [1u8; 32], collection: DELETED_COLLECTION },
				]),
				Ok(vec![
					PreviewOutcome::Fails { reason: PreviewFailure::CollectionNotRegistered },
					PreviewOutcome::Fails { reason: PreviewFailure::CollectionOwnerChanged },
					PreviewOutcome::Fails { reason: PreviewFailure::UnknownItem { item: 1 } },
					PreviewOutcome::Fails {
						reason: PreviewFailure::ContractSelectionFailed {
							error: DispatchError::Other("SelectorFailed"),
						},
					},
					PreviewOutcome::Fails { reason: PreviewFailure::NoItems },
					PreviewOutcome::Fails { reason: PreviewFailure::UnknownCollection },
				])
			);
		});
	}

	#[test]
	fn preview_rejects_an_oversized_batch_before_selection() {
		new_test_ext().execute_with(|| {
			assert_eq!(
				NftClaims::preview_mints(vec![
					PreviewQuery {
						credit: [0u8; 32],
						collection: COLLECTION
					};
					MAX_PREVIEW_QUERIES as usize + 1
				]),
				Err(BatchError::TooLarge { max: MAX_PREVIEW_QUERIES })
			);
			assert!(SelectorCalls::get().is_empty());
		});
	}

	#[test]
	fn a_random_selection_with_no_items_fails() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			add_collection(COLLECTION, COLLECTION_OWNER, 0);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::NoItems
			);
		});
	}

	#[test]
	fn a_random_selection_against_a_deleted_collection_fails() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			// The collection is gone from the backend while its registration lingers.
			MockCollections::set(&Vec::new());

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				Error::<Test>::UnknownCollection
			);
		});
	}

	#[test]
	fn a_contract_selection_asks_the_registered_contract() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			register_collection(ItemSelection::Contract(CONTRACT));
			add_collection(COLLECTION, COLLECTION_OWNER, 10);
			SelectorItem::set(&9);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));

			// The contract is called with the credit as the only entropy and the claim mints
			// exactly the item it picked.
			assert_eq!(
				SelectorCalls::get(),
				vec![(COLLECTION_OWNER, CONTRACT, COLLECTION, [1u8; 32])]
			);
			assert_eq!(MintedInstances::get(), vec![(COLLECTION, 9, PURSE)]);
		});
	}

	#[test]
	fn a_minter_reentering_with_the_same_credit_fails_already_claimed() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			register_collection(ItemSelection::Contract(CONTRACT));

			// The selector claims the same credit mid-selection, as a reentrant minter
			// contract would. The credit is spent before the selector runs, so the nested
			// claim fails and the outer claim still mints exactly once.
			SelectorReentry::set(&Some(ReentrantClaim {
				claimant: ALICE,
				kind: ClaimantKind::Account,
				block: BLOCK,
				credit: [1u8; 32],
				leaf_index: 0,
				proof: proof_of(&awards, 0).into_inner(),
				collection: COLLECTION,
				mint_to: PURSE + 1,
			}));

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));

			assert_eq!(ReentryResult::get(), Some(Err(Error::<Test>::AlreadyClaimed.into())));
			assert_eq!(claimed_leaves(BLOCK), 1);
			assert_eq!(MintedInstances::get().len(), 1);
		});
	}

	#[test]
	fn a_minter_reentering_with_another_credit_loses_neither_claims_count() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			register_collection(ItemSelection::Contract(CONTRACT));

			// The selector claims a different credit of the same block mid-selection. The
			// claimed count is read after the selector returns, so the nested claim's
			// increment is not overwritten from a stale snapshot.
			SelectorReentry::set(&Some(ReentrantClaim {
				claimant: ALICE,
				kind: ClaimantKind::Account,
				block: BLOCK,
				credit: [3u8; 32],
				leaf_index: 2,
				proof: proof_of(&awards, 2).into_inner(),
				collection: COLLECTION,
				mint_to: PURSE + 1,
			}));

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));

			assert_eq!(ReentryResult::get(), Some(Ok(())));
			assert_eq!(claimed_leaves(BLOCK), 2);
			assert_eq!(MintedInstances::get().len(), 2);
			assert!(leaf_is_claimed(BLOCK, 0));
			assert!(leaf_is_claimed(BLOCK, 2));
		});
	}

	#[test]
	fn a_failing_contract_leaves_the_credit_unspent() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			register_collection(ItemSelection::Contract(CONTRACT));
			SelectorFails::set(&true);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					proof_of(&awards, 0),
					COLLECTION,
					PURSE
				),
				DispatchError::Other("SelectorFailed")
			);

			assert!(!leaf_is_claimed(BLOCK, 0));
			assert_eq!(claimed_leaves(BLOCK), 0);
			assert!(MintedInstances::get().is_empty());

			// The gate is the contract's to lift: once it stops failing, the credit claims.
			SelectorFails::set(&false);
			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
		});
	}

	#[test]
	fn try_state_holds_after_claims_and_catches_corrupted_accounting() {
		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);
			assert_ok!(NftClaims::do_try_state());

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_ok!(NftClaims::do_try_state());

			// More claimed leaves than the tree commits to is the corruption the check is for.
			ClaimedLeaves::<Test>::insert(BLOCK, BoundedVec::truncate_from(vec![0b1111u8]));
			assert!(NftClaims::do_try_state().is_err());
		});
	}

	#[test]
	fn try_state_catches_a_registration_that_outlived_its_collection() {
		new_test_ext().execute_with(|| {
			register_collection(ItemSelection::Random);
			assert_ok!(NftClaims::do_try_state());

			// What a runtime that left `OnCollectionDeleted` unwired would leave behind: a
			// registration naming a collection that no longer answers for an owner.
			CollectionMinters::<Test>::insert(
				COLLECTION + 1,
				CollectionMinter { owner: COLLECTION_OWNER, selection: ItemSelection::Random },
			);
			assert!(NftClaims::do_try_state().is_err());
		});
	}

	#[test]
	fn try_state_holds_once_a_tree_is_removed_and_catches_stranded_leaves() {
		new_test_ext().execute_with(|| {
			// A one-leaf tree, so the first claim is its last and removes it.
			let awards = vec![(AccountOrPerson::Account(ALICE), [1u8; 32])];
			let tree = tree_of(BLOCK, &awards);
			store_tree(&awards);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			// The claimed leaves outlive the tree, and their expiry entry stays with them, which
			// `try_state` accepts.
			assert!(!CreditTrees::<Test>::contains_key(BLOCK));
			assert!(TreeExpiries::<Test>::contains_key(
				ExpiryTimestamp::from(tree.timestamp),
				BLOCK
			));
			assert_ok!(NftClaims::do_try_state());

			// Without that entry no sweep reaches the bitmap, so it would sit there for good.
			TreeExpiries::<Test>::remove(ExpiryTimestamp::from(tree.timestamp), BLOCK);
			assert!(NftClaims::do_try_state().is_err());
		});
	}

	#[test]
	fn try_state_catches_a_private_tree_index_that_disagrees_with_the_trees() {
		new_test_ext().execute_with(|| {
			use crate::PrivateGameTrees;

			let awards = awards();
			let mut private_tree = tree_of(BLOCK, &awards);
			private_tree.private_slots = 1;
			CreditTrees::<Test>::insert(BLOCK, private_tree);
			TreeExpiries::<Test>::insert(ExpiryTimestamp::from(private_tree.timestamp), BLOCK, ());
			PrivateGameTrees::<Test>::insert(private_tree.game_index, BLOCK, ());
			assert_ok!(NftClaims::do_try_state());

			// Without the entry the game's close never finds the tree, which then outlives the
			// game it belongs to.
			PrivateGameTrees::<Test>::remove(private_tree.game_index, BLOCK);
			assert!(NftClaims::do_try_state().is_err());
			PrivateGameTrees::<Test>::insert(private_tree.game_index, BLOCK, ());

			// An entry naming no tree is the other way round: nothing removes it.
			PrivateGameTrees::<Test>::insert(private_tree.game_index, BLOCK + 1, ());
			assert!(NftClaims::do_try_state().is_err());
		});
	}

	#[test]
	fn a_claim_refunds_the_selector_reservation_to_the_branch_taken() {
		use crate::weights::WeightInfo;
		use frame_support::dispatch::GetDispatchInfo;

		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			let proof = proof_of(&awards, 0);
			let charged = crate::Call::<Test>::claim {
				claimant: ClaimantKind::Account,
				block: BLOCK,
				credit: [1u8; 32],
				leaf_index: 0,
				proof: proof.clone(),
				collection: COLLECTION,
				mint_to: PURSE,
			}
			.get_dispatch_info()
			.call_weight;
			// A successful claim mints, so it also pays for the mint's runtime hooks. Only the
			// selector reservation is refundable.
			let account_minted = <Test as Config>::WeightInfo::claim_account(proof.len() as u32)
				.saturating_add(MINT_HOOK_WEIGHT);
			let person_minted = <Test as Config>::WeightInfo::claim_person(proof.len() as u32)
				.saturating_add(MINT_HOOK_WEIGHT);

			// The random branch consumes no selector weight, so the whole reservation comes
			// back.
			let post = NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof.clone(),
				COLLECTION,
				PURSE,
			)
			.expect("random-branch claim succeeds");
			assert_eq!(post.actual_weight, Some(account_minted));
			assert!(account_minted.all_lt(charged));

			// The contract branch pays what the selector really consumed, still below the
			// reservation.
			register_collection(ItemSelection::Contract(CONTRACT));
			let post = NftClaims::claim(
				RuntimeOrigin::signed(PERSON),
				ClaimantKind::Person,
				BLOCK,
				[2u8; 32],
				1,
				proof_of(&awards, 1),
				COLLECTION,
				PURSE + 1,
			)
			.expect("contract-branch claim succeeds");
			assert_eq!(
				post.actual_weight,
				Some(person_minted.saturating_add(SELECTOR_CONSUMED_WEIGHT))
			);
			assert!(post.actual_weight.expect("set above").all_lt(charged));
		});
	}

	#[test]
	fn a_failed_claim_refunds_the_selector_reservation_to_the_branch() {
		use crate::weights::WeightInfo;
		use frame_support::dispatch::GetDispatchInfo;

		new_test_ext().execute_with(|| {
			let awards = awards();
			store_tree(&awards);

			let proof = proof_of(&awards, 0);
			let charged = crate::Call::<Test>::claim {
				claimant: ClaimantKind::Account,
				block: BLOCK,
				credit: [1u8; 32],
				leaf_index: 0,
				proof: proof.clone(),
				collection: COLLECTION,
				mint_to: PURSE,
			}
			.get_dispatch_info()
			.call_weight;
			let account_base = <Test as Config>::WeightInfo::claim_account(proof.len() as u32);
			let person_base = <Test as Config>::WeightInfo::claim_person(proof.len() as u32);

			// A failure before any selection refunds the whole reservation: a bad proof never
			// reaches a minter, so a failing claim cannot occupy the selector's block space.
			let err = NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				2,
				proof.clone(),
				COLLECTION,
				PURSE,
			)
			.expect_err("the wrong leaf index fails the proof");
			assert_eq!(err.error, Error::<Test>::InvalidProof.into());
			assert_eq!(err.post_info.actual_weight, Some(account_base));
			assert!(account_base.all_lt(charged));

			// A failing contract selection pays what the contract really burned before failing,
			// above the pre-selection floor and still below the reservation.
			register_collection(ItemSelection::Contract(CONTRACT));
			SelectorFails::set(&true);
			let err = NftClaims::claim(
				RuntimeOrigin::signed(PERSON),
				ClaimantKind::Person,
				BLOCK,
				[2u8; 32],
				1,
				proof_of(&awards, 1),
				COLLECTION,
				PURSE + 1,
			)
			.expect_err("the contract selection fails");
			assert_eq!(err.error, DispatchError::Other("SelectorFailed"));
			let actual = err.post_info.actual_weight.expect("a failure refunds to a weight");
			assert_eq!(actual, person_base.saturating_add(SELECTOR_FAILED_WEIGHT));
			assert!(person_base.all_lt(actual));
			assert!(actual.all_lt(charged));
		});
	}
}

mod set_collection_minter {
	use super::*;
	use crate::{CollectionMinter, CollectionMinters, Error, Event, ItemSelection};
	use sp_core::H160;

	const OWNER: u64 = 50;
	const COLLECTION: u32 = 3;
	const CONTRACT: H160 = H160::repeat_byte(0xcd);

	#[test]
	fn the_owner_registers_and_withdraws_a_collection() {
		new_test_ext().execute_with(|| {
			add_collection(COLLECTION, OWNER, 2);

			let selection = ItemSelection::Contract(CONTRACT);
			assert_ok!(NftClaims::set_collection_minter(
				RuntimeOrigin::signed(OWNER),
				COLLECTION,
				Some(selection)
			));
			assert_eq!(
				CollectionMinters::<Test>::get(COLLECTION),
				Some(CollectionMinter { owner: OWNER, selection })
			);

			assert_ok!(NftClaims::set_collection_minter(
				RuntimeOrigin::signed(OWNER),
				COLLECTION,
				None
			));
			assert_eq!(CollectionMinters::<Test>::get(COLLECTION), None);

			assert_eq!(
				nft_claims_events(),
				vec![
					Event::CollectionMinterSet {
						collection: COLLECTION,
						selection: Some(selection)
					},
					Event::CollectionMinterSet { collection: COLLECTION, selection: None },
				]
			);
		});
	}

	#[test]
	fn only_the_owner_registers() {
		new_test_ext().execute_with(|| {
			add_collection(COLLECTION, OWNER, 2);

			assert_noop!(
				NftClaims::set_collection_minter(
					RuntimeOrigin::signed(OWNER + 1),
					COLLECTION,
					Some(ItemSelection::Random)
				),
				Error::<Test>::NotCollectionOwner
			);
			assert_eq!(CollectionMinters::<Test>::get(COLLECTION), None);
		});
	}

	#[test]
	fn an_unknown_collection_cannot_be_registered() {
		new_test_ext().execute_with(|| {
			assert_noop!(
				NftClaims::set_collection_minter(
					RuntimeOrigin::signed(OWNER),
					COLLECTION,
					Some(ItemSelection::Random)
				),
				Error::<Test>::UnknownCollection
			);
			assert_noop!(
				NftClaims::set_collection_minter(RuntimeOrigin::signed(OWNER), COLLECTION, None),
				Error::<Test>::UnknownCollection
			);
		});
	}

	#[test]
	fn deleting_a_collection_clears_its_registration() {
		new_test_ext().execute_with(|| {
			add_collection(COLLECTION, OWNER, 2);
			assert_ok!(NftClaims::set_collection_minter(
				RuntimeOrigin::signed(OWNER),
				COLLECTION,
				Some(ItemSelection::Random)
			));

			// Scarcity calls the deletion hook when the collection is deleted, so no
			// registration outlives the collection it names and none can be stranded.
			<crate::ClearCollectionMinter<Test> as indiv_pallet_scarcity::OnCollectionDeleted>::on_collection_deleted(
				COLLECTION,
			);
			assert_eq!(CollectionMinters::<Test>::get(COLLECTION), None);
		});
	}
	#[test]
	fn an_address_without_code_cannot_be_registered_as_a_minter() {
		new_test_ext().execute_with(|| {
			add_collection(COLLECTION, OWNER, 2);
			ContractValid::set(&false);

			assert_noop!(
				NftClaims::set_collection_minter(
					RuntimeOrigin::signed(OWNER),
					COLLECTION,
					Some(ItemSelection::Contract(CONTRACT))
				),
				sp_runtime::DispatchError::Other("NotAContract")
			);
			assert_eq!(CollectionMinters::<Test>::get(COLLECTION), None);

			// Only a contract selection is validated: the random one names no contract.
			assert_ok!(NftClaims::set_collection_minter(
				RuntimeOrigin::signed(OWNER),
				COLLECTION,
				Some(ItemSelection::Random)
			));
		});
	}
}

mod expiry {
	use super::*;
	use crate::{AuthorizeInvalidity, PendingTreeDeletions, TreeExpiries};
	use frame_support::pallet_prelude::{
		InvalidTransaction, TransactionSource, TransactionValidityError,
	};
	use indiv_support::credit_trees::oldest_expiry;

	/// The timestamp the mock's `tree(10)` commits to, which is the oldest any test here files.
	const TIMESTAMP: u32 = 1_010;

	fn sweep(oldest: u32) -> frame_support::dispatch::DispatchResultWithPostInfo {
		NftClaims::sweep_expired_trees(
			RuntimeOrigin::from(frame_system::RawOrigin::Authorized),
			oldest,
			1,
		)
	}

	/// The timestamp the next sweep starts at.
	fn oldest_filed() -> Option<u32> {
		oldest_expiry::<TreeExpiries<Test>, AwardBlock>()
	}

	#[test]
	fn a_received_tree_is_filed_under_its_timestamp() {
		new_test_ext().execute_with(|| {
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10)])
			));

			assert!(TreeExpiries::<Test>::contains_key(ExpiryTimestamp::from(TIMESTAMP), 10));
			assert_eq!(oldest_filed(), Some(TIMESTAMP));
		});
	}

	#[test]
	fn a_tree_that_arrives_past_its_deadline_is_not_stored() {
		new_test_ext().execute_with(|| {
			// The tree of block 10 is timestamped 1010, so its deadline is one TTL later.
			set_now(due_at(TIMESTAMP));

			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![replay(10)])
			));

			assert!(!CreditTrees::<Test>::contains_key(10));
			assert_eq!(oldest_filed(), None);
			assert_eq!(
				nft_claims_events(),
				vec![
					Event::CreditTreeStale { block: 10 },
					Event::CreditTreesReceived { count: 1, stored: 0 },
				]
			);
		});
	}

	#[test]
	fn a_tree_of_more_leaves_than_the_bitmap_holds_is_not_stored() {
		new_test_ext().execute_with(|| {
			// The mock's trees commit to three leaves.
			MaxCreditsPerAwardBlock::set(&2);

			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10)])
			));

			assert!(!CreditTrees::<Test>::contains_key(10));
			assert_eq!(
				nft_claims_events(),
				vec![
					Event::CreditTreeOversized { block: 10 },
					Event::CreditTreesReceived { count: 1, stored: 0 },
				]
			);
		});
	}

	#[test]
	fn a_tree_one_second_short_of_its_deadline_is_still_stored() {
		new_test_ext().execute_with(|| {
			set_now(due_at(TIMESTAMP) - 1);

			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![replay(10)])
			));

			assert_eq!(CreditTrees::<Test>::get(10), Some(tree(10)));
		});
	}

	#[test]
	fn a_sweep_removes_the_due_trees_and_queues_their_deletion() {
		new_test_ext().execute_with(|| {
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10)])
			));
			set_now(due_at(TIMESTAMP));
			System::reset_events();

			assert_ok!(sweep(TIMESTAMP));

			assert!(!CreditTrees::<Test>::contains_key(10));
			assert_eq!(oldest_filed(), None);
			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![10]);
			assert!(nft_claims_events().contains(&Event::CreditTreesExpired { count: 1 }));
		});
	}

	#[test]
	fn a_sweep_stops_at_the_first_tree_that_is_not_due() {
		new_test_ext().execute_with(|| {
			// Block 11's tree is timestamped one second after block 10's, so its deadline is one
			// second later as well.
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10), update(1, 11)])
			));
			set_now(due_at(TIMESTAMP));

			let post = sweep(TIMESTAMP).expect("the sweep goes through");

			assert_eq!(
				post.actual_weight,
				Some(<MockWeightInfo as crate::WeightInfo>::sweep_expired_trees(1)),
			);
			assert!(!CreditTrees::<Test>::contains_key(10));
			assert!(CreditTrees::<Test>::contains_key(11), "the tree that is not due stays");
			assert_eq!(oldest_filed(), Some(TIMESTAMP + 1));
			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![10]);
		});
	}

	#[test]
	fn more_due_trees_than_one_sweep_removes_take_several() {
		new_test_ext().execute_with(|| {
			// Three trees against a `MaxTreeDeletionsPerMessage` of two.
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10), update(1, 11), update(2, 12)])
			));
			set_now(due_at(TIMESTAMP + 2));

			let post = sweep(TIMESTAMP).expect("the first sweep goes through");
			assert_eq!(
				post.actual_weight,
				Some(<MockWeightInfo as crate::WeightInfo>::sweep_expired_trees(2)),
			);
			assert_eq!(oldest_filed(), Some(TIMESTAMP + 2), "the sweep is up to the third tree");

			let post = sweep(TIMESTAMP + 2).expect("the second sweep goes through");
			assert_eq!(
				post.actual_weight,
				Some(<MockWeightInfo as crate::WeightInfo>::sweep_expired_trees(1)),
			);
			assert_eq!(CreditTrees::<Test>::iter().count(), 0);
			assert_eq!(oldest_filed(), None);
		});
	}

	#[test]
	fn a_tree_that_arrives_out_of_order_is_swept_first() {
		new_test_ext().execute_with(|| {
			// A later block's tree first, then an earlier one, as an out-of-order delivery gives.
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 11)])
			));
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![replay(10)])
			));

			assert_eq!(oldest_filed(), Some(TIMESTAMP), "the sweep starts at the earlier tree");

			set_now(due_at(TIMESTAMP));
			assert_ok!(sweep(TIMESTAMP));

			assert!(!CreditTrees::<Test>::contains_key(10));
			assert!(CreditTrees::<Test>::contains_key(11), "the tree that is not due stays");
		});
	}

	#[test]
	fn the_sweep_clears_the_private_tree_index() {
		new_test_ext().execute_with(|| {
			// A private game whose ring never arrived. No close removes its trees, so the sweep
			// is what removes them, and their credits were mintable on neither path.
			let mut private_tree = tree(10);
			private_tree.private_slots = 1;
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![CreditTreeDelivery {
					sequence: Some(0),
					block: 10,
					tree: private_tree
				}])
			));
			assert!(crate::PrivateGameTrees::<Test>::contains_key(private_tree.game_index, 10));

			set_now(due_at(TIMESTAMP));
			assert_ok!(sweep(TIMESTAMP));

			assert!(!CreditTrees::<Test>::contains_key(10));
			assert!(!crate::PrivateGameTrees::<Test>::contains_key(private_tree.game_index, 10));
			System::assert_has_event(Event::CreditTreesExpired { count: 1 }.into());
		});
	}

	#[test]
	fn the_offchain_worker_submits_one_sweep_per_block() {
		new_test_ext().execute_with(|| {
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10)])
			));
			set_now(due_at(TIMESTAMP));

			run_offchain_worker(5);
			run_offchain_worker(6);

			// The discriminator is the submitting block, so the sweeps of two blocks differ while
			// no sweep has been included. `oldest` alone cannot tell them apart, and a repeated
			// encoding gives a hash the pool has banned.
			assert_eq!(
				submitted_calls(),
				vec![
					RuntimeCall::NftClaims(crate::Call::sweep_expired_trees {
						oldest: TIMESTAMP,
						discriminator: 5
					}),
					RuntimeCall::NftClaims(crate::Call::sweep_expired_trees {
						oldest: TIMESTAMP,
						discriminator: 6
					}),
				]
			);
		});
	}

	#[test]
	fn a_sweep_is_authorized_only_for_the_oldest_filed_timestamp() {
		new_test_ext().execute_with(|| {
			assert_eq!(
				crate::Pallet::<Test>::authorize_sweep_expired_trees(
					TransactionSource::Local,
					&TIMESTAMP
				),
				Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
					AuthorizeInvalidity::NothingToSweep as u8
				))),
				"nothing is filed",
			);

			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10), update(1, 11)])
			));

			assert_eq!(
				crate::Pallet::<Test>::authorize_sweep_expired_trees(
					TransactionSource::Local,
					&TIMESTAMP
				),
				Err(InvalidTransaction::Future.into()),
				"the oldest tree's deadline has not passed",
			);

			set_now(due_at(TIMESTAMP));
			assert!(crate::Pallet::<Test>::authorize_sweep_expired_trees(
				TransactionSource::Local,
				&TIMESTAMP
			)
			.is_ok());

			assert_eq!(
				crate::Pallet::<Test>::authorize_sweep_expired_trees(
					TransactionSource::Local,
					&(TIMESTAMP + 1)
				),
				Err(InvalidTransaction::Future.into()),
				"a timestamp the sweep has not reached",
			);

			assert_eq!(
				crate::Pallet::<Test>::authorize_sweep_expired_trees(
					TransactionSource::Local,
					&(TIMESTAMP - 1)
				),
				Err(InvalidTransaction::Stale.into()),
				"a timestamp the sweep is past",
			);
		});
	}

	#[test]
	fn a_sweep_from_an_external_source_is_rejected() {
		new_test_ext().execute_with(|| {
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 10)])
			));
			set_now(due_at(TIMESTAMP));

			assert_eq!(
				crate::Pallet::<Test>::authorize_sweep_expired_trees(
					TransactionSource::External,
					&TIMESTAMP
				),
				Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
					AuthorizeInvalidity::TransactionNotLocal as u8
				))),
			);
		});
	}
}

mod deletions {
	use super::*;
	use crate::{AuthorizeInvalidity, PendingTreeDeletions};
	use codec::Encode;
	use frame_support::pallet_prelude::{
		InvalidTransaction, TransactionSource, TransactionValidityError,
	};

	fn send(front: AwardBlock) -> frame_support::dispatch::DispatchResultWithPostInfo {
		NftClaims::send_tree_deletions(
			RuntimeOrigin::from(frame_system::RawOrigin::Authorized),
			front,
			1,
		)
	}

	fn queue(blocks: Vec<AwardBlock>) {
		PendingTreeDeletions::<Test>::put(
			BoundedVec::try_from(blocks).expect("fits MaxQueuedTreeDeletions"),
		);
	}

	#[test]
	fn a_send_carries_a_messages_worth_and_leaves_the_rest_queued() {
		new_test_ext().execute_with(|| {
			// Three blocks against a `MaxTreeDeletionsPerMessage` of two.
			queue(vec![10, 11, 12]);

			assert_ok!(send(10));

			assert_eq!(last_sent_deletions(), vec![10, 11]);
			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![12]);
			assert!(nft_claims_events().contains(&Event::TreeDeletionsSent {
				blocks: BoundedVec::truncate_from(vec![10, 11])
			}));
		});
	}

	#[test]
	fn a_failed_send_keeps_the_queue_for_the_next_cycle() {
		new_test_ext().execute_with(|| {
			queue(vec![10]);
			fail_deletion_xcms(true);

			assert_ok!(send(10));

			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![10]);
			assert!(nft_claims_events().contains(&Event::TreeDeletionSendFailed));
			assert!(sent_deletion_xcms().is_empty());
		});
	}

	#[test]
	fn a_full_queue_drops_the_deletions_it_has_no_room_for_and_reports_them_together() {
		new_test_ext().execute_with(|| {
			// One slot short of `MaxQueuedTreeDeletions`. One sweep cannot fill the queue that far,
			// but a stalled delivery leaves it in this state.
			queue(vec![1, 2, 3]);

			crate::Pallet::<Test>::queue_tree_deletions(&[5, 6]);

			// The last slot took the first block, so only the second one is dropped.
			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![1, 2, 3, 5]);
			assert!(nft_claims_events().contains(&Event::TreeDeletionsDropped {
				blocks: BoundedVec::truncate_from(vec![6])
			}));

			// A queue with no room left drops every block of the call.
			crate::Pallet::<Test>::queue_tree_deletions(&[7, 8]);

			assert_eq!(PendingTreeDeletions::<Test>::get().to_vec(), vec![1, 2, 3, 5]);
			assert!(nft_claims_events().contains(&Event::TreeDeletionsDropped {
				blocks: BoundedVec::truncate_from(vec![7, 8])
			}));

			// One event per call, whatever a call drops.
			assert_eq!(
				nft_claims_events()
					.iter()
					.filter(|event| matches!(event, Event::TreeDeletionsDropped { .. }))
					.count(),
				2
			);
		});
	}

	#[test]
	fn the_offchain_worker_names_the_front_it_sends() {
		new_test_ext().execute_with(|| {
			// Three blocks against a `MaxTreeDeletionsPerMessage` of two, so one send leaves a
			// second front to send.
			queue(vec![10, 11, 12]);

			run_offchain_worker(5);
			// The send of the first front goes through, which leaves block 12 at the front.
			assert_ok!(send(10));
			run_offchain_worker(6);

			// Both submissions sit in one retry window and differ in `front` alone. That gives the
			// second a hash the pool has not banned.
			assert_eq!(
				submitted_calls(),
				vec![
					RuntimeCall::NftClaims(crate::Call::send_tree_deletions {
						front: 10,
						discriminator: 0
					}),
					RuntimeCall::NftClaims(crate::Call::send_tree_deletions {
						front: 12,
						discriminator: 0
					}),
				]
			);
		});
	}

	#[test]
	fn a_send_is_authorized_only_while_something_is_queued() {
		new_test_ext().execute_with(|| {
			assert_eq!(
				crate::Pallet::<Test>::authorize_send_tree_deletions(TransactionSource::Local, &10),
				Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
					AuthorizeInvalidity::NoQueuedTreeDeletions as u8
				))),
				"nothing is queued",
			);

			queue(vec![10, 11]);
			let validity =
				crate::Pallet::<Test>::authorize_send_tree_deletions(TransactionSource::Local, &10)
					.expect("the queue has a front to send")
					.0;
			assert_eq!(
				validity.provides,
				vec![("nft-claims:send-tree-deletions", 10u32).encode()],
				"the tag is the front of the queue, so a send that goes through frees the next one",
			);

			assert_eq!(
				crate::Pallet::<Test>::authorize_send_tree_deletions(TransactionSource::Local, &11),
				Err(TransactionValidityError::Invalid(InvalidTransaction::Stale)),
				"block 11 is queued but is not the front, so only the send naming 10 authorizes",
			);

			assert_eq!(
				crate::Pallet::<Test>::authorize_send_tree_deletions(
					TransactionSource::External,
					&10
				),
				Err(TransactionValidityError::Invalid(InvalidTransaction::Custom(
					AuthorizeInvalidity::TransactionNotLocal as u8
				))),
			);
		});
	}
}

mod migration {
	use super::*;
	use crate::{
		migration::{
			v1::{ClaimedCounts, ClaimedCredits},
			MigrateV0ToV1,
		},
		ClaimedLeaves,
	};
	use frame_support::traits::{GetStorageVersion, OnRuntimeUpgrade, StorageVersion};
	use indiv_support::credit_trees::NftClaimCreditLeaf;

	/// Whether the tree of `block` is filed for expiry under the timestamp it commits to.
	fn filed(block: AwardBlock) -> bool {
		TreeExpiries::<Test>::contains_key(ExpiryTimestamp::from(tree(block).timestamp), block)
	}

	/// Stores the tree of `block` the way a chain running the old code left it: the tree alone,
	/// with no expiry entry and with `claimed` of its leaves recorded by hash.
	fn store_old_tree(block: AwardBlock, claimed: u32) {
		CreditTrees::<Test>::insert(block, tree(block));
		for index in 0..claimed {
			ClaimedCredits::<Test>::insert(block, NftClaimCreditLeaf([index as u8; 32]), ());
		}
		ClaimedCounts::<Test>::insert(block, claimed);
	}

	fn migrate() {
		StorageVersion::new(0).put::<NftClaims>();
		<MigrateV0ToV1<Test> as OnRuntimeUpgrade>::on_runtime_upgrade();
	}

	#[test]
	fn the_migration_files_an_unclaimed_tree_under_its_timestamp() {
		new_test_ext().execute_with(|| {
			store_old_tree(10, 0);

			migrate();

			assert_eq!(CreditTrees::<Test>::get(10), Some(tree(10)));
			assert!(filed(10));
			assert!(!ClaimedLeaves::<Test>::contains_key(10), "no leaf of it was claimed");
			assert_eq!(NftClaims::on_chain_storage_version(), 1);
		});
	}

	#[test]
	fn the_migration_settles_a_partly_claimed_tree() {
		new_test_ext().execute_with(|| {
			// One of the three leaves was claimed, under a hash that names no leaf index.
			store_old_tree(10, 1);

			migrate();

			assert_eq!(CreditTrees::<Test>::get(10), None, "the tree is removed as claimed");
			assert_eq!(
				ClaimedLeaves::<Test>::get(10).into_inner(),
				vec![0b111u8],
				"every leaf of it counts as spent"
			);
			assert!(filed(10), "the sweep drops the bitmap");
			assert_eq!(PendingTreeDeletions::<Test>::get().into_inner(), vec![10]);
		});
	}

	#[test]
	fn a_settled_tree_cannot_be_claimed_after_a_replay() {
		new_test_ext().execute_with(|| {
			store_old_tree(10, 1);
			migrate();

			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![replay(10)])
			));

			assert_eq!(CreditTrees::<Test>::get(10), Some(tree(10)));
			for leaf_index in 0..tree(10).leaf_count {
				assert!(
					NftClaims::leaf_is_claimed(&ClaimedLeaves::<Test>::get(10), leaf_index),
					"a replayed tree must mint nothing"
				);
			}
		});
	}

	#[test]
	fn the_migration_removes_a_tree_that_outgrew_the_credit_bound() {
		new_test_ext().execute_with(|| {
			let mut oversized = tree(10);
			oversized.leaf_count = MaxCreditsPerAwardBlock::get() + 1;
			CreditTrees::<Test>::insert(10, oversized);

			migrate();

			assert_eq!(CreditTrees::<Test>::get(10), None);
			assert!(!filed(10), "nothing is left to sweep");
			assert!(!ClaimedLeaves::<Test>::contains_key(10), "no bitmap covers its leaves");
			assert_eq!(PendingTreeDeletions::<Test>::get().into_inner(), vec![10]);
		});
	}

	#[test]
	fn the_migration_clears_the_records_the_bitmap_replaces() {
		new_test_ext().execute_with(|| {
			store_old_tree(10, 2);
			store_old_tree(11, 0);

			migrate();

			assert_eq!(ClaimedCredits::<Test>::iter().count(), 0);
			assert_eq!(ClaimedCounts::<Test>::iter().count(), 0);
		});
	}

	#[test]
	fn the_version_gate_keeps_the_migration_from_running_twice() {
		new_test_ext().execute_with(|| {
			store_old_tree(10, 0);
			migrate();

			// A tree of the shape the migration files, stored after it ran. Only a second run
			// files this one, and the version gate is what stops that.
			store_old_tree(11, 0);
			<MigrateV0ToV1<Test> as OnRuntimeUpgrade>::on_runtime_upgrade();

			assert!(!filed(11), "the version gate must keep the migration from running twice");
		});
	}
}

/// The private claim path: the ring the game chain delivers and the claims proven against it.
mod private_claims {
	use super::*;
	use crate::{
		AbandonedPrivateGames, AuthorizeInvalidity, ClosedPrivateGames, CollectionMinter,
		CollectionMinters, Error, ItemSelection, PrivateClaimsThisBlock, PrivateGameTrees,
		PrivateRingCloses, PrivateRings, SpentPrivateClaims, PRIVATE_CLOSE_ITEMS,
	};
	use frame_support::traits::OnInitialize;
	use indiv_pallet_scarcity::CollectionId;
	use indiv_support::{
		credit_trees::{PrivateGameOutcome, PrivateRingDelivery},
		identity::AccountOrPerson,
		traits::Alias,
		utils::BigEndianU64,
	};
	use sp_runtime::transaction_validity::{
		InvalidTransaction, TransactionSource, TransactionValidityError,
	};
	use verifiable::{mock::Mock, GenerateVerifiable};

	/// A claim's error carries the weight it consumed, so the tests compare the error alone and
	/// check that the state is untouched, as the public claim tests do.
	macro_rules! assert_claim_noop {
		($call:expr, $err:expr $(,)?) => {{
			let root = sp_io::storage::root(sp_runtime::StateVersion::V1);
			assert_eq!($call.map(|_| ()).map_err(|e| e.error), Err($err.into()));
			assert_eq!(
				root,
				sp_io::storage::root(sp_runtime::StateVersion::V1),
				"storage has been mutated"
			);
		}};
	}

	const GAME: crate::GameIdx = 7;
	const COLLECTION: CollectionId = 1;
	/// The account a public claim would be made by, for the test that closes that path.
	const ALICE: u64 = 1;
	/// The award block the public-path test stores its tree under.
	const BLOCK: AwardBlock = 10;
	const COLLECTION_OWNER: u64 = 50;
	/// The purse key the NFT is minted to.
	const PURSE: u64 = 77;

	/// A claimant's one-time ring key and the secret behind it.
	fn member(
		seed: u8,
	) -> (<Mock as GenerateVerifiable>::Secret, <Mock as GenerateVerifiable>::Member) {
		let secret = Mock::new_secret([seed; 32]);
		let key = Mock::member_from_secret(&secret);
		(secret, key)
	}

	/// Deliver the game's ring over `keys`, granting `slots` slots to each, and register the
	/// collection claims mint into.
	fn store_ring(slots: u8, keys: &[<Mock as GenerateVerifiable>::Member]) {
		assert_ok!(NftClaims::receive_private_rings(
			game_chain_origin(),
			private_ring_batch(vec![private_ring_delivery(GAME, slots, keys)])
		));
		add_collection(COLLECTION, COLLECTION_OWNER, 2);
		CollectionMinters::<Test>::insert(
			COLLECTION,
			CollectionMinter { owner: COLLECTION_OWNER, selection: ItemSelection::Random },
		);
		open_claim_window();
	}

	/// Move to the block the stored ring's claims open in, which is the earliest one a claim is
	/// taken in.
	fn open_claim_window() {
		let ring = PrivateRings::<Test>::get(GAME).expect("a ring is stored");
		System::set_block_number(ring.opens_at);
	}

	/// One close step of [`GAME`], as the offchain worker submits it.
	fn close() -> frame_support::dispatch::DispatchResultWithPostInfo {
		NftClaims::close_private_ring(frame_system::RawOrigin::Authorized.into(), GAME, 1)
	}

	/// The validity of a close step of [`GAME`], as the pool checks it.
	fn authorize_close() -> Result<(), TransactionValidityError> {
		NftClaims::authorize_close_private_ring(TransactionSource::Local, &GAME).map(|_| ())
	}

	/// One award block of [`GAME`], as the game chain delivers the tree of a private game.
	fn private_update(block: AwardBlock) -> CreditTreeDelivery {
		let mut tree = tree(block);
		tree.private_slots = 1;
		CreditTreeDelivery { sequence: None, block, tree }
	}

	/// Store the tree [`GAME`] awarded in `block`.
	fn store_private_tree(block: AwardBlock) {
		assert_ok!(NftClaims::receive_credit_trees(
			game_chain_origin(),
			batch(vec![private_update(block)])
		));
	}

	/// The abandonment of `game_index`, as the game chain delivers it for a ring it did not
	/// build over the `key_count` keys that had registered.
	fn abandoned_delivery(
		game_index: crate::GameIdx,
		slots: u8,
		key_count: u32,
	) -> PrivateRingDelivery<<Mock as GenerateVerifiable>::Members> {
		PrivateRingDelivery {
			game_index,
			slots,
			outcome: PrivateGameOutcome::Abandoned { key_count },
		}
	}

	/// The proof `secret` makes for `slot` of [`GAME`], minting `COLLECTION` to `mint_to`, with
	/// the alias it yields.
	fn proof(
		secret: &<Mock as GenerateVerifiable>::Secret,
		keys: &[<Mock as GenerateVerifiable>::Member],
		slot: u8,
		mint_to: u64,
	) -> (crate::RingProofOf<Test>, Alias) {
		proof_for(secret, keys, slot, COLLECTION, mint_to)
	}

	/// The proof `secret` makes for `slot` of [`GAME`], minting `collection` to `mint_to`, with
	/// the alias it yields.
	fn proof_for(
		secret: &<Mock as GenerateVerifiable>::Secret,
		keys: &[<Mock as GenerateVerifiable>::Member],
		slot: u8,
		collection: CollectionId,
		mint_to: u64,
	) -> (crate::RingProofOf<Test>, Alias) {
		let commitment =
			Mock::open(Default::default(), &Mock::member_from_secret(secret), keys.iter().cloned())
				.expect("the member is in the ring");
		Mock::create(
			commitment,
			secret,
			&NftClaims::private_claim_context(GAME, slot),
			&NftClaims::private_claim_message(collection, &mint_to),
		)
		.expect("the mock creates a proof")
	}

	/// What `authorize` makes of the claim. The pool and the block both run it first.
	fn authorize(
		slot: u8,
		alias: Alias,
		proof: crate::RingProofOf<Test>,
		collection: CollectionId,
		mint_to: u64,
	) -> Result<(), TransactionValidityError> {
		NftClaims::authorize_claim_private(
			TransactionSource::External,
			&GAME,
			&slot,
			&alias,
			&proof,
			&collection,
			&mint_to,
		)
		.map(|_| ())
	}

	/// The dispatch alone, as it runs once `authorize` has passed.
	fn dispatch(
		slot: u8,
		alias: Alias,
		proof: crate::RingProofOf<Test>,
		collection: CollectionId,
		mint_to: u64,
	) -> frame_support::dispatch::DispatchResultWithPostInfo {
		NftClaims::claim_private(
			frame_system::RawOrigin::Authorized.into(),
			GAME,
			slot,
			alias,
			proof,
			collection,
			mint_to,
		)
	}

	/// A whole claim, the way a block runs one: `authorize`, then the dispatch it authorized.
	fn claim(
		slot: u8,
		alias: Alias,
		proof: crate::RingProofOf<Test>,
		collection: CollectionId,
		mint_to: u64,
	) -> frame_support::dispatch::DispatchResultWithPostInfo {
		authorize(slot, alias, proof.clone(), collection, mint_to)
			.expect("the claim is authorized");
		dispatch(slot, alias, proof, collection, mint_to)
	}

	#[test]
	fn a_delivered_ring_is_stored_once() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(2, &keys);

			let ring = PrivateRings::<Test>::get(GAME).unwrap();
			assert_eq!(ring.key_count, 2);
			assert_eq!(ring.slots, 2);

			// A second, different ring for the same game is refused. The stored ring stays,
			// because proofs are already built against it.
			let other = [member(3).1];
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![private_ring_delivery(GAME, 2, &other)])
			));
			assert_eq!(PrivateRings::<Test>::get(GAME).unwrap().key_count, 2);
			System::assert_has_event(Event::PrivateOutcomeConflict { game_index: GAME }.into());
		});
	}

	#[test]
	fn an_abandoned_game_puts_its_credits_back_on_the_public_path() {
		new_test_ext().execute_with(|| {
			let awards = vec![(AccountOrPerson::Account(ALICE), [1u8; 32])];
			let mut private_tree = tree_of(BLOCK, &awards);
			private_tree.private_slots = 2;
			CreditTrees::<Test>::insert(BLOCK, private_tree);
			add_collection(COLLECTION, COLLECTION_OWNER, 2);
			CollectionMinters::<Test>::insert(
				COLLECTION,
				CollectionMinter { owner: COLLECTION_OWNER, selection: ItemSelection::Random },
			);

			// The game chain gave up on the ring, so nobody proved anything against it and the
			// credits are minted publicly instead.
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![abandoned_delivery(GAME, 2, 3)])
			));
			System::assert_has_event(
				Event::PrivateGameAbandoned { game_index: GAME, key_count: 3 }.into(),
			);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));
			assert_eq!(MintedInstances::get().len(), 1);

			// The game has no ring either way, so no private claim of it exists.
			assert!(PrivateRings::<Test>::get(GAME).is_none());
		});
	}

	#[test]
	fn an_abandonment_does_not_undo_a_ring_the_game_already_delivered() {
		new_test_ext().execute_with(|| {
			store_ring(2, &[member(1).1, member(2).1]);

			// Claims may already rest on the ring, so reopening the public path would mint a
			// second NFT for every credit they spent.
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![abandoned_delivery(GAME, 2, 2)])
			));

			assert!(AbandonedPrivateGames::<Test>::get(GAME).is_none());
			assert!(PrivateRings::<Test>::get(GAME).is_some());
			System::assert_has_event(Event::PrivateOutcomeConflict { game_index: GAME }.into());
		});
	}

	#[test]
	fn a_ring_after_an_abandonment_is_refused() {
		new_test_ext().execute_with(|| {
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![abandoned_delivery(GAME, 2, 1)])
			));

			// The public path is open by now, so a claim may already have minted a credit the
			// ring would let its owner mint again.
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![private_ring_delivery(GAME, 2, &[member(1).1])])
			));

			assert!(PrivateRings::<Test>::get(GAME).is_none());
			assert!(AbandonedPrivateGames::<Test>::get(GAME).is_some());
			System::assert_has_event(Event::PrivateOutcomeConflict { game_index: GAME }.into());
		});
	}

	#[test]
	fn a_ring_granting_no_slot_is_refused() {
		new_test_ext().execute_with(|| {
			// A game that grants no slot is a public game, so a ring for one cannot be genuine.
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![private_ring_delivery(GAME, 0, &[member(1).1])])
			));

			assert!(PrivateRings::<Test>::get(GAME).is_none());
		});
	}

	#[test]
	fn a_ring_member_mints_without_naming_themselves() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			store_ring(2, &keys);
			let (proof, alias) = proof(&secret, &keys, 0, PURSE);

			assert_ok!(claim(0, alias, proof, COLLECTION, PURSE));

			assert_eq!(MintedInstances::get().len(), 1);
			assert_eq!(MintedInstances::get()[0].2, PURSE);
			assert_eq!(SpentPrivateClaims::<Test>::iter_prefix(GAME).count(), 1);
		});
	}

	#[test]
	fn a_claim_carries_no_signer() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			store_ring(1, &keys);
			let (proof, alias) = proof(&secret, &keys, 0, PURSE);

			// The proof is the whole authorisation, so the call takes no signed origin. An
			// account on the claim would tie its maker's mints to each other.
			assert_claim_noop!(
				NftClaims::claim_private(
					RuntimeOrigin::signed(ALICE),
					GAME,
					0,
					alias,
					proof,
					COLLECTION,
					PURSE
				),
				DispatchError::BadOrigin
			);
		});
	}

	#[test]
	fn a_slot_mints_once() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			store_ring(2, &keys);

			let (first, alias) = proof(&secret, &keys, 0, PURSE);
			assert_ok!(claim(0, alias, first.clone(), COLLECTION, PURSE));

			// The alias is the nullifier, so the pool turns the replay away before a block spends
			// anything on it.
			assert_eq!(
				authorize(0, alias, first, COLLECTION, PURSE),
				Err(AuthorizeInvalidity::SlotAlreadyClaimed.into())
			);

			// The next slot is a different context, so the same member mints again. A purse key
			// holds one NFT, so the second claim names a fresh one, as a claimant does to keep
			// their mints apart.
			let (second, second_alias) = proof(&secret, &keys, 1, PURSE + 1);
			assert_ok!(claim(1, second_alias, second, COLLECTION, PURSE + 1));
			assert_ne!(alias, second_alias);
			assert_eq!(SpentPrivateClaims::<Test>::iter_prefix(GAME).count(), 2);
		});
	}

	#[test]
	fn every_member_holds_the_same_slots() {
		new_test_ext().execute_with(|| {
			// One ring, one entitlement. Both members claim both slots against the same set, so
			// no claim narrows its maker down.
			let (first_secret, first_key) = member(1);
			let (second_secret, second_key) = member(2);
			let keys = [first_key, second_key];
			store_ring(2, &keys);

			for (index, secret) in [&first_secret, &second_secret].iter().enumerate() {
				for slot in 0..2u8 {
					let purse = PURSE + (index as u64) * 2 + slot as u64;
					let (proof, alias) = proof(secret, &keys, slot, purse);
					assert_ok!(claim(slot, alias, proof, COLLECTION, purse));
					NftClaims::on_initialize(System::block_number() + 1);
				}
			}

			assert_eq!(SpentPrivateClaims::<Test>::iter_prefix(GAME).count(), 4);
		});
	}

	#[test]
	fn a_slot_the_game_does_not_grant_is_refused() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			store_ring(1, &keys);

			// The game granted one slot, so no member may claim under its second context,
			// whatever proof they make.
			let (proof, alias) = proof(&secret, &keys, 1, PURSE);
			assert_eq!(
				authorize(1, alias, proof, COLLECTION, PURSE),
				Err(AuthorizeInvalidity::SlotOutOfRange.into())
			);
		});
	}

	#[test]
	fn a_proof_for_another_collection_does_not_mint() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			store_ring(2, &keys);
			// A second registered collection. Without the message binding, an observer of a
			// pending claim could spend its alias on this one.
			let other: CollectionId = COLLECTION + 1;
			add_collection(other, COLLECTION_OWNER, 2);
			CollectionMinters::<Test>::insert(
				other,
				CollectionMinter { owner: COLLECTION_OWNER, selection: ItemSelection::Random },
			);

			let (proof, alias) = proof_for(&secret, &keys, 0, COLLECTION, PURSE);
			assert_eq!(
				authorize(0, alias, proof, other, PURSE),
				Err(AuthorizeInvalidity::InvalidRingProof.into())
			);
		});
	}

	#[test]
	fn a_proof_for_another_purse_does_not_mint() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			store_ring(2, &keys);

			// The message binds the purse key, so an observed proof cannot be redirected.
			let (proof, alias) = proof(&secret, &keys, 0, PURSE);
			assert_eq!(
				authorize(0, alias, proof, COLLECTION, PURSE + 1),
				Err(AuthorizeInvalidity::InvalidRingProof.into())
			);
		});
	}

	#[test]
	fn a_proof_that_yields_another_alias_does_not_mint() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			store_ring(2, &keys);

			// The dispatch spends the alias the call names, so `authorize` ties it to the proof.
			// A claim naming an alias of its own mints nothing.
			let (proof, alias) = proof(&secret, &keys, 0, PURSE);
			let other = [9u8; 32];
			assert_ne!(alias, other);
			assert_eq!(
				authorize(0, other, proof, COLLECTION, PURSE),
				Err(AuthorizeInvalidity::InvalidRingProof.into())
			);
		});
	}

	#[test]
	fn a_non_member_does_not_mint() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(2, &keys);

			// The outsider proves against a ring they are in, but not the one that was delivered.
			let (outsider_secret, outsider_key) = member(9);
			let outsider_ring = [outsider_key];
			let (proof, alias) = proof(&outsider_secret, &outsider_ring, 0, PURSE);
			assert_eq!(
				authorize(0, alias, proof, COLLECTION, PURSE),
				Err(AuthorizeInvalidity::InvalidRingProof.into())
			);
		});
	}

	#[test]
	fn a_game_without_a_ring_takes_no_claim() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key];

			let (proof, alias) = proof(&secret, &keys, 0, PURSE);
			assert_eq!(
				authorize(0, alias, proof, COLLECTION, PURSE),
				Err(AuthorizeInvalidity::UnknownPrivateRing.into())
			);
		});
	}

	#[test]
	fn the_block_cap_bounds_the_verifications() {
		new_test_ext().execute_with(|| {
			let members = [member(1), member(2), member(3)];
			let keys = members.iter().map(|(_, key)| *key).collect::<Vec<_>>();
			store_ring(1, &keys);

			// Two per block in the mock, so the third is held back rather than verified.
			for (index, (secret, _)) in members.iter().take(2).enumerate() {
				let purse = PURSE + index as u64;
				let (proof, alias) = proof(secret, &keys, 0, purse);
				assert_ok!(claim(0, alias, proof, COLLECTION, purse));
			}

			// `Future` and not a custom invalidity. The claim is valid and only waits for a block
			// with room, so the pool keeps it.
			let (proof, alias) = proof(&members[2].0, &keys, 0, PURSE + 2);
			assert_eq!(
				authorize(0, alias, proof.clone(), COLLECTION, PURSE + 2),
				Err(InvalidTransaction::Future.into())
			);

			// The allowance reopens with the next block.
			assert_eq!(PrivateClaimsThisBlock::<Test>::get(), 2);
			NftClaims::on_initialize(System::block_number() + 1);
			assert_eq!(PrivateClaimsThisBlock::<Test>::get(), 0);
			assert_ok!(claim(0, alias, proof, COLLECTION, PURSE + 2));
		});
	}

	#[test]
	fn the_window_opens_and_closes_the_games_claims() {
		new_test_ext().execute_with(|| {
			let (secret, key) = member(1);
			let keys = [key, member(2).1];
			let received = System::block_number();
			store_ring(1, &keys);

			// Two blocks of delay and ten of window in the mock, counted from the delivery.
			let ring = PrivateRings::<Test>::get(GAME).unwrap();
			assert_eq!(ring.opens_at, received + 2);
			assert_eq!(ring.closes_at, received + 12);
			System::assert_has_event(
				Event::PrivateRingReceived {
					game_index: GAME,
					slots: 1,
					key_count: 2,
					opens_at: ring.opens_at,
					closes_at: ring.closes_at,
				}
				.into(),
			);

			// A claim before the window opens waits in the pool: the opening block is what
			// makes it valid, so it is `Future` and not a custom invalidity.
			System::set_block_number(ring.opens_at - 1);
			let (early_proof, early_alias) = proof(&secret, &keys, 0, PURSE);
			assert_eq!(
				authorize(0, early_alias, early_proof.clone(), COLLECTION, PURSE),
				Err(InvalidTransaction::Future.into())
			);

			// The last block of the window still takes it.
			System::set_block_number(ring.closes_at - 1);
			assert_ok!(claim(0, early_alias, early_proof, COLLECTION, PURSE));

			// The next one does not, and never will, so it is dropped rather than kept.
			System::set_block_number(ring.closes_at);
			let (late_proof, late_alias) = proof(&member(2).0, &keys, 0, PURSE + 1);
			assert_eq!(
				authorize(0, late_alias, late_proof, COLLECTION, PURSE + 1),
				Err(AuthorizeInvalidity::PrivateClaimWindowClosed.into())
			);
		});
	}

	#[test]
	fn a_redelivered_ring_does_not_extend_the_window() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			let ring = PrivateRings::<Test>::get(GAME).unwrap();

			// The same ring again, blocks later. A window that moved with a redelivery would
			// leave the last claims of the game standing alone in time.
			System::set_block_number(ring.closes_at - 1);
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![private_ring_delivery(GAME, 1, &keys)])
			));

			assert_eq!(PrivateRings::<Test>::get(GAME).unwrap(), ring);
			assert!(!System::events().iter().any(|record| matches!(
				record.event,
				RuntimeEvent::NftClaims(Event::PrivateOutcomeConflict { .. })
			)));
		});
	}

	#[test]
	fn a_closed_window_drops_the_ring_and_its_spent_aliases() {
		new_test_ext().execute_with(|| {
			let members = [member(1), member(2)];
			let keys = members.iter().map(|(_, key)| *key).collect::<Vec<_>>();
			store_ring(1, &keys);
			for (index, (secret, _)) in members.iter().enumerate() {
				let purse = PURSE + index as u64;
				let (proof, alias) = proof(secret, &keys, 0, purse);
				assert_ok!(claim(0, alias, proof, COLLECTION, purse));
			}
			assert_eq!(SpentPrivateClaims::<Test>::iter_prefix(GAME).count(), 2);

			// The window is what allows the removal, so nothing goes while it is open. The
			// closing block alone makes the call valid, so `authorize` keeps it in the pool.
			assert_noop!(close(), Error::<Test>::PrivateClaimWindowOpen);
			assert_eq!(authorize_close(), Err(InvalidTransaction::Future.into()));

			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;
			System::set_block_number(closes_at);
			// The two claims above filled the block's allowance, which the next block reopens.
			NftClaims::on_initialize(closes_at);
			let post = close().unwrap();

			assert_eq!(SpentPrivateClaims::<Test>::iter_prefix(GAME).count(), 0);
			assert!(PrivateRings::<Test>::get(GAME).is_none());
			System::assert_has_event(Event::PrivateRingClosed { game_index: GAME }.into());

			// Two of the budget's thirty-two aliases and none of its trees, refunded down to
			// what the call removed.
			let call_weight =
				crate::Call::<Test>::close_private_ring { game_index: GAME, discriminator: 1 }
					.get_dispatch_info()
					.call_weight;
			let actual = post.actual_weight.expect("the call reports its weight");
			assert_eq!(actual, MockWeightInfo::close_private_ring(2, 0));
			assert!(actual.all_lt(call_weight));

			// Nothing is left to close, and a claim of the game is refused on the ring rather
			// than on the window.
			assert_noop!(close(), Error::<Test>::UnknownPrivateRing);
			assert_eq!(authorize_close(), Err(AuthorizeInvalidity::NoPrivateRingToClose.into()));
			let (proof, alias) = proof(&members[0].0, &keys, 0, PURSE + 9);
			assert_eq!(
				authorize(0, alias, proof, COLLECTION, PURSE + 9),
				Err(AuthorizeInvalidity::UnknownPrivateRing.into())
			);
		});
	}

	#[test]
	fn closing_keeps_the_ring_until_the_last_alias_is_removed() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);

			// One more alias than a call removes, so the first step spends its whole budget.
			for i in 0..=PRIVATE_CLOSE_ITEMS {
				SpentPrivateClaims::<Test>::insert(GAME, Alias::from([i as u8; 32]), ());
			}
			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;
			System::set_block_number(closes_at);

			let post = close().unwrap();
			assert_eq!(
				post.actual_weight,
				Some(MockWeightInfo::close_private_ring(PRIVATE_CLOSE_ITEMS, 0)),
				"a full budget refunds nothing",
			);
			assert_eq!(SpentPrivateClaims::<Test>::iter_prefix(GAME).count(), 1);
			assert!(
				PrivateRings::<Test>::contains_key(GAME),
				"the ring says the removal is still owed",
			);

			assert_ok!(close());
			assert_eq!(SpentPrivateClaims::<Test>::iter_prefix(GAME).count(), 0);
			assert!(PrivateRings::<Test>::get(GAME).is_none());
		});
	}

	#[test]
	fn a_closed_game_takes_no_further_outcome() {
		new_test_ext().execute_with(|| {
			let members = [member(1), member(2)];
			let keys = members.iter().map(|(_, key)| *key).collect::<Vec<_>>();
			store_ring(1, &keys);
			let (proof, alias) = proof(&members[0].0, &keys, 0, PURSE);
			assert_ok!(claim(0, alias, proof, COLLECTION, PURSE));

			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;
			System::set_block_number(closes_at);
			NftClaims::on_initialize(closes_at);
			assert_ok!(close());
			assert!(ClosedPrivateGames::<Test>::contains_key(GAME));

			// The same ring again. Its spent aliases went with it, so a fresh window over the
			// same keys would mint every slot of the game a second time.
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![private_ring_delivery(GAME, 1, &keys)])
			));
			assert!(PrivateRings::<Test>::get(GAME).is_none());
			System::assert_has_event(Event::PrivateOutcomeConflict { game_index: GAME }.into());

			// Nor does an abandonment reopen the public path for a game that held a ring.
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![abandoned_delivery(GAME, 1, 2)])
			));
			assert!(!AbandonedPrivateGames::<Test>::contains_key(GAME));
		});
	}

	#[test]
	fn the_trees_of_a_private_game_are_indexed_for_its_close() {
		new_test_ext().execute_with(|| {
			store_private_tree(BLOCK);

			// The tree is keyed by its award block, so the index is what the close finds it by.
			assert!(PrivateGameTrees::<Test>::contains_key(GAME, BLOCK));

			// A tree that names no slots is a public game's. Its own claims remove it, so it
			// takes no index entry.
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![update(0, 11)])
			));
			assert!(CreditTrees::<Test>::contains_key(11));
			assert!(!PrivateGameTrees::<Test>::contains_key(GAME, 11));
		});
	}

	#[test]
	fn closing_removes_the_games_trees_and_queues_their_deletion() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			store_private_tree(BLOCK);
			System::reset_events();

			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;
			System::set_block_number(closes_at);
			let post = close().unwrap();

			// The game's credits minted through its ring or not at all, so the tree is gone at
			// the close rather than at its own deadline.
			assert!(!CreditTrees::<Test>::contains_key(BLOCK));
			assert!(!PrivateGameTrees::<Test>::contains_key(GAME, BLOCK));
			assert_eq!(PendingTreeDeletions::<Test>::get().into_inner(), vec![BLOCK]);
			assert_eq!(post.actual_weight, Some(MockWeightInfo::close_private_ring(0, 1)));
			assert_eq!(
				nft_claims_events(),
				vec![
					Event::PrivateGameTreesRemoved { game_index: GAME, count: 1 },
					Event::PrivateRingClosed { game_index: GAME },
				],
				"the removal reports the close, not an expiry of unclaimed credits",
			);

			// The expiry entry stays behind, as it does for a fully claimed tree, and the sweep
			// of it is what removes the bitmap.
			assert!(TreeExpiries::<Test>::contains_key(
				ExpiryTimestamp::from(1_000 + BLOCK),
				BLOCK
			));
		});
	}

	#[test]
	fn closing_keeps_the_ring_until_the_last_tree_is_removed() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			// One more tree than a step removes, the mock carrying two per deletion message.
			for block in BLOCK..BLOCK + 3 {
				store_private_tree(block);
			}
			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;
			System::set_block_number(closes_at);

			let post = close().unwrap();
			assert_eq!(
				post.actual_weight,
				Some(MockWeightInfo::close_private_ring(0, 2)),
				"a full budget refunds nothing",
			);
			assert_eq!(PrivateGameTrees::<Test>::iter_prefix(GAME).count(), 1);
			assert_eq!(CreditTrees::<Test>::iter().count(), 1);
			assert!(
				PrivateRings::<Test>::contains_key(GAME),
				"the ring says the removal is still owed",
			);

			assert_ok!(close());
			assert_eq!(PrivateGameTrees::<Test>::iter_prefix(GAME).count(), 0);
			assert_eq!(CreditTrees::<Test>::iter().count(), 0);
			assert!(PrivateRings::<Test>::get(GAME).is_none());
			assert_eq!(PendingTreeDeletions::<Test>::get().len(), 3);
		});
	}

	#[test]
	fn a_tree_of_a_closed_game_is_not_stored() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;
			System::set_block_number(closes_at);
			assert_ok!(close());
			assert!(ClosedPrivateGames::<Test>::contains_key(GAME));
			System::reset_events();

			// A replay from the game chain, whose root outlives this chain's tree. The game's
			// ring and its spent aliases are gone, so the tree would take a claim on neither
			// path and nothing but the sweep would remove it.
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![private_update(BLOCK)])
			));

			assert!(!CreditTrees::<Test>::contains_key(BLOCK));
			assert!(!PrivateGameTrees::<Test>::contains_key(GAME, BLOCK));
			assert_eq!(
				nft_claims_events(),
				vec![
					Event::CreditTreePrivateGameClosed { block: BLOCK },
					Event::CreditTreesReceived { count: 1, stored: 0 },
				]
			);

			// The refusal is on the slots the tree names. A tree that names none is a public
			// game's and is stored as any other.
			assert_ok!(NftClaims::receive_credit_trees(
				game_chain_origin(),
				batch(vec![replay(BLOCK)])
			));
			assert!(CreditTrees::<Test>::contains_key(BLOCK));
		});
	}

	#[test]
	fn the_offchain_worker_submits_a_close_for_a_shut_window() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;

			// Nothing goes out while the window is open. The pool would hold such a step as
			// `Future` every block until the window closed.
			run_offchain_worker(closes_at - 1);
			assert_eq!(submitted_calls(), vec![]);

			run_offchain_worker(closes_at);
			run_offchain_worker(closes_at + 1);

			// The discriminator is the submitting block, so the steps of two blocks differ while
			// no step has been included. `game_index` alone cannot tell them apart, and a
			// repeated encoding gives a hash the pool has banned.
			assert_eq!(
				submitted_calls(),
				vec![
					RuntimeCall::NftClaims(crate::Call::close_private_ring {
						game_index: GAME,
						discriminator: closes_at
					}),
					RuntimeCall::NftClaims(crate::Call::close_private_ring {
						game_index: GAME,
						discriminator: closes_at + 1
					}),
				]
			);
		});
	}

	#[test]
	fn a_close_is_authorized_for_a_local_source_only() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			System::set_block_number(PrivateRings::<Test>::get(GAME).unwrap().closes_at);

			// Only this pallet's own offchain worker submits a close, so a gossiped one is
			// refused whatever the state says.
			assert_eq!(
				NftClaims::authorize_close_private_ring(TransactionSource::External, &GAME)
					.map(|_| ()),
				Err(AuthorizeInvalidity::TransactionNotLocal.into())
			);
			assert_ok!(authorize_close());
		});
	}

	#[test]
	fn a_ring_is_filed_under_the_block_it_closes_in_and_unfiled_when_it_goes() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;

			// The offchain worker reads this entry rather than every ring, so the ring is filed
			// under its own closing block for as long as it is held.
			assert_eq!(
				PrivateRingCloses::<Test>::iter().collect::<Vec<_>>(),
				vec![(BigEndianU64(closes_at), GAME, ())]
			);
			assert_ok!(NftClaims::do_try_state());

			System::set_block_number(closes_at);
			assert_ok!(close());

			assert!(PrivateRings::<Test>::get(GAME).is_none());
			assert_eq!(
				PrivateRingCloses::<Test>::iter().count(),
				0,
				"the entry goes with the ring"
			);
			assert_ok!(NftClaims::do_try_state());
		});
	}

	#[test]
	fn the_offchain_worker_closes_the_earliest_window_first() {
		new_test_ext().execute_with(|| {
			const LATER_GAME: crate::GameIdx = GAME + 1;

			// Two rings a block apart, so the second closes a block after the first. The index
			// iterates in closing order whatever order the games hash in.
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			let first_closes = PrivateRings::<Test>::get(GAME).unwrap().closes_at;
			System::set_block_number(System::block_number() + 1);
			assert_ok!(NftClaims::receive_private_rings(
				game_chain_origin(),
				private_ring_batch(vec![private_ring_delivery(LATER_GAME, 1, &keys)])
			));
			let later_closes = PrivateRings::<Test>::get(LATER_GAME).unwrap().closes_at;
			assert!(later_closes > first_closes);

			// The later game's window is still open, so it waits even though its entry is filed.
			run_offchain_worker(first_closes);
			assert_eq!(
				submitted_calls(),
				vec![RuntimeCall::NftClaims(crate::Call::close_private_ring {
					game_index: GAME,
					discriminator: first_closes
				})],
				"the earliest window goes first and the open one is left alone",
			);

			// With the first game closed the index hands the worker the next one.
			System::set_block_number(later_closes);
			assert_ok!(close());
			assert!(!PrivateRingCloses::<Test>::contains_key(BigEndianU64(first_closes), GAME));
			run_offchain_worker(later_closes);
			assert_eq!(
				submitted_calls().last(),
				Some(&RuntimeCall::NftClaims(crate::Call::close_private_ring {
					game_index: LATER_GAME,
					discriminator: later_closes
				}))
			);
		});
	}

	#[test]
	fn try_state_catches_a_close_index_that_disagrees_with_the_rings() {
		new_test_ext().execute_with(|| {
			let keys = [member(1).1, member(2).1];
			store_ring(1, &keys);
			let closes_at = PrivateRings::<Test>::get(GAME).unwrap().closes_at;

			// Without the entry the worker never finds the game, so its ring is never closed.
			PrivateRingCloses::<Test>::remove(BigEndianU64(closes_at), GAME);
			assert!(NftClaims::do_try_state().is_err());

			// Filed under the wrong block, the worker submits the close too early or too late.
			PrivateRingCloses::<Test>::insert(BigEndianU64(closes_at + 1), GAME, ());
			assert!(NftClaims::do_try_state().is_err());
			PrivateRingCloses::<Test>::remove(BigEndianU64(closes_at + 1), GAME);
			PrivateRingCloses::<Test>::insert(BigEndianU64(closes_at), GAME, ());
			assert_ok!(NftClaims::do_try_state());

			// An entry naming no ring leaves the worker submitting a close `authorize` refuses.
			PrivateRingCloses::<Test>::insert(BigEndianU64(closes_at), GAME + 1, ());
			assert!(NftClaims::do_try_state().is_err());
		});
	}

	#[test]
	fn the_last_claim_of_an_abandoned_games_tree_clears_the_index() {
		new_test_ext().execute_with(|| {
			// The game built no ring, so its trees mint over the public path. They are indexed
			// as every private game's trees are, and its close never runs to remove them.
			let awards = vec![(AccountOrPerson::Account(ALICE), [1u8; 32])];
			let mut private_tree = tree_of(BLOCK, &awards);
			private_tree.private_slots = 2;
			CreditTrees::<Test>::insert(BLOCK, private_tree);
			PrivateGameTrees::<Test>::insert(GAME, BLOCK, ());
			AbandonedPrivateGames::<Test>::insert(GAME, ());
			add_collection(COLLECTION, COLLECTION_OWNER, 2);
			CollectionMinters::<Test>::insert(
				COLLECTION,
				CollectionMinter { owner: COLLECTION_OWNER, selection: ItemSelection::Random },
			);

			assert_ok!(NftClaims::claim(
				RuntimeOrigin::signed(ALICE),
				ClaimantKind::Account,
				BLOCK,
				[1u8; 32],
				0,
				proof_of(&awards, 0),
				COLLECTION,
				PURSE
			));

			// The claim took the tree's only leaf, so the tree goes with it and the index entry
			// it left would name a tree nothing holds.
			assert!(!CreditTrees::<Test>::contains_key(BLOCK));
			assert!(!PrivateGameTrees::<Test>::contains_key(GAME, BLOCK));
			System::assert_has_event(Event::TreeFullyClaimed { block: BLOCK }.into());
		});
	}

	#[test]
	fn a_private_games_tree_takes_no_public_claim() {
		new_test_ext().execute_with(|| {
			// The tree says the game is private, so the public path is closed before the game's
			// ring arrives.
			let awards = vec![(AccountOrPerson::Account(ALICE), [1u8; 32])];
			let mut private_tree = tree_of(BLOCK, &awards);
			private_tree.private_slots = 2;
			CreditTrees::<Test>::insert(BLOCK, private_tree);
			add_collection(COLLECTION, COLLECTION_OWNER, 2);
			CollectionMinters::<Test>::insert(
				COLLECTION,
				CollectionMinter { owner: COLLECTION_OWNER, selection: ItemSelection::Random },
			);

			assert_claim_noop!(
				NftClaims::claim(
					RuntimeOrigin::signed(ALICE),
					ClaimantKind::Account,
					BLOCK,
					[1u8; 32],
					0,
					BoundedVec::default(),
					COLLECTION,
					PURSE
				),
				Error::<Test>::PrivateGame
			);
		});
	}
}
