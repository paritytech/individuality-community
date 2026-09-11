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
use indiv_support::credit_trees::MAX_PRIVATE_CLAIM_SLOTS;
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

/// A distinct ring key, standing in for the one-time key a claimant's wallet makes.
#[cfg(feature = "runtime-benchmarks")]
fn private_ring_key<T: Config>(seed: u32) -> crate::PrivateRingKey<T> {
	use verifiable::GenerateVerifiable;

	let secret = T::RingVrf::new_secret(sp_io::hashing::blake2_256(&seed.encode()));
	T::RingVrf::member_from_secret(&secret)
}

/// Open the private claim path of `game_index`, as a private game's first award does.
#[cfg(feature = "runtime-benchmarks")]
fn open_private_game<T: Config>(game_index: GameIdx, slots: u8) {
	PrivateGames::<T>::insert(
		game_index,
		PrivateGameInfo {
			slots,
			registration_starts: 0,
			registration_ends: u32::MAX,
			key_count: 0,
			eligible_players: 0,
			phase: PrivateGamePhase::Building { included: 0, failures: 0 },
		},
	);
}

/// Register `count` keys for `game_index`, as that many claimants would.
#[cfg(feature = "runtime-benchmarks")]
fn fill_private_ring<T: Config>(game_index: GameIdx, count: u32) {
	let keys = (0..count).map(private_ring_key::<T>).collect::<Vec<_>>();
	PrivateRingKeys::<T>::insert(
		game_index,
		BoundedVec::try_from(keys).expect("count is bounded by MaxPrivateRingKeys"),
	);
	PrivateGames::<T>::mutate(game_index, |game| {
		if let Some(game) = game {
			game.key_count = count;
		}
	});
}

/// Close `game_index`'s registration, which is what lets its rings be built.
#[cfg(feature = "runtime-benchmarks")]
fn close_private_registration<T: Config>(game_index: GameIdx) {
	PrivateGames::<T>::mutate(game_index, |game| {
		if let Some(game) = game {
			game.registration_ends = 0;
		}
	});
}

/// Push `count` of `game_index`'s keys, so a measured step starts from a part-built ring.
#[cfg(feature = "runtime-benchmarks")]
fn push_private_keys<T: Config>(game_index: GameIdx, count: u32) {
	let mut pushed = 0;
	while pushed < count {
		let step = (count - pushed).min(T::PrivateKeysPerBuild::get());
		pallet::Pallet::<T>::do_build_private_ring(game_index, step)
			.expect("registration is closed and the keys are waiting");
		pushed += step;
	}
}

