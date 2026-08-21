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

//! Benchmarks for the NFT claim credits.

use super::*;

use codec::Encode;
use frame_benchmarking::v2::{benchmarks, *};
use frame_system::{pallet_prelude::BlockNumberFor, RawOrigin};
use indiv_support::credit_trees::EXPIRY_BUCKET_SECONDS;
use sp_runtime::{traits::One, transaction_validity::TransactionSource};

/// What the benchmarks cannot set up themselves, because only the runtime knows how its XCM
/// channels and its clock are made.
pub trait BenchmarkHelper {
	/// Opens an HRMP channel to the NFT claims chain that carries `max_message_size` bytes per
	/// message, which is what decides how many credit trees one delivery takes.
	fn open_nft_claims_channel(max_message_size: u32);

	/// Moves [`indiv_pallet_game::Config::UnixTime`] to `secs` since the UNIX epoch.
	///
	/// A sweep's validity depends on the clock, so a benchmark of it sets the clock. Only the
	/// runtime knows which pallet holds it.
	fn set_unix_time(secs: u64);
}

#[benchmarks]
mod benches {
	use super::*;

	// The `on_initialize` path that records a block's root. `n` is the number of leaves the tree
	// is built over, swept over the whole range one block can award: the hashing, the awards'
	// contribution to the proof size, and the retained ring all scale with it.
	//
	// The ring is set up full, so the run includes dropping the oldest award block, which is the
	// worst case and the one every block pays for once the chain has been running.
	#[benchmark]
	fn build_credit_tree(
		n: Linear<1, { T::MaxCreditsPerBlock::get() }>,
	) -> Result<(), BenchmarkError> {
		let retained = T::MaxRetainedAwardBlocks::get();
		frame_system::Pallet::<T>::set_block_number((retained + 10).into());
		let block = frame_system::Pallet::<T>::block_number();

		let awards =
			|count: u32| -> BoundedVec<NftClaimCreditAward<T::AccountId>, T::MaxCreditsPerBlock> {
				(0..count)
					.map(|i| NftClaimCreditAward {
						claimant: AccountOrPerson::Person(sp_io::hashing::blake2_256(&i.encode())),
						credit: sp_io::hashing::blake2_256(&(i, b"credit").encode()),
					})
					.collect::<Vec<_>>()
					.try_into()
					.expect("count is bounded by MaxCreditsPerBlock")
			};
		NftClaimCreditAwards::<T>::insert(block, awards(n));

		// The block that drops out of the ring, holding a full set of awards to remove.
		let dropped: BlockNumberFor<T> = 1u32.into();
		NftClaimCreditAwards::<T>::insert(dropped, awards(T::MaxCreditsPerBlock::get()));
		NftClaimCreditAwardBlocks::<T>::put(BoundedVec::<
			BlockNumberFor<T>,
			T::MaxRetainedAwardBlocks,
		>::truncate_from(
			(1..=retained).map(Into::into).collect::<Vec<_>>()
		));

		PendingNftClaimCreditRootInfo::<T>::put(NftClaimCreditRootInfo {
			game_index: 7,
			timestamp: 1_234,
		});

		#[block]
		{
			pallet::Pallet::<T>::build_credit_tree(block + One::one());
		}

		let credit_root =
			NftClaimCreditRoots::<T>::get(block).expect("a root is recorded for the block");
		assert_eq!(credit_root.leaf_count, n);
		assert_eq!(NftClaimCreditAwards::<T>::decode_len(block).unwrap_or(0) as u32, n);
		assert!(!NftClaimCreditAwards::<T>::contains_key(dropped));

		Ok(())
	}

	// The same path over a block that awarded nothing: the common case, since only blocks that
	// awarded a credit record a root.
	#[benchmark]
	fn build_credit_tree_empty() -> Result<(), BenchmarkError> {
		let block = frame_system::Pallet::<T>::block_number();

		#[block]
		{
			pallet::Pallet::<T>::build_credit_tree(block + One::one());
		}

		assert!(!NftClaimCreditRoots::<T>::contains_key(block));

		Ok(())
	}

