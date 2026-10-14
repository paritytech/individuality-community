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

//! # Proof batch pallet
//!
//! A pallet to batch proof and validate them in on batch.
//!
//! [`Pallet`] implements [`DeferMembershipProofs`]

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
mod marginal_weight;
#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
pub mod weights;

use alloc::vec::Vec;
use codec::{Decode, Encode, FullCodec};
use frame_support::{
	pallet_prelude::*, weights::Weight, CloneNoBound, DebugNoBound, PartialEqNoBound,
};
use indiv_support::traits::{Alias, DeferMembershipProofs, RingMembershipProof};
use marginal_weight::marginal_weight;
use sp_runtime::traits::{BlakeTwo256, Hash};
use verifiable::{BatchProofItem, BatchProofItemFor, DecodeUnchecked, Error, GenerateVerifiable};

pub use pallet::*;
pub use weights::WeightInfo;

const LOG_TARGET: &str = "runtime::proof-batch";

/// The most ring hashes [`BatchRingHashes`] holds.
///
/// It bounds the scan of a push. A proof for another ring pays the ring setup at each push.
pub const MAX_RINGS: u32 = 512;

/// The number of [`WeightInfo::verify_batch_1`] to [`WeightInfo::verify_batch_2048`] samples.
const SAMPLE_COUNT: usize = 12;

/// The ring count of the ring setup benchmark [`WeightInfo::verify_batch_2048_512_rings`].
const RING_SETUP_RINGS: u64 = 512;

pub type ConfigOf<T> = <<T as Config>::Crypto as GenerateVerifiable>::Config;
pub type MembersOf<T> = <<T as Config>::Crypto as GenerateVerifiable>::Members;
pub type ProofOf<T> = <<T as Config>::Crypto as GenerateVerifiable>::Proof;

/// The hash of a ring: its configuration and members, see [`ring_hash`].
type RingHash = [u8; 32];

/// The hash that identifies a ring.
fn ring_hash<T: Config>(config: &ConfigOf<T>, members: &MembersOf<T>) -> RingHash {
	BlakeTwo256::hash_of(&(config, members)).into()
}

/// Ring members that decode with [`DecodeUnchecked::decode_unchecked`].
///
/// The batch stores only roots that their collection validated, so decoding skips the validation.
#[derive(Encode, TypeInfo, Clone, PartialEq, Debug)]
pub struct UncheckedMembers<M>(pub M);

impl<M: DecodeUnchecked> Decode for UncheckedMembers<M> {
	fn decode<I: codec::Input>(input: &mut I) -> Result<Self, codec::Error> {
		Ok(Self(M::decode_unchecked(input)?))
	}
}

/// A deferred proof.
#[derive(Encode, Decode, TypeInfo, CloneNoBound, PartialEqNoBound, DebugNoBound)]
#[scale_info(skip_type_params(T))]
pub struct BatchProof<T: Config> {
	/// The `ring_hash` of the ring. `on_finalize` sorts the batch by it.
	pub ring_hash: RingHash,
	/// The configuration of the ring the proof is checked against.
	pub ring_config: ConfigOf<T>,
	/// The root of the ring the proof is checked against.
	pub ring_members: UncheckedMembers<MembersOf<T>>,
	/// The single-context proof.
	pub proof: ProofOf<T>,
	/// The context of the proof.
	pub context: Vec<u8>,
	/// The message of the proof.
	pub message: Vec<u8>,
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_system::pallet_prelude::BlockNumberFor;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// The proof scheme.
		type Crypto: GenerateVerifiable<
			Config: FullCodec + TypeInfo + core::fmt::Debug + PartialEq,
			Members: DecodeUnchecked,
			Proof: Parameter,
		>;

		/// Weight information for this pallet.
		type WeightInfo: WeightInfo;