/// Put `game_index` in its cleanup phase with `entries` claimants registered.
#[cfg(feature = "runtime-benchmarks")]
fn seed_private_clean_up<T: Config>(game_index: GameIdx, entries: u32) {
	open_private_game::<T>(game_index, 1);
	fill_private_ring::<T>(game_index, T::MinPrivateRingKeys::get());
	// The step that ended the build removed the keys, so a game in its cleanup holds none.
	PrivateRingKeys::<T>::remove(game_index);
	for index in 0..entries {
		let claimant = AccountOrPerson::Person(sp_io::hashing::blake2_256(&index.encode()));
		PrivateRegistrations::<T>::insert(game_index, &claimant, ());
		PrivateCreditBalances::<T>::insert(game_index, &claimant, 1);
	}
	PrivateGames::<T>::mutate(game_index, |game| {
		if let Some(game) = game {
			game.phase = PrivateGamePhase::CleaningUp;
		}
	});
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
			private_slots: 0,
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

	/// Worst case for `n` removals: `n` roots are due, each under a timestamp of its own, so the
	/// call pays for every removal and for one map key per root. A root that is not due follows
	/// them, which is the entry the sweep reads to stop.
	#[benchmark]
	fn sweep_expired_roots(
		n: Linear<0, { T::MaxRootsPerSweep::get() }>,
	) -> Result<(), BenchmarkError> {
		fill_due_expiries::<T>(n);
		let origin = RawOrigin::Authorized;

		#[extrinsic_call]
		_(origin, FIRST_ROOT_TIMESTAMP, BlockNumberFor::<T>::from(0u32));

		// Only the root that is not due is left, which is the one filed last.
		assert_eq!(NftClaimCreditRoots::<T>::iter().count(), 1);
		assert_eq!(RootExpiries::<T>::iter().count(), 1);
		assert_eq!(oldest_expiry::<RootExpiries<T>, BlockNumberFor<T>>(), Some(root_timestamp(n)));

		Ok(())
	}

	// Registering one key for one claimant. The key list is read and rewritten at its
	// `MaxEncodedLen` whatever it holds, so a full list costs what an empty one does and the
	// benchmark needs no component.
	#[benchmark]
	fn register_private_claim_key() -> Result<(), BenchmarkError> {
		let game_index = 1;
		let caller: T::AccountId = whitelisted_caller();
		let claimant = AccountOrPerson::Account(caller.clone());
		open_private_game::<T>(game_index, MAX_PRIVATE_CLAIM_SLOTS);
		PrivateCreditBalances::<T>::insert(
			game_index,
			&claimant,
			T::PrivateClaimEntryCredits::get(),
		);
		fill_private_ring::<T>(game_index, T::MaxPrivateRingKeys::get() - 1);

		#[extrinsic_call]
		_(RawOrigin::Signed(caller), game_index, private_ring_key::<T>(u32::MAX));

		assert!(PrivateRingKeys::<T>::get(game_index).len() as u32 == T::MaxPrivateRingKeys::get());

		Ok(())
	}

	// One push step. `n` is the keys it pushes, swept over the whole per-call range. The call is
	// KZG commitment work, which scales with the keys pushed and nothing else.
	//
	// The keys before the measured step are pushed first, so `n` is what the measured run pushes
	// and the step is a middle one that finishes no root.
	#[benchmark]
	fn build_private_ring(
		n: Linear<1, { T::PrivateKeysPerBuild::get() }>,
	) -> Result<(), BenchmarkError> {
		let game_index = 1;
		let keys = T::MinPrivateRingKeys::get().max(n);
		open_private_game::<T>(game_index, 1);
		fill_private_ring::<T>(game_index, keys);
		close_private_registration::<T>(game_index);
		push_private_keys::<T>(game_index, keys - n);

		#[extrinsic_call]
		_(RawOrigin::Authorized, game_index, n, BlockNumberFor::<T>::zero());

		assert!(
			matches!(
				PrivateGames::<T>::get(game_index).expect("the game is private").phase,
				PrivateGamePhase::Building { included, .. } if included == keys
			),
			"the step pushed the keys it was given"
		);

		Ok(())
	}

	// The step that closes the ring, which is the same call with no keys to push. Finishing the
	// root, storing it, dropping the keys it commits to and queueing it for delivery is the worst
	// of its two shapes; abandoning a ring writes the same entries with no root to compute.
	#[benchmark]
	fn finish_private_ring() -> Result<(), BenchmarkError> {
		let game_index = 1;
		let keys = T::MinPrivateRingKeys::get();
		open_private_game::<T>(game_index, 1);
		fill_private_ring::<T>(game_index, keys);
		close_private_registration::<T>(game_index);
		push_private_keys::<T>(game_index, keys);

		#[extrinsic_call]
		build_private_ring(RawOrigin::Authorized, game_index, 0, BlockNumberFor::<T>::zero());

		assert!(PrivateOutcomes::<T>::get(game_index).is_some(), "the root is final");
		assert!(PrivateRingIntermediates::<T>::get(game_index).is_none());
		assert!(PrivateRingKeys::<T>::decode_len(game_index).is_none(), "the keys go with it");

		Ok(())
	}

	/// Authorizing a sweep reads the oldest entry and the clock. The map holds one sweep's worth
	/// of entries, each under a key of its own, which is the state a sweep is submitted against.
	#[benchmark]
	fn authorize_sweep_expired_roots() -> Result<(), BenchmarkError> {
		fill_due_expiries::<T>(T::MaxRootsPerSweep::get());

		#[block]
		{
			pallet::Pallet::<T>::authorize_sweep_expired_roots(
				TransactionSource::Local,
				&FIRST_ROOT_TIMESTAMP,
			)
			.expect("must authorize");
		}

		Ok(())
	}

	// Authorizing a build step reads the game's record and its keys, which say how many keys the
	// step still owes, so it is measured against a full registration.
	#[benchmark]
	fn authorize_build_private_ring() -> Result<(), BenchmarkError> {
		let game_index = 1;
		open_private_game::<T>(game_index, 1);
		fill_private_ring::<T>(game_index, T::MaxPrivateRingKeys::get());
		close_private_registration::<T>(game_index);
		let to_include =
			pallet::Pallet::<T>::private_ring_build_step(game_index).expect("a step is due");

		#[block]
		{
			pallet::Pallet::<T>::authorize_build_private_ring(
				TransactionSource::Local,
				&game_index,
				&to_include,
			)
			.expect("must authorize");
		}

		Ok(())
	}

	// Delivering one ring, which assembles the XCM around a full ring root and drains the queue.
	#[benchmark]
	fn send_private_ring() -> Result<(), BenchmarkError> {
		let game_index = 1;
		// The channel is sized for the ring message, which is far larger than a credit tree's.
		<T as Config>::BenchmarkHelper::open_nft_claims_channel(
			pallet::Pallet::<T>::private_ring_channel_size(),
		);
		open_private_game::<T>(game_index, 1);
		fill_private_ring::<T>(game_index, T::MinPrivateRingKeys::get());
		close_private_registration::<T>(game_index);
		while let Some(to_include) = pallet::Pallet::<T>::private_ring_build_step(game_index) {
			pallet::Pallet::<T>::do_build_private_ring(game_index, to_include)
				.expect("the ring builds");
		}

		#[extrinsic_call]
		_(RawOrigin::Authorized, game_index, BlockNumberFor::<T>::zero());

		assert!(PrivateOutcomes::<T>::get(game_index).is_none(), "a sent ring leaves the queue");

		Ok(())
	}

	// Authorizing a delivery decodes the queue and, when it is empty, reads the outcome of a game
	// a full queue turned away. That branch touches both entries, so it is the one measured.
	#[benchmark]
	fn authorize_send_private_ring() -> Result<(), BenchmarkError> {
		let game_index = 1;
		open_private_game::<T>(game_index, 1);
		fill_private_ring::<T>(game_index, T::MinPrivateRingKeys::get());
		close_private_registration::<T>(game_index);
		while let Some(to_include) = pallet::Pallet::<T>::private_ring_build_step(game_index) {
			pallet::Pallet::<T>::do_build_private_ring(game_index, to_include)
				.expect("the ring builds");
		}
		// The ring reached its outcome with the queue full, so it waits outside the queue.
		PrivateRingDeliveryQueue::<T>::kill();

		#[block]
		{
			pallet::Pallet::<T>::authorize_send_private_ring(TransactionSource::Local, &game_index)
				.expect("must authorize");
		}

		Ok(())
	}

	// One cleanup step, over `n` of the registrations a private game leaves behind. The ring keys
	// went with the build step that closed the ring, so no step here touches them.
	#[benchmark]
	fn clean_up_private_game(
		n: Linear<0, { crate::PRIVATE_CLEAN_UP_ITEMS }>,
	) -> Result<(), BenchmarkError> {
		let game_index = 1;
		seed_private_clean_up::<T>(game_index, n);

		#[extrinsic_call]
		_(RawOrigin::Authorized, game_index, BlockNumberFor::<T>::zero());

		assert_eq!(PrivateRegistrations::<T>::iter_prefix(game_index).count(), 0);

		Ok(())
	}

	// Authorizing a cleanup step reads the game's record and its undelivered outcome, so it is a
	// fixed cost.
	#[benchmark]
	fn authorize_clean_up_private_game() -> Result<(), BenchmarkError> {
		let game_index = 1;
		seed_private_clean_up::<T>(game_index, 1);

		#[block]
		{
			pallet::Pallet::<T>::authorize_clean_up_private_game(
				TransactionSource::Local,
				&game_index,
			)
			.expect("must authorize");
		}

		Ok(())
	}

	// No `impl_benchmark_test_suite!`: a mock for this pallet is a mock of the whole game it sits
	// on, which the game crate already has, so its tests are the ones that run these paths. The
	// benchmarks themselves are exercised by `frame-omni-bencher` against the runtime.
}