	// Delivering a message worth of credit trees: `n` is the number of trees the message carries,
	// which drives both the tree reads and the size of the XCM assembled from them.
	//
	// The queue is filled to `MaxQueuedCreditTrees` first and the channel sized to carry exactly
	// `n` trees, so the run also pays for rewriting the entries `n` leaves behind. That remainder
	// is what an outage's first recovery transaction faces, and a queue holding only the delivered
	// trees would leave it uncharged.
	//
	// The two costs pull in opposite directions over `n`: a larger message assembles more XCM but
	// leaves fewer entries to rewrite, so the fitted per-tree term is small and the base carries
	// the full-queue rewrite. Every delivery pays that base, which is the point.
	#[benchmark]
	fn send_credit_trees(
		n: Linear<1, { T::MaxCreditTreesPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		<T as Config>::BenchmarkHelper::open_nft_claims_channel(
			pallet::Pallet::<T>::credit_tree_channel_size(n),
		);
		let queued = T::MaxQueuedCreditTrees::get();
		queue_credit_trees::<T>(queued);

		#[extrinsic_call]
		_(RawOrigin::Authorized, 0, BlockNumberFor::<T>::zero());

		assert_eq!(
			CreditTreeDeliveryQueue::<T>::decode_len().unwrap_or(0) as u32,
			queued - n,
			"a delivered message drains the trees it carried and leaves the rest",
		);

		Ok(())
	}

	// The manual repair of a lost delivery. Every named block resolves to a tree, so all `n` of
	// them are packed into the message.
	#[benchmark]
	fn replay_credit_trees(
		n: Linear<1, { T::MaxCreditTreesPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		<T as Config>::BenchmarkHelper::open_nft_claims_channel(
			pallet::Pallet::<T>::credit_tree_channel_size(T::MaxCreditTreesPerMessage::get()),
		);
		let blocks = queue_credit_trees::<T>(n);
		let caller: T::AccountId = whitelisted_caller();

		#[extrinsic_call]
		_(
			RawOrigin::Signed(caller),
			BoundedVec::try_from(blocks).expect("n is bounded by MaxCreditTreesPerMessage"),
		);

		Ok(())
	}

	// Authorizing a delivery decodes the whole delivery queue, so it is measured against a queue
	// at `MaxQueuedCreditTrees`: the state an outage leaves behind, and the only one where the
	// decode is more than a few bytes.
	#[benchmark]
	fn authorize_send_credit_trees() -> Result<(), BenchmarkError> {
		queue_credit_trees::<T>(T::MaxQueuedCreditTrees::get());

		#[block]
		{
			pallet::Pallet::<T>::authorize_send_credit_trees(TransactionSource::Local, &0)
				.expect("must authorize");
		}

		Ok(())
	}

	/// Worst case: every block named holds a root, so the call reads each one and removes both its
	/// entries. A block whose root is already gone costs the read alone.
	#[benchmark]
	fn receive_tree_deletions(
		n: Linear<1, { T::MaxTreeDeletionsPerMessage::get() }>,
	) -> Result<(), BenchmarkError> {
		let blocks = record_roots::<T>(n);
		let origin = T::EnsureClaimsChainOrigin::try_successful_origin()
			.map_err(|_| BenchmarkError::Stop("failed to construct claims chain origin"))?;

		#[extrinsic_call]
		_(
			origin as T::RuntimeOrigin,
			BoundedVec::try_from(blocks).expect("n is bounded by MaxTreeDeletionsPerMessage"),
		);

		assert_eq!(NftClaimCreditRoots::<T>::iter().count(), 0);
		assert_eq!(RootExpiries::<T>::iter().count(), 0);

		Ok(())
	}

	/// Worst case for `n` removals: the bucket holds exactly `n` roots, so the call pays for every
	/// removal. Below the limit it also takes the branch that moves the watermark on.
	///
	/// A drain that reaches the limit cannot tell that it emptied the bucket. At `n` equal to
	/// [`Config::MaxRootsPerSweep`] that branch is unreachable, and the fit then charges the call
	/// for one write it does not make.
	#[benchmark]
	fn sweep_expired_roots(
		n: Linear<0, { T::MaxRootsPerSweep::get() }>,
	) -> Result<(), BenchmarkError> {
		let bucket = expiry_bucket(ROOT_TIMESTAMP);
		record_roots::<T>(n);
		let origin = RawOrigin::Authorized;

		#[extrinsic_call]
		_(origin, bucket, BlockNumberFor::<T>::from(0u32));

		assert_eq!(NftClaimCreditRoots::<T>::iter().count(), 0);
		assert_eq!(RootExpiries::<T>::iter_prefix(bucket).count(), 0);
		if n < T::MaxRootsPerSweep::get() {
			assert_eq!(NextRootExpiryBucket::<T>::get(), Some(bucket.saturating_add(1)));
		}

		Ok(())
	}

	/// Authorizing a sweep reads the watermark and the clock. Neither grows with any input, so the
	/// benchmark only needs a bucket whose TTL has run out.
	#[benchmark]
	fn authorize_sweep_expired_roots() -> Result<(), BenchmarkError> {
		let bucket = expiry_bucket(ROOT_TIMESTAMP);
		record_roots::<T>(1);
		<T as Config>::BenchmarkHelper::set_unix_time(bucket_deadline(
			bucket,
			pallet::Pallet::<T>::root_ttl(),
		));

		#[block]
		{
			pallet::Pallet::<T>::authorize_sweep_expired_roots(TransactionSource::Local, &bucket)
				.expect("must authorize");
		}

		Ok(())
	}

	// No `impl_benchmark_test_suite!`: a mock for this pallet is a mock of the whole game it sits
	// on, which the game crate already has, so its tests are the ones that run these paths. The
	// benchmarks themselves are exercised by `frame-omni-bencher` against the runtime.
}

/// The `timestamp` every benchmarked root commits to. It sits one whole bucket in, so the clock can
/// reach the bucket's deadline.
const ROOT_TIMESTAMP: u32 = EXPIRY_BUCKET_SECONDS;

/// Records `n` roots, one per block, each filed under [`ROOT_TIMESTAMP`]'s expiry bucket, and
/// returns their blocks.
fn record_roots<T: Config>(n: u32) -> Vec<BlockNumberFor<T>> {
	let blocks = (1..=n).map(BlockNumberFor::<T>::from).collect::<Vec<_>>();

	for (index, block) in blocks.iter().enumerate() {
		NftClaimCreditRoots::<T>::insert(
			block,
			NftClaimCreditTree {
				game_index: 7,
				root: CreditProofNode([index as u8; 32]),
				leaf_count: 1,
				timestamp: ROOT_TIMESTAMP,
			},
		);
		pallet::Pallet::<T>::note_root_expiry(*block, ROOT_TIMESTAMP);
	}

	blocks
}

/// Records `n` credit trees, one per block, and queues every one of them for delivery.
fn queue_credit_trees<T: Config>(n: u32) -> Vec<BlockNumberFor<T>> {
	let blocks = (1..=n).map(BlockNumberFor::<T>::from).collect::<Vec<_>>();

	for (index, block) in blocks.iter().enumerate() {
		NftClaimCreditRoots::<T>::insert(
			block,
			NftClaimCreditTree {
				game_index: 7,
				root: CreditProofNode([index as u8; 32]),
				leaf_count: 1,
				timestamp: 1_234,
			},
		);
		pallet::Pallet::<T>::queue_credit_tree_delivery(*block);
	}

	blocks
}
