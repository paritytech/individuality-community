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

//! NFT claims pallet benchmarking.

use super::*;
use crate::{
	pallet::{
		AbandonedPrivateGames, ClaimedLeaves, CollectionMinters, NextExpectedSequence,
		PendingTreeDeletions, PrivateGameTrees, PrivateRingCloses, TreeExpiries,
	},
	types::CreditTreeBatch,
	BenchmarkHelper,
};
use alloc::vec::Vec;
use frame_benchmarking::{v2::*, BenchmarkError};
use frame_support::{
	pallet_prelude::TransactionSource,
	traits::{EnsureOrigin, EnsureOriginWithArg},
	BoundedVec,
};
use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
use indiv_support::{credit_trees::CreditTreeDelivery, utils::BigEndianU64};
use sp_runtime::traits::{Bounded, Zero};

/// The `i`-th distinct credit a benchmarked tree commits to.
fn credit(i: u32) -> NftClaimCredit {
	let mut credit = [0u8; 32];
	credit[..4].copy_from_slice(&i.to_le_bytes());
	credit
}

/// A batch of `n` trees of the live stream, one per award block, as the game pallet sends it.
///
/// The sequence numbers start at one, so a receiver still expecting zero sees the batch as
/// ahead of the stream.
///
/// Every tree names a private game of its own, which is the dearest batch to store: a tree that
/// names slots reads `ClosedPrivateGames` and writes `PrivateGameTrees`, and a game per tree gives
/// each of those a key of its own.
fn batch<T: Config>(n: u32) -> CreditTreeBatch<T> {
	let mut trees = BoundedVec::new();
	for i in 0..n {
		trees
			.try_push(CreditTreeDelivery {
				sequence: Some(i.saturating_add(1) as TreeSequence),
				block: i,
				tree: NftClaimCreditTree {
					private_slots: 1,
					game_index: i.saturating_add(1),
					// Distinct per tree and never the zero root the pallet skips as invalid.
					root: CreditProofNode([i.saturating_add(1) as u8; 32]),
					leaf_count: 1,
					timestamp: 1_000,
				},
			})
			.expect("n is bounded by MaxTreesPerMessage; qed");
	}

	CreditTreeBatch::<T> { source_time: 1_000, trees }
}

#[benchmarks]
mod benches {
	use super::*;