/// The `timestamp` the first benchmarked root commits to. The value is arbitrary, because
/// `fill_due_expiries` sets the clock from it.
const FIRST_ROOT_TIMESTAMP: u32 = 1_000_000;

/// The `timestamp` the root of `index` commits to. One timestamp per root is the worst case: every
/// entry then holds a key of its own, where roots sharing a timestamp would share the map's first
/// key.
fn root_timestamp(index: u32) -> u32 {
	FIRST_ROOT_TIMESTAMP.saturating_add(index)
}

/// Records `n` roots, one per block, each filed under a timestamp of its own, and returns their
/// blocks.
fn record_roots<T: Config>(n: u32) -> Vec<BlockNumberFor<T>> {
	let blocks = (1..=n).map(BlockNumberFor::<T>::from).collect::<Vec<_>>();

	for (index, block) in blocks.iter().enumerate() {
		let timestamp = root_timestamp(index as u32);
		NftClaimCreditRoots::<T>::insert(
			block,
			NftClaimCreditTree {
				game_index: 7,
				root: CreditProofNode([index as u8; 32]),
				leaf_count: 1,
				timestamp,
				private_slots: 0,
			},
		);
		pallet::Pallet::<T>::note_root_expiry(*block, timestamp);
	}

	blocks
}

/// Records `n` roots that are due, plus one that is not, and sets the clock to the deadline of the
/// last due one. The root filed after it is what stops the sweep.
fn fill_due_expiries<T: Config>(n: u32) {
	record_roots::<T>(n.saturating_add(1));

	let last_due = FIRST_ROOT_TIMESTAMP.saturating_add(n).saturating_sub(1);
	<T as Config>::BenchmarkHelper::set_unix_time(expiry_deadline(
		last_due,
		pallet::Pallet::<T>::root_ttl(),
	));
}

/// Records `n` credit trees, one per block, and queues every one of them for delivery.
fn queue_credit_trees<T: Config>(n: u32) -> Vec<BlockNumberFor<T>> {
	let blocks = (1..=n).map(BlockNumberFor::<T>::from).collect::<Vec<_>>();

	for (index, block) in blocks.iter().enumerate() {
		NftClaimCreditRoots::<T>::insert(
			block,
			NftClaimCreditTree {
				private_slots: 0,
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
