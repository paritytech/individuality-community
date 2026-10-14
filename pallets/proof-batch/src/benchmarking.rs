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

//! Proof batch pallet benchmarking.
//!
//! The batch cost is concave in the proof count, so it is sampled at the fixed counts 1, 2, 4,
//! ..., 2048 and each proof is charged its [`marginal_weight`]. A sample repeats one proof: the
//! batch verifier combines items with random coefficients and does not deduplicate them.

use super::*;
use alloc::vec;
use frame_benchmarking::v2::*;
use frame_support::traits::Hooks;
use sp_runtime::traits::Zero;

/// The deferred proof of the benchmark ring of `seed`, with the ring hash.
fn batch_proof<T: Config>(seed: u32) -> (BatchProof<T>, RingHash) {
	let BenchmarkRing { config, members, proof } = T::BenchmarkHelper::ring_with_proof(seed);
	let hash = ring_hash::<T>(&config, &members);
	let proof = BatchProof::<T> {
		ring_hash: hash,
		ring_config: config,
		ring_members: UncheckedMembers(members),
		proof: proof.proof,
		context: proof.context,
		message: proof.message,
	};
	(proof, hash)
}

/// Fill the batch with `count` copies of one proof against one ring.
fn fill_one_ring<T: Config>(count: u32) {
	let (proof, hash) = batch_proof::<T>(0);
	BatchRingHashes::<T>::put(vec![hash]);
	BatchProofs::<T>::put(vec![proof; count as usize]);
}

fn verify_batch<T: Config>(count: u32) {
	fill_one_ring::<T>(count);
	assert_eq!(BatchProofs::<T>::decode_len(), Some(count as usize));
}

fn finalize<T: Config>() {
	Pallet::<T>::on_finalize(Zero::zero());
}

fn assert_batch_is_empty<T: Config>() {
	assert!(!BatchProofs::<T>::exists());
	assert!(!BatchRingHashes::<T>::exists());
}

#[benchmarks]
mod benches {
	use super::*;

	#[benchmark]
	fn verify_batch_1() {
		verify_batch::<T>(1);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_2() {
		verify_batch::<T>(2);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_4() {
		verify_batch::<T>(4);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_8() {
		verify_batch::<T>(8);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_16() {
		verify_batch::<T>(16);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_32() {
		verify_batch::<T>(32);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_64() {
		verify_batch::<T>(64);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_128() {
		verify_batch::<T>(128);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_256() {
		verify_batch::<T>(256);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_512() {
		verify_batch::<T>(512);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_1024() {
		verify_batch::<T>(1_024);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	#[benchmark]
	fn verify_batch_2048() {
		verify_batch::<T>(2_048);
		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	/// 2048 proofs over 512 rings, 4 proofs per ring. The difference to [`verify_batch_2048`] is
	/// the setup of 511 extra rings.
	///
	/// The batch check sets up a ring verifier at each ring change, so two rings in alternation
	/// cost as much as 512 distinct rings, and the benchmark needs only two proofs. The ring hash
	/// only orders the batch: each group of 4 proofs gets its own hash, in group order, and the
	/// groups alternate between the two rings.
	#[benchmark]
	fn verify_batch_2048_512_rings() {
		let rings = [0, 1].map(|seed| batch_proof::<T>(seed).0);
		let per_ring = 2_048 / RING_SETUP_RINGS as u32;
		let mut hashes = vec![];
		let mut proofs = vec![];
		for group in 0..RING_SETUP_RINGS as u32 {
			let mut hash = [0; 32];
			hash[..4].copy_from_slice(&group.to_be_bytes());
			hashes.push(hash);
			let proof = &rings[(group % 2) as usize];
			proofs.extend(
				(0..per_ring).map(|_| BatchProof::<T> { ring_hash: hash, ..proof.clone() }),
			);
		}
		let (items, _) = Pallet::<T>::batch_items(proofs.clone());
		let ring_changes =
			items.windows(2).filter(|pair| pair[0].members != pair[1].members).count();
		assert_eq!(ring_changes as u64, RING_SETUP_RINGS - 1);
		BatchRingHashes::<T>::put(hashes);
		BatchProofs::<T>::put(proofs);

		#[block]
		{
			finalize::<T>();
		}
		assert_batch_is_empty::<T>();
	}

	/// The first access in a block to the whitelisted storage: the read of the pool validation
	/// flag and an empty `on_finalize`. The keys leave the whitelist, so the reads, the writes
	/// and the absence proofs count.
	#[benchmark]
	fn first_access() {
		for key in [
			TmpPoolValidation::<T>::hashed_key(),
			BatchProofs::<T>::hashed_key(),
			BatchRingHashes::<T>::hashed_key(),
		] {
			frame_benchmarking::benchmarking::remove_from_whitelist(key.to_vec());
		}

		#[block]
		{
			TmpPoolValidation::<T>::get();
			finalize::<T>();
		}

		assert_batch_is_empty::<T>();
	}

	impl_benchmark_test_suite!(Pallet, crate::mock::new_test_ext(), crate::mock::Test);
}