	/// Worst case: every tree in the batch is new, so each one is written, and the batch is
	/// ahead of the expected sequence, so the gap is reported as well.
	#[benchmark]
	fn receive_credit_trees(
		n: Linear<1, { T::MaxTreesPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		NextExpectedSequence::<T>::put(0);

		let batch = batch::<T>(n);
		let origin = T::EnsureGameChainOrigin::try_successful_origin()
			.map_err(|_| BenchmarkError::Stop("failed to construct game chain origin"))?;

		#[extrinsic_call]
		_(origin as T::RuntimeOrigin, batch);

		assert_eq!(CreditTrees::<T>::iter().count(), n as usize);
		assert_eq!(NextExpectedSequence::<T>::get(), n.saturating_add(1) as TreeSequence);

		Ok(())
	}

	/// A claim that leaves credits behind, which is the weight such a claim is refunded down to.
	/// The tree holds `2^n` leaves, so the proof carries `n` sibling hashes and the call rehashes
	/// every one of them. Nothing else has been claimed of that tree.
	///
	/// The component starts at one because a one-leaf tree has no such claim. Its first claim is
	/// its last, and only a proof of no sibling hashes verifies against it. It stops at the
	/// largest power of two within [`Config::MaxCreditsPerAwardBlock`], which is the biggest tree
	/// this chain stores.
	///
	/// The claim is made under [`ClaimantKind::Account`], the kind whose origin check takes the
	/// signing account as it stands.
	///
	/// The collection is registered with [`ItemSelection::Random`], which is the branch this
	/// weight stands for: a contract selection adds the runtime selector's metered weight on
	/// top, reserved and refunded outside this function.
	#[benchmark]
	fn claim_account(
		n: Linear<1, { T::MaxCreditsPerAwardBlock::get().ilog2() }>,
	) -> Result<(), BenchmarkError> {
		let kind = ClaimantKind::Account;
		let (origin, _claimant, credits, leaf_index, sibling_hashes) =
			claimable_tree::<T>(&kind, n)?;
		let mint_to: T::AccountId = account("purse", 0, 0);

		#[extrinsic_call]
		claim(
			origin as T::RuntimeOrigin,
			kind,
			BLOCK,
			credits[leaf_index as usize],
			leaf_index,
			sibling_hashes,
			COLLECTION,
			mint_to,
		);

		assert_eq!(Pallet::<T>::claimed_leaf_count(&ClaimedLeaves::<T>::get(BLOCK)), 1);
		assert!(CreditTrees::<T>::contains_key(BLOCK), "the tree still has credits to claim");

		Ok(())
	}

	/// As `claim_account`, except that resolving [`ClaimantKind::Person`] has to look the signer's
	/// alias up rather than take the account as it stands.
	#[benchmark]
	fn claim_person(
		n: Linear<1, { T::MaxCreditsPerAwardBlock::get().ilog2() }>,
	) -> Result<(), BenchmarkError> {
		let kind = ClaimantKind::Person;
		let (origin, _claimant, credits, leaf_index, sibling_hashes) =
			claimable_tree::<T>(&kind, n)?;
		let mint_to: T::AccountId = account("purse", 0, 0);

		#[extrinsic_call]
		claim(
			origin as T::RuntimeOrigin,
			kind,
			BLOCK,
			credits[leaf_index as usize],
			leaf_index,
			sibling_hashes,
			COLLECTION,
			mint_to,
		);

		assert_eq!(Pallet::<T>::claimed_leaf_count(&ClaimedLeaves::<T>::get(BLOCK)), 1);
		assert!(CreditTrees::<T>::contains_key(BLOCK), "the tree still has credits to claim");

		Ok(())
	}

	/// Worst case: as `claim_account`, but this claim spends the tree's last credit. It therefore
	/// also removes the tree and queues the deletion the game chain is owed. The expiry entry and
	/// the bitmap stay for the sweep to take.
	#[benchmark]
	fn claim_last_account(
		n: Linear<0, { T::MaxCreditsPerAwardBlock::get().ilog2() }>,
	) -> Result<(), BenchmarkError> {
		let kind = ClaimantKind::Account;
		let (origin, _claimant, credits, leaf_index, sibling_hashes) =
			claimable_tree::<T>(&kind, n)?;
		let mint_to: T::AccountId = account("purse", 0, 0);
		// Every other leaf is spent, so this claim completes the tree.
		spend_every_leaf_but::<T>(leaf_index);

		#[extrinsic_call]
		claim(
			origin as T::RuntimeOrigin,
			kind,
			BLOCK,
			credits[leaf_index as usize],
			leaf_index,
			sibling_hashes,
			COLLECTION,
			mint_to,
		);

		assert!(!CreditTrees::<T>::contains_key(BLOCK), "the fully claimed tree is removed");
		assert!(Pallet::<T>::leaf_is_claimed(&ClaimedLeaves::<T>::get(BLOCK), leaf_index));
		assert_eq!(PendingTreeDeletions::<T>::get().to_vec(), alloc::vec![BLOCK]);

		Ok(())
	}

	/// Worst case: as `claim_last_account`, except that resolving [`ClaimantKind::Person`] has to
	/// look the signer's alias up rather than take the account as it stands.
	#[benchmark]
	fn claim_last_person(
		n: Linear<0, { T::MaxCreditsPerAwardBlock::get().ilog2() }>,
	) -> Result<(), BenchmarkError> {
		let kind = ClaimantKind::Person;
		let (origin, _claimant, credits, leaf_index, sibling_hashes) =
			claimable_tree::<T>(&kind, n)?;
		let mint_to: T::AccountId = account("purse", 0, 0);
		// Every other leaf is spent, so this claim completes the tree.
		spend_every_leaf_but::<T>(leaf_index);

		#[extrinsic_call]
		claim(
			origin as T::RuntimeOrigin,
			kind,
			BLOCK,
			credits[leaf_index as usize],
			leaf_index,
			sibling_hashes,
			COLLECTION,
			mint_to,
		);

		assert!(!CreditTrees::<T>::contains_key(BLOCK), "the fully claimed tree is removed");
		assert!(Pallet::<T>::leaf_is_claimed(&ClaimedLeaves::<T>::get(BLOCK), leaf_index));
		assert_eq!(PendingTreeDeletions::<T>::get().to_vec(), alloc::vec![BLOCK]);

		Ok(())
	}

	/// Worst case for `n` removals: `n` trees are due, each under a timestamp of its own, so the
	/// call pays for every removal and for one map key per tree. A tree that is not due follows
	/// them, which is the entry the sweep reads to stop.
	#[benchmark]
	fn sweep_expired_trees(
		n: Linear<0, { T::MaxTreeDeletionsPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		fill_due_expiries::<T>(n);
		let origin = RawOrigin::Authorized;

		#[extrinsic_call]
		_(origin, FIRST_EXPIRY_TIMESTAMP, BlockNumberFor::<T>::from(0u32));

		assert_eq!(PendingTreeDeletions::<T>::get().len(), n as usize);
		// Only the tree that is not due is left, which is the one filed last.
		assert_eq!(TreeExpiries::<T>::iter().count(), 1);
		assert_eq!(
			oldest_expiry::<TreeExpiries<T>, AwardBlock>(),
			Some(FIRST_EXPIRY_TIMESTAMP.saturating_add(n))
		);

		Ok(())
	}

	/// Authorizing a sweep reads the oldest entry and the clock. The map holds one sweep's worth
	/// of entries, each under a key of its own, which is the state a sweep is submitted against.
	#[benchmark]
	fn authorize_sweep_expired_trees() -> Result<(), BenchmarkError> {
		fill_due_expiries::<T>(T::MaxTreeDeletionsPerMessage::get());

		#[block]
		{
			Pallet::<T>::authorize_sweep_expired_trees(
				TransactionSource::Local,
				&FIRST_EXPIRY_TIMESTAMP,
			)
			.expect("must authorize");
		}

		Ok(())
	}

	/// A message that carries `n` deletions. The queue holds exactly `n`, so the message is the
	/// size the component names.
	///
	/// A read and a write of the queue cost its `MaxEncodedLen` whatever it holds. The cost of
	/// rewriting a remainder therefore sits in the base, not in `n`.
	///
	/// The channel is sized to [`MIN_CHANNEL_MESSAGE_SIZE`], the room the `integrity_test` holds a
	/// full deletion message to, so the message the send builds fits it.
	#[benchmark]
	fn send_tree_deletions(
		n: Linear<1, { T::MaxTreeDeletionsPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		T::BenchmarkHelper::open_game_chain_channel(MIN_CHANNEL_MESSAGE_SIZE as u32);
		queue_deletions::<T>(n);
		let origin = RawOrigin::Authorized;

		// `queue_deletions` fills the queue from block zero, which is the front it leaves.
		#[extrinsic_call]
		_(origin, 0, BlockNumberFor::<T>::from(0u32));

		assert!(PendingTreeDeletions::<T>::get().is_empty(), "the message went out");

		Ok(())
	}

	/// Authorizing a send decodes the whole queue, so the benchmark fills it to
	/// `MaxQueuedTreeDeletions`, which is the worst case a backlog of deletions leaves behind.
	#[benchmark]
	fn authorize_send_tree_deletions() -> Result<(), BenchmarkError> {
		queue_deletions::<T>(T::MaxQueuedTreeDeletions::get());

		#[block]
		{
			Pallet::<T>::authorize_send_tree_deletions(TransactionSource::Local, &0)
				.expect("must authorize");
		}

		Ok(())
	}

	/// Worst case: every ring in the batch is new, so each one is written.
	#[benchmark]
	fn receive_private_rings(
		n: Linear<1, { T::MaxPrivateRingsPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		let (root, _proof, _alias) = <T as Config>::BenchmarkHelper::private_ring_and_proof(
			&pallet::Pallet::<T>::private_claim_context(1, 0),
			&pallet::Pallet::<T>::private_claim_message(0, &whitelisted_caller()),
		);
		let rings = (0..n)
			.map(|game_index| PrivateRingDelivery {
				game_index,
				slots: 1,
				outcome: PrivateGameOutcome::Ring { root: root.clone(), key_count: 1 },
			})
			.collect::<Vec<_>>();
		let batch = crate::PrivateRingBatchOf::<T> {
			source_time: 1_000,
			rings: BoundedVec::try_from(rings)
				.map_err(|_| BenchmarkError::Stop("n is bounded by MaxPrivateRingsPerMessage"))?,
		};
		let origin = T::EnsureGameChainOrigin::try_successful_origin()
			.map_err(|_| BenchmarkError::Stop("failed to construct game chain origin"))?;

		#[extrinsic_call]
		_(origin as T::RuntimeOrigin, batch);

		assert_eq!(PrivateRings::<T>::iter().count(), n as usize);

		Ok(())
	}

	/// Worst case: the alias is unspent, so the claim spends it and mints.
	///
	/// The ring VRF verification is not measured here. `authorize` runs it and
	/// `authorize_claim_private` charges it. The collection is registered with
	/// [`ItemSelection::Random`], as in `claim_account`, a contract selection's weight being
	/// reserved and refunded outside this function.
	#[benchmark]
	fn claim_private() -> Result<(), BenchmarkError> {
		let game_index = 1;
		let slot = 0;
		let mint_to: T::AccountId = account("mint_to", 0, 0);

		let owner: T::AccountId = account("collection-owner", 0, 0);
		let collection: CollectionId = 0;
		let (root, proof, alias) = <T as Config>::BenchmarkHelper::private_ring_and_proof(
			&pallet::Pallet::<T>::private_claim_context(game_index, slot),
			&pallet::Pallet::<T>::private_claim_message(collection, &mint_to),
		);
		PrivateRings::<T>::insert(
			game_index,
			crate::PrivateRing {
				root,
				slots: 1,
				key_count: 1,
				opens_at: BlockNumberFor::<T>::zero(),
				closes_at: BlockNumberFor::<T>::max_value(),
			},
		);
		// One item, so the random draw resolves to it whatever the alias's bytes are.
		T::BenchmarkHelper::prepare_collection(&owner, collection, 0);
		CollectionMinters::<T>::insert(
			collection,
			CollectionMinter { owner, selection: ItemSelection::Random },
		);

		#[extrinsic_call]
		_(RawOrigin::Authorized, game_index, slot, alias, proof, collection, mint_to);

		assert_eq!(SpentPrivateClaims::<T>::iter_prefix(game_index).count(), 1);

		Ok(())
	}

	/// Worst case: every check passes, so the whole ring VRF verification runs. It takes no
	/// component, a ring proof costing the same whatever the ring holds.
	#[benchmark]
	fn authorize_claim_private() -> Result<(), BenchmarkError> {
		let game_index = 1;
		let slot = 0;
		let mint_to: T::AccountId = account("mint_to", 0, 0);
		let collection: CollectionId = 0;
		let (root, proof, alias) = <T as Config>::BenchmarkHelper::private_ring_and_proof(
			&pallet::Pallet::<T>::private_claim_context(game_index, slot),
			&pallet::Pallet::<T>::private_claim_message(collection, &mint_to),
		);
		PrivateRings::<T>::insert(
			game_index,
			crate::PrivateRing {
				root,
				slots: 1,
				key_count: 1,
				opens_at: BlockNumberFor::<T>::zero(),
				closes_at: BlockNumberFor::<T>::max_value(),
			},
		);

		#[block]
		{
			pallet::Pallet::<T>::authorize_claim_private(
				TransactionSource::External,
				&game_index,
				&slot,
				&alias,
				&proof,
				&collection,
				&mint_to,
			)
			.expect("the proof verifies against the ring it was made for");
		}

		Ok(())
	}

	/// Worst case for `a` aliases and `t` trees: both budgets are filled, so the ring stays and
	/// the call refunds nothing.
	///
	/// One key per alias and one award block per tree, because the entries removed are the cost
	/// drivers: a fixed set of them would charge one write however many a real call makes. The
	/// two do not interact, one being a removal per spent alias and the other a removal per tree
	/// the game's award blocks hold, so they are separate components.
	#[benchmark]
	fn close_private_ring(
		a: Linear<0, PRIVATE_CLOSE_ITEMS>,
		t: Linear<0, { T::MaxTreeDeletionsPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		let game_index = 1;
		let caller: T::AccountId = account("caller", 0, 0);
		let (root, _proof, _alias) = <T as Config>::BenchmarkHelper::private_ring_and_proof(
			&pallet::Pallet::<T>::private_claim_context(game_index, 0),
			&pallet::Pallet::<T>::private_claim_message(0, &caller),
		);
		// A window that closed in the block the ring arrived in, which is the state the call is
		// allowed in.
		PrivateRings::<T>::insert(
			game_index,
			crate::PrivateRing {
				root,
				slots: 1,
				key_count: 1,
				opens_at: BlockNumberFor::<T>::zero(),
				closes_at: BlockNumberFor::<T>::zero(),
			},
		);
		// Filed as `receive_private_rings` files it, so the step that drops the ring pays for
		// clearing the entry the offchain worker found it by.
		PrivateRingCloses::<T>::insert(BigEndianU64(0), game_index, ());
		for i in 0..a {
			let mut alias = [0u8; 32];
			alias[..4].copy_from_slice(&i.to_le_bytes());
			SpentPrivateClaims::<T>::insert(game_index, Alias::from(alias), ());
		}
		for block in 0..t {
			CreditTrees::<T>::insert(
				block,
				NftClaimCreditTree {
					game_index,
					root: CreditProofNode([block as u8; 32]),
					leaf_count: 1,
					timestamp: 1_000,
					private_slots: 1,
				},
			);
			PrivateGameTrees::<T>::insert(game_index, block, ());
		}

		#[extrinsic_call]
		_(RawOrigin::Authorized, game_index, BlockNumberFor::<T>::from(0u32));

		assert_eq!(
			SpentPrivateClaims::<T>::iter_prefix(game_index).count(),
			0,
			"one call removes the whole alias budget",
		);
		assert_eq!(
			PrivateGameTrees::<T>::iter_prefix(game_index).count(),
			0,
			"one call removes the whole tree budget",
		);
		assert_eq!(PendingTreeDeletions::<T>::get().len(), t as usize);

		Ok(())
	}

	/// Authorizing a close reads the game's ring, which is the whole of the state the check
	/// looks at. It takes no component, a ring costing the same whatever it holds.
	#[benchmark]
	fn authorize_close_private_ring() -> Result<(), BenchmarkError> {
		let game_index = 1;
		let caller: T::AccountId = account("caller", 0, 0);
		let (root, _proof, _alias) = <T as Config>::BenchmarkHelper::private_ring_and_proof(
			&pallet::Pallet::<T>::private_claim_context(game_index, 0),
			&pallet::Pallet::<T>::private_claim_message(0, &caller),
		);
		PrivateRings::<T>::insert(
			game_index,
			crate::PrivateRing {
				root,
				slots: 1,
				key_count: 1,
				opens_at: BlockNumberFor::<T>::zero(),
				closes_at: BlockNumberFor::<T>::zero(),
			},
		);

		#[block]
		{
			pallet::Pallet::<T>::authorize_close_private_ring(
				TransactionSource::Local,
				&game_index,
			)
			.expect("the window is closed");
		}

		Ok(())
	}

	#[benchmark]
	fn set_collection_minter() -> Result<(), BenchmarkError> {
		let owner: T::AccountId = account("collection-owner", 0, 0);
		let collection: CollectionId = 0;
		T::BenchmarkHelper::prepare_collection(&owner, collection, 0);
		let contract = T::BenchmarkHelper::prepare_contract(&owner);

		#[extrinsic_call]
		_(RawOrigin::Signed(owner.clone()), collection, Some(ItemSelection::Contract(contract)));

		assert_eq!(
			CollectionMinters::<T>::get(collection),
			Some(CollectionMinter { owner, selection: ItemSelection::Contract(contract) })
		);

		Ok(())
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}

const BLOCK: AwardBlock = 1;

const COLLECTION: CollectionId = 0;

/// A tree of `2^n` leaves stored under [`BLOCK`], with the origin of a `kind` claimant and the
/// proof material for claiming its last leaf.
///
/// The last leaf's proof carries a sibling from every layer, so verifying it is the work `n`
/// charges for.
#[allow(clippy::type_complexity)]
fn claimable_tree<T: Config>(
	kind: &ClaimantKind,
	n: u32,
) -> Result<
	(
		T::RuntimeOrigin,
		AccountOrPerson<T::AccountId>,
		Vec<NftClaimCredit>,
		u32,
		BoundedVec<CreditProofNode, T::MaxProofNodes>,
	),
	BenchmarkError,
> {
	let origin = T::EnsureClaimant::try_successful_origin(kind)
		.map_err(|_| BenchmarkError::Stop("failed to construct claimant origin"))?;
	let claimant = T::EnsureClaimant::ensure_origin(origin.clone(), kind)
		.map_err(|_| BenchmarkError::Stop("claimant origin does not resolve"))?;
	let owner: T::AccountId = account("collection-owner", 0, 0);
	// One item, so the random draw picks it whatever the credit's bytes are.
	T::BenchmarkHelper::prepare_collection(&owner, COLLECTION, 0);
	CollectionMinters::<T>::insert(
		COLLECTION,
		CollectionMinter { owner, selection: ItemSelection::Random },
	);

	let leaf_count = 1u32 << n;
	let credits = (0..leaf_count).map(credit).collect::<Vec<_>>();
	let leaves = credits
		.iter()
		.map(|credit| credit_leaf(&claimant, credit))
		.collect::<Vec<NftClaimCreditLeaf>>();
	let leaf_index = leaf_count - 1;
	let proof = binary_merkle_tree::merkle_proof::<BlakeTwo256, _, _>(leaves, leaf_index);

	let timestamp = 1_000;
	let game_index = 1;
	CreditTrees::<T>::insert(
		BLOCK,
		NftClaimCreditTree {
			game_index,
			root: proof.root.into(),
			leaf_count,
			timestamp,
			private_slots: 1,
		},
	);
	TreeExpiries::<T>::insert(ExpiryTimestamp::from(timestamp), BLOCK, ());
	// The tree is a private game's whose ring was abandoned. That is the dearer of the two shapes
	// a claim takes: it reads `AbandonedPrivateGames`, which a tree of a public game does not,
	// and the claim that removes the tree clears its `PrivateGameTrees` entry as well.
	AbandonedPrivateGames::<T>::insert(game_index, ());
	PrivateGameTrees::<T>::insert(game_index, BLOCK, ());

	let sibling_hashes = BoundedVec::try_from(
		proof.proof.into_iter().map(CreditProofNode::from).collect::<Vec<_>>(),
	)
	.map_err(|_| BenchmarkError::Stop("proof exceeds MaxProofNodes"))?;

	Ok((origin, claimant, credits, leaf_index, sibling_hashes))
}

/// The timestamp the first filed tree of a sweep benchmark commits to, which is what the sweep
/// names. Each further tree adds a second to it, so every tree holds a key of its own. The value
/// itself is arbitrary, because `fill_due_expiries` sets the clock from it.
const FIRST_EXPIRY_TIMESTAMP: u32 = 1_000_000;

/// Files `n` trees that are due, each under a timestamp of its own, plus one that is not.
///
/// One timestamp per tree is the worst case: every removal reads and writes a key of its own,
/// where trees sharing a timestamp would share the map's first key. The clock ends at the deadline
/// of the last due tree, so the tree filed after it is what stops the sweep.
///
/// Every tree names a private game of its own, so the sweep pays for clearing a
/// `PrivateGameTrees` entry per tree, which a public game's tree does not have.
fn fill_due_expiries<T: Config>(n: u32) {
	for block in 0..=n {
		let timestamp = FIRST_EXPIRY_TIMESTAMP.saturating_add(block);
		let game_index = block.saturating_add(1);
		CreditTrees::<T>::insert(
			block,
			NftClaimCreditTree {
				game_index,
				root: CreditProofNode([block as u8; 32]),
				leaf_count: 2,
				timestamp,
				private_slots: 1,
			},
		);
		PrivateGameTrees::<T>::insert(game_index, block, ());
		TreeExpiries::<T>::insert(ExpiryTimestamp::from(timestamp), block, ());
		// A partly claimed tree, so the sweep pays for removing a bitmap that is there.
		ClaimedLeaves::<T>::insert(block, BoundedVec::truncate_from(alloc::vec![0b01u8]));
	}
	let last_due = FIRST_EXPIRY_TIMESTAMP.saturating_add(n).saturating_sub(1);
	T::BenchmarkHelper::set_unix_time(expiry_deadline(last_due, T::TreeTtl::get()));
}

/// Marks every leaf of [`BLOCK`]'s tree claimed except `leaf_index`, so the next claim of it
/// completes the tree.
fn spend_every_leaf_but<T: Config>(leaf_index: u32) {
	let leaf_count = CreditTrees::<T>::get(BLOCK).expect("the tree is stored").leaf_count;
	let mut bitmap = alloc::vec![0u8; leaf_count.div_ceil(8) as usize];
	for index in (0..leaf_count).filter(|index| *index != leaf_index) {
		bitmap[(index / 8) as usize] |= 1u8 << (index % 8);
	}
	ClaimedLeaves::<T>::insert(BLOCK, BoundedVec::truncate_from(bitmap));
}

fn queue_deletions<T: Config>(n: u32) {
	let blocks = (0..n.min(T::MaxQueuedTreeDeletions::get())).collect::<Vec<_>>();
	PendingTreeDeletions::<T>::put(BoundedVec::truncate_from(blocks));
}