		/// Builds the rings and proofs of the benchmarks.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: BenchmarkHelper<Self::Crypto>;
	}

	#[pallet::storage]
	// Whitelisted: only the first access in a block reads the trie, see `first_access`.
	#[pallet::whitelist_storage]
	pub type TmpPoolValidation<T: Config> = StorageValue<_, bool, ValueQuery>;

	/// The `ring_hash` of the first [`MAX_RINGS`] distinct rings in the batch.
	///
	/// A push for a ring in it pays no ring setup.
	#[pallet::storage]
	// Whitelisted: only the first access in a block reads the trie, see `first_access`.
	#[pallet::whitelist_storage]
	#[pallet::unbounded]
	pub type BatchRingHashes<T: Config> = StorageValue<_, Vec<RingHash>, ValueQuery>;

	/// The proofs in the batch, in the order they were pushed.
	#[pallet::storage]
	// Whitelisted: only the first access in a block reads the trie, see `first_access`.
	#[pallet::whitelist_storage]
	#[pallet::unbounded]
	pub type BatchProofs<T: Config> = StorageValue<_, Vec<BatchProof<T>>, ValueQuery>;

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// Register the first access in the block to the whitelisted storage, see
		/// [`WeightInfo::first_access`].
		fn on_initialize(_n: BlockNumberFor<T>) -> Weight {
			frame_system::Pallet::<T>::register_extra_weight_unchecked(
				T::WeightInfo::first_access(),
				DispatchClass::Mandatory,
			);
			Weight::zero()
		}

		fn on_finalize(_n: BlockNumberFor<T>) {
			Self::verify_batch();
		}
	}
}

impl<T: Config> Pallet<T> {
	/// Note that the runtime validates a transaction for the transaction pool.
	///
	/// Temporary: `frame_system::Pallet::is_pool_transaction_validation()` replaces it once
	/// polkadot-sdk#12651 is merged and polkadot-sdk is bumped.
	pub fn tmp_note_pool_transaction_validation() {
		TmpPoolValidation::<T>::put(true);
	}

	/// The aliases of `items` and the weight to register, see
	/// [`DeferMembershipProofs::verify_or_defer`].
	fn check(
		ring_config: ConfigOf<T>,
		ring_members: MembersOf<T>,
		items: Vec<RingMembershipProof<ProofOf<T>>>,
	) -> Result<(Vec<Alias>, Weight), Error> {
		let count = items.len() as u32;
		if count == 0 {
			return Ok((Vec::new(), Weight::zero()));
		}
		if TmpPoolValidation::<T>::get() {
			return Self::verify_now(ring_config, ring_members, items);
		}

		let aliases = items
			.iter()
			.map(|item| match T::Crypto::extract_unverified_aliases(&item.proof)?.as_slice() {
				[alias] => Ok(*alias),
				_ => Err(Error::ContextCountMismatch),
			})
			.collect::<Result<Vec<_>, _>>()?;

		let hash = ring_hash::<T>(&ring_config, &ring_members);
		let hashes = BatchRingHashes::<T>::get();
		let tracked = hashes.len() as u32;
		let known = hashes.contains(&hash);
		if !known && tracked < MAX_RINGS {
			BatchRingHashes::<T>::append(hash);
		}

		let proofs_before = BatchProofs::<T>::decode_len().unwrap_or(0) as u32;
		// The first proof's ring setup is part of `verify_batch_1`.
		let setup_weight =
			if known || proofs_before == 0 { Weight::zero() } else { Self::ring_setup_weight() };
		let samples = Self::verify_batch_samples();
		let mut weight = setup_weight;
		for k in 1..=count {
			weight.saturating_accrue(marginal_weight(proofs_before.saturating_add(k), samples));
		}
		for item in items {
			BatchProofs::<T>::append(BatchProof::<T> {
				ring_hash: hash,
				ring_config,
				ring_members: UncheckedMembers(ring_members.clone()),
				proof: item.proof,
				context: item.context,
				message: item.message,
			});
		}

		Ok((aliases, weight))
	}

	/// Verify `items` against the ring with `batch_validate`.
	///
	/// Its weight is the sum of the marginal weights of `items` in an empty batch, which is at
	/// least the batch cost.
	fn verify_now(
		ring_config: ConfigOf<T>,
		ring_members: MembersOf<T>,
		items: Vec<RingMembershipProof<ProofOf<T>>>,
	) -> Result<(Vec<Alias>, Weight), Error> {
		let count = items.len() as u32;
		let proofs = items
			.into_iter()
			.map(|item| BatchProofItem {
				proof: item.proof,
				config: ring_config,
				members: ring_members.clone(),
				context: item.context,
				message: item.message,
			})
			.collect::<Vec<_>>();
		let aliases = T::Crypto::batch_validate(&proofs)?;
		let samples = Self::verify_batch_samples();
		let weight = (1..=count)
			.fold(Weight::zero(), |weight, k| weight.saturating_add(marginal_weight(k, samples)));
		Ok((aliases, weight))
	}

