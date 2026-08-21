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
		ClaimedLeaves, CollectionMinters, NextExpectedSequence, NextExpiryBucket,
		PendingTreeDeletions, TreeExpiries,
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
use indiv_support::credit_trees::{CreditTreeDelivery, EXPIRY_BUCKET_SECONDS};

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
fn batch<T: Config>(n: u32) -> CreditTreeBatch<T> {
	let mut trees = BoundedVec::new();
	for i in 0..n {
		trees
			.try_push(CreditTreeDelivery {
				sequence: Some(i.saturating_add(1) as TreeSequence),
				block: i,
				tree: NftClaimCreditTree {
					game_index: 1,
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

	/// Worst case for `n` removals: the bucket holds exactly `n` trees, so the call pays for every
	/// removal. Below the limit it also takes the branch that moves the watermark on.
	///
	/// A drain that reaches the limit cannot tell that it emptied the bucket. At `n` equal to
	/// [`Config::MaxTreeDeletionsPerMessage`] that branch is unreachable, and the fit then charges
	/// the call for one write it does not make.
	#[benchmark]
	fn sweep_expired_trees(
		n: Linear<0, { T::MaxTreeDeletionsPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		let bucket = fill_expiry_bucket::<T>(n);
		let origin = RawOrigin::Authorized;

		#[extrinsic_call]
		_(origin, bucket, BlockNumberFor::<T>::from(0u32));

		assert_eq!(PendingTreeDeletions::<T>::get().len(), n as usize);
		assert_eq!(TreeExpiries::<T>::iter_prefix(bucket).count(), 0);
		if n < T::MaxTreeDeletionsPerMessage::get() {
			assert_eq!(NextExpiryBucket::<T>::get(), Some(bucket.saturating_add(1)));
		}

		Ok(())
	}

	/// Authorizing a sweep reads the watermark and the clock. Neither grows with any input, so the
	/// benchmark only needs a bucket whose deadline has passed.
	#[benchmark]
	fn authorize_sweep_expired_trees() -> Result<(), BenchmarkError> {
		let bucket = fill_expiry_bucket::<T>(1);
		T::BenchmarkHelper::set_unix_time(bucket_deadline(bucket, T::TreeTtl::get()));

		#[block]
		{
			Pallet::<T>::authorize_sweep_expired_trees(TransactionSource::Local, &bucket)
				.expect("must authorize");
		}

		Ok(())
	}

	/// A message that carries `n` deletions. The queue holds exactly `n`, so the message is the
	/// size the component names.
	///
	/// A read and a write of the queue cost its `MaxEncodedLen` whatever it holds. The cost of
	/// rewriting a remainder therefore sits in the base, not in `n`.
	#[benchmark]
	fn send_tree_deletions(
		n: Linear<1, { T::MaxTreeDeletionsPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
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
	CreditTrees::<T>::insert(
		BLOCK,
		NftClaimCreditTree { game_index: 1, root: proof.root.into(), leaf_count, timestamp },
	);
	TreeExpiries::<T>::insert(expiry_bucket(timestamp), BLOCK, ());

	let sibling_hashes = BoundedVec::try_from(
		proof.proof.into_iter().map(CreditProofNode::from).collect::<Vec<_>>(),
	)
	.map_err(|_| BenchmarkError::Stop("proof exceeds MaxProofNodes"))?;

	Ok((origin, claimant, credits, leaf_index, sibling_hashes))
}

/// Files `n` trees under one expiry bucket and points the sweep's watermark at it. A sweep of a
/// bucket that has fallen due starts from this state.
fn fill_expiry_bucket<T: Config>(n: u32) -> ExpiryBucket {
	// The first bucket past a whole TTL, so the clock can reach its deadline.
	let bucket = expiry_bucket(T::TreeTtl::get().saturated_into::<u32>()).saturating_add(1);
	let timestamp = bucket.saturating_mul(EXPIRY_BUCKET_SECONDS);

	for block in 0..n {
		CreditTrees::<T>::insert(
			block,
			NftClaimCreditTree {
				game_index: 1,
				root: CreditProofNode([block as u8; 32]),
				leaf_count: 2,
				timestamp,
			},
		);
		TreeExpiries::<T>::insert(bucket, block, ());
		// A partly claimed tree, so the sweep pays for removing a bitmap that is there.
		ClaimedLeaves::<T>::insert(block, BoundedVec::truncate_from(alloc::vec![0b01u8]));
	}
	NextExpiryBucket::<T>::put(bucket);

	bucket
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