	/// Verify the batch and clear it. Panic if the batch is invalid.
	fn verify_batch() {
		let proofs = BatchProofs::<T>::take();
		BatchRingHashes::<T>::kill();
		if proofs.is_empty() {
			return;
		}

		let (items, ring_hashes) = Self::batch_items(proofs);
		if T::Crypto::batch_validate(&items).is_ok() {
			return;
		}
		for (index, result) in T::Crypto::batch_validate_per_item(&items).iter().enumerate() {
			if let Err(error) = result {
				log::error!(
					target: LOG_TARGET,
					"proof {index} of ring hash {:?} failed batch verification: {error:?}",
					ring_hashes[index],
				);
			}
		}
		panic!("proof batch: the block contains an invalid ring VRF proof");
	}

	/// The input of the batch check, sorted by ring hash then push order, and the ring hash of
	/// each item.
	///
	/// The verifier sets up its ring verifier at every ring change, so sorting saves a setup per
	/// change. It sorts indices, which are cheaper to move than proofs.
	pub(crate) fn batch_items(
		proofs: Vec<BatchProof<T>>,
	) -> (Vec<BatchProofItemFor<T::Crypto>>, Vec<RingHash>) {
		let mut order = proofs
			.iter()
			.enumerate()
			.map(|(index, proof)| (proof.ring_hash, index))
			.collect::<Vec<_>>();
		order.sort_unstable();
		let mut proofs = proofs.into_iter().map(Some).collect::<Vec<_>>();
		let mut ring_hashes = Vec::with_capacity(order.len());
		let mut items = Vec::with_capacity(order.len());
		for (ring_hash, index) in order {
			let Some(proof) = proofs[index].take() else {
				panic!("proof batch: the sort order repeats the proof {index}");
			};
			ring_hashes.push(ring_hash);
			items.push(BatchProofItem {
				proof: proof.proof,
				config: proof.ring_config,
				members: proof.ring_members.0,
				context: proof.context,
				message: proof.message,
			});
		}
		(items, ring_hashes)
	}

	fn verify_batch_samples() -> [Weight; SAMPLE_COUNT] {
		[
			T::WeightInfo::verify_batch_1(),
			T::WeightInfo::verify_batch_2(),
			T::WeightInfo::verify_batch_4(),
			T::WeightInfo::verify_batch_8(),
			T::WeightInfo::verify_batch_16(),
			T::WeightInfo::verify_batch_32(),
			T::WeightInfo::verify_batch_64(),
			T::WeightInfo::verify_batch_128(),
			T::WeightInfo::verify_batch_256(),
			T::WeightInfo::verify_batch_512(),
			T::WeightInfo::verify_batch_1024(),
			T::WeightInfo::verify_batch_2048(),
		]
	}

	/// The cost of one more ring in the batch check, rounded up.
	///
	/// The cost is linear in the ring count, and the cost of a ring grows a little with the proof
	/// count, so the benchmark uses the largest batch.
	fn ring_setup_weight() -> Weight {
		let extra = T::WeightInfo::verify_batch_2048_512_rings()
			.saturating_sub(T::WeightInfo::verify_batch_2048());
		let rings = RING_SETUP_RINGS - 1;
		Weight::from_parts(extra.ref_time().div_ceil(rings), extra.proof_size().div_ceil(rings))
	}
}

impl<T: Config> DeferMembershipProofs<T::Crypto> for Pallet<T> {
	fn verify_or_defer_max_weight(count: u32) -> Weight {
		let samples = Self::verify_batch_samples();
		// Every step in a segment is equal, so the first item of each segment covers all steps.
		let max_step = (0..SAMPLE_COUNT as u32).fold(samples[0], |max, i| {
			max.max(marginal_weight(1u32.saturating_add(1 << i), samples))
		});
		max_step
			.saturating_mul(u64::from(count))
			.saturating_add(Self::ring_setup_weight())
	}

	/// It registers the weight that the caller's benchmark of a push into an empty batch does not
	/// measure, as extra block weight of the `Normal` class, at most
	/// [`DeferMembershipProofs::verify_or_defer_max_weight`] of the item count:
	///
	/// - the share of a deferred proof in the batch check, which runs in `on_finalize`;
	/// - the ring setup of a ring that is not in [`BatchRingHashes`] of a non-empty batch;
	/// - a verification at once, in pool validation.
	///
	/// A weight reclaim of the transaction cannot undo registered weight, and the caller's
	/// `CheckWeight` still sees it, because every `validate` runs before any `prepare`.
	fn verify_or_defer(
		ring_config: ConfigOf<T>,
		ring_members: MembersOf<T>,
		items: Vec<RingMembershipProof<ProofOf<T>>>,
	) -> Result<Vec<Alias>, Error> {
		let (aliases, weight) = Self::check(ring_config, ring_members, items)?;
		frame_system::Pallet::<T>::register_extra_weight_unchecked(weight, DispatchClass::Normal);
		Ok(aliases)
	}
}

/// A ring with a valid single-context proof against it, for the benchmarks.
#[cfg(feature = "runtime-benchmarks")]
pub struct BenchmarkRing<Crypto: GenerateVerifiable> {
	/// The ring configuration.
	pub config: Crypto::Config,
	/// The ring root.
	pub members: Crypto::Members,
	/// A valid proof against the ring.
	pub proof: RingMembershipProof<Crypto::Proof>,
}

/// Builds the rings and proofs of the benchmarks.
#[cfg(feature = "runtime-benchmarks")]
pub trait BenchmarkHelper<Crypto: GenerateVerifiable> {
	/// A ring with a valid proof. Each `seed` gives a distinct ring root.
	///
	/// Use the largest ring configuration of the runtime, because the verification cost grows
	/// with it.
	fn ring_with_proof(seed: u32) -> BenchmarkRing<Crypto>;
}

/// Builds Bandersnatch rings of one member at the domain size `D`.
#[cfg(feature = "runtime-benchmarks")]
pub struct BandersnatchBenchmarkHelper<D>(core::marker::PhantomData<D>);

#[cfg(feature = "runtime-benchmarks")]
impl<D: Get<verifiable::ring::RingDomainSize>>
	BenchmarkHelper<indiv_support::crypto::BandersnatchVrfVerifiable>
	for BandersnatchBenchmarkHelper<D>
{
	fn ring_with_proof(
		seed: u32,
	) -> BenchmarkRing<indiv_support::crypto::BandersnatchVrfVerifiable> {
		use indiv_support::crypto::{BandersnatchSuite, BandersnatchVrfVerifiable as Crypto};
		use verifiable::ring::{
			ark_vrf::ring::SrsLookup, ring_verifier_builder_params, StaticChunk,
		};

		let config = D::get();
		let mut entropy = [0u8; 32];
		entropy[..4].copy_from_slice(&seed.to_le_bytes());
		let secret = Crypto::new_secret(entropy);
		let member = Crypto::member_from_secret(&secret);

		let builder_params = ring_verifier_builder_params::<BandersnatchSuite>(config);
		let lookup = |range: core::ops::Range<usize>| {
			(&builder_params)
				.lookup(range)
				.map(|points: Vec<_>| points.into_iter().map(StaticChunk).collect())
				.ok_or(())
		};
		let mut intermediate = Crypto::start_members(config);
		Crypto::push_members(&mut intermediate, core::iter::once(member), lookup)
			.expect("one member fits every ring");
		let members = Crypto::finish_members(intermediate);

		let context = b"proof-batch-benchmark".to_vec();
		let message = seed.encode();
		let commitment = Crypto::open(config, &member, core::iter::once(member))
			.expect("the member is in the ring");
		let (proof, _) =
			Crypto::create(commitment, &secret, &context, &message).expect("the proof is valid");
		BenchmarkRing { config, members, proof: RingMembershipProof { proof, context, message } }
	}
}
