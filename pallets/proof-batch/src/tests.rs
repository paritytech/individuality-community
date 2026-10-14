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

use crate::{mock::*, *};
use frame_support::{dispatch::DispatchClass, traits::Hooks};
use indiv_support::traits::Alias;

fn finalize() {
	<ProofBatch as Hooks<u64>>::on_finalize(1);
}

fn batch_is_empty() -> bool {
	!BatchProofs::<Test>::exists() && !BatchRingHashes::<Test>::exists()
}

/// The aliases of a check and the weight it registered.
struct Checked {
	aliases: Vec<Alias>,
	registered: Weight,
}

fn registered() -> Weight {
	*frame_system::Pallet::<Test>::block_weight().get(DispatchClass::Normal)
}

/// Call [`DeferMembershipProofs::verify_or_defer`] and read the weight it registered.
fn verify_or_defer(
	ring: MockRing,
	items: Vec<RingMembershipProof<ProofOf<Test>>>,
) -> Result<Checked, Error> {
	let (config, members) = ring;
	let before = registered();
	let aliases = ProofBatch::verify_or_defer(config, members, items)?;
	Ok(Checked { aliases, registered: registered().saturating_sub(before) })
}

fn seeded_ring_hash(seed: u8) -> RingHash {
	let (config, members) = ring(seed);
	ring_hash::<Test>(&config, &members)
}

fn proof_ring_hashes() -> Vec<RingHash> {
	BatchProofs::<Test>::get().iter().map(|proof| proof.ring_hash).collect()
}

fn defer_one(seed: u8, message: &[u8]) -> Checked {
	let (item, alias) = proof(seed, message);
	let verified = verify_or_defer(ring(seed), vec![item]).expect("aliases read");
	assert_eq!(verified.aliases, vec![alias]);
	verified
}

/// Append `count` ring hashes that no real ring has.
fn append_unused_ring_hashes(count: u32) {
	for i in 0..count {
		let mut hash = [0xff; 32];
		hash[..4].copy_from_slice(&i.to_le_bytes());
		BatchRingHashes::<Test>::append(hash);
	}
}

fn samples() -> [Weight; 12] {
	ProofBatch::verify_batch_samples()
}

#[test]
fn pool_validation_verifies_at_once() {
	new_test_ext().execute_with(|| {
		ProofBatch::tmp_note_pool_transaction_validation();
		let (item, alias) = proof(1, b"pool");

		let verified = verify_or_defer(ring(1), vec![item]).unwrap();

		assert_eq!(verified.aliases, vec![alias]);
		assert_eq!(verified.registered, samples()[0]);
		assert!(batch_is_empty());
	});
}

#[test]
fn pool_validation_rejects_an_invalid_proof() {
	new_test_ext().execute_with(|| {
		ProofBatch::tmp_note_pool_transaction_validation();

		assert_eq!(
			verify_or_defer(ring(1), vec![invalid_proof(1)]).map(|v| v.aliases),
			Err(Error::VerificationFailed)
		);
		assert!(batch_is_empty());
	});
}

#[test]
fn block_defers_an_invalid_proof() {
	new_test_ext().execute_with(|| {
		let alias =
			verifiable::mock::Mock::extract_unverified_aliases(&invalid_proof(1).proof).unwrap()[0];

		let verified = verify_or_defer(ring(1), vec![invalid_proof(1)]).unwrap();

		assert_eq!(verified.aliases, vec![alias]);
		assert_eq!(BatchProofs::<Test>::decode_len(), Some(1));
	});
}

#[test]
fn block_defers_and_on_finalize_verifies_the_batch() {
	new_test_ext().execute_with(|| {
		defer_one(1, b"a");
		defer_one(1, b"b");

		assert_eq!(BatchProofs::<Test>::decode_len(), Some(2));
		finalize();
		assert!(batch_is_empty());
	});
}

#[test]
#[should_panic(expected = "the block contains an invalid ring VRF proof")]
fn on_finalize_panics_on_an_invalid_proof() {
	new_test_ext().execute_with(|| {
		defer_one(1, b"a");
		verify_or_defer(ring(2), vec![invalid_proof(2)]).unwrap();
		defer_one(1, b"b");

		finalize();
	});
}

#[test]
fn on_initialize_registers_the_first_access() {
	new_test_ext().execute_with(|| {
		let mandatory =
			|| *frame_system::Pallet::<Test>::block_weight().get(DispatchClass::Mandatory);
		let before = mandatory();

		assert_eq!(<ProofBatch as Hooks<u64>>::on_initialize(1), Weight::zero());

		assert_eq!(mandatory().saturating_sub(before), <() as WeightInfo>::first_access());
	});
}

#[test]
fn on_finalize_with_an_empty_batch_does_nothing() {
	new_test_ext().execute_with(|| {
		finalize();
		assert!(batch_is_empty());
	});
}

#[test]
fn each_proof_stores_its_ring_and_each_ring_hash_is_tracked_once() {
	new_test_ext().execute_with(|| {
		defer_one(1, b"a");
		defer_one(2, b"b");
		defer_one(1, b"c");
		defer_one(3, b"d");
		defer_one(2, b"e");

		let [one, two, three] = [1, 2, 3].map(seeded_ring_hash);
		assert_eq!(BatchRingHashes::<Test>::get(), vec![one, two, three]);
		assert_eq!(proof_ring_hashes(), vec![one, two, one, three, two]);
		assert_eq!(
			BatchProofs::<Test>::get()
				.iter()
				.map(|proof| proof.ring_members.0.clone())
				.collect::<Vec<_>>(),
			[1, 2, 1, 3, 2].map(|seed| ring(seed).1)
		);

		finalize();
		assert!(batch_is_empty());
	});
}

#[test]
fn batch_items_are_sorted_by_ring_hash_then_push_order() {
	new_test_ext().execute_with(|| {
		defer_one(1, b"a");
		defer_one(2, b"b");
		defer_one(1, b"c");
		defer_one(2, b"d");

		let (items, hashes) = ProofBatch::batch_items(BatchProofs::<Test>::get());

		let [one, two] = [1, 2].map(seeded_ring_hash);
		let (first, second) = if one < two { (1, 2) } else { (2, 1) };
		let expected = |seed: u8| match seed {
			1 => [b"a", b"c"],
			_ => [b"b", b"d"],
		};
		assert_eq!(hashes, [first, first, second, second].map(seeded_ring_hash));
		assert_eq!(
			items.iter().map(|item| item.message.as_slice()).collect::<Vec<_>>(),
			[expected(first), expected(second)].concat()
		);
		assert_eq!(
			items.iter().map(|item| item.members.clone()).collect::<Vec<_>>(),
			[first, first, second, second].map(|seed| ring(seed).1)
		);
	});
}

#[test]
fn a_multi_item_push_keeps_the_input_order() {
	new_test_ext().execute_with(|| {
		let items = [proof(1, b"a"), proof(1, b"b"), proof(1, b"c")];
		let expected = items.iter().map(|(_, alias)| *alias).collect::<Vec<_>>();

		let verified =
			verify_or_defer(ring(1), items.into_iter().map(|(item, _)| item).collect()).unwrap();

		assert_eq!(verified.aliases, expected);
		assert_eq!(proof_ring_hashes(), vec![seeded_ring_hash(1); 3]);
		let steps = (1..=3)
			.fold(Weight::zero(), |total, k| total.saturating_add(marginal_weight(k, samples())));
		assert_eq!(verified.registered, steps);
		finalize();
	});
}

#[test]
fn an_empty_push_charges_nothing() {
	new_test_ext().execute_with(|| {
		let verified = verify_or_defer(ring(1), vec![]).unwrap();

		assert!(verified.aliases.is_empty());
		assert_eq!(verified.registered, Weight::zero());
		assert!(batch_is_empty());
	});
}

#[test]
fn a_proof_with_two_contexts_is_rejected_in_both_paths() {
	new_test_ext().execute_with(|| {
		assert_eq!(
			verify_or_defer(ring(1), vec![two_context_proof(1)]).map(|v| v.aliases),
			Err(Error::ContextCountMismatch)
		);
		assert!(batch_is_empty());

		ProofBatch::tmp_note_pool_transaction_validation();
		assert!(verify_or_defer(ring(1), vec![two_context_proof(1)]).is_err());
	});
}

#[test]
fn a_failed_push_writes_nothing() {
	new_test_ext().execute_with(|| {
		let (valid, _) = proof(1, b"a");

		assert_eq!(
			verify_or_defer(ring(1), vec![valid, two_context_proof(1)]).map(|v| v.aliases),
			Err(Error::ContextCountMismatch)
		);
		assert!(batch_is_empty());
		assert_eq!(registered(), Weight::zero());
	});
}

#[test]
fn a_ring_past_max_rings_is_deferred_and_pays_its_setup_at_each_push() {
	new_test_ext().execute_with(|| {
		defer_one(1, b"a");
		append_unused_ring_hashes(MAX_RINGS - 2);
		defer_one(2, b"b");
		assert_eq!(BatchRingHashes::<Test>::decode_len(), Some(MAX_RINGS as usize));

		// A tracked ring pays no setup.
		assert_eq!(defer_one(2, b"c").registered, marginal_weight(3, samples()));

		// An untracked ring is deferred and pays the setup at each push.
		let setup = ProofBatch::ring_setup_weight();
		for (k, message) in [(4, b"d"), (5, b"e")] {
			let verified = defer_one(3, message);
			assert_eq!(verified.registered, marginal_weight(k, samples()).saturating_add(setup));
		}
		assert_eq!(BatchRingHashes::<Test>::decode_len(), Some(MAX_RINGS as usize));
		assert_eq!(BatchProofs::<Test>::decode_len(), Some(5));

		finalize();
		assert!(batch_is_empty());
	});
}

#[test]
fn deferred_proofs_register_their_share_and_ring_setup() {
	new_test_ext().execute_with(|| {
		// The first proof registers `verify_batch_1`, which includes the first ring's setup.
		assert_eq!(defer_one(1, b"a").registered, samples()[0]);
		// A known ring registers its marginal weight.
		assert_eq!(defer_one(1, b"b").registered, marginal_weight(2, samples()));
		// A new ring also registers its setup.
		let setup = ProofBatch::ring_setup_weight();
		assert!(setup.ref_time() > 0);
		assert_eq!(
			defer_one(2, b"c").registered,
			marginal_weight(3, samples()).saturating_add(setup)
		);
		finalize();
	});
}

#[test]
fn the_marginal_weights_cover_the_sampled_batch_cost() {
	new_test_ext().execute_with(|| {
		let mut total = Weight::zero();
		for n in 1..=2_048u32 {
			total.saturating_accrue(defer_one(1, &n.to_le_bytes()).registered);
			if n.is_power_of_two() {
				let sample = samples()[n.ilog2() as usize];
				assert!(total.all_gte(sample), "{n} proofs register {total:?} < {sample:?}");
			}
		}
	});
}

#[test]
fn the_max_weight_covers_every_registration() {
	new_test_ext().execute_with(|| {
		// A first ring, the last tracked ring, an untracked ring and the pool path.
		let first = defer_one(1, b"a");
		append_unused_ring_hashes(MAX_RINGS - 2);
		let checks = [first, defer_one(2, b"b"), defer_one(3, b"c")];
		assert_eq!(BatchRingHashes::<Test>::decode_len(), Some(MAX_RINGS as usize));
		for check in &checks {
			let max_weight = ProofBatch::verify_or_defer_max_weight(1);
			assert!(
				check.registered.all_lte(max_weight),
				"{:?} > {max_weight:?}",
				check.registered
			);
		}
		ProofBatch::tmp_note_pool_transaction_validation();
		assert!(defer_one(1, b"d").registered.all_lte(ProofBatch::verify_or_defer_max_weight(1)));
		let two = verify_or_defer(ring(1), vec![proof(1, b"e").0, proof(1, b"f").0]).unwrap();
		assert!(two.registered.all_lte(ProofBatch::verify_or_defer_max_weight(2)));
		assert!(
			ProofBatch::verify_or_defer_max_weight(1).ref_time() <
				ProofBatch::verify_or_defer_max_weight(2).ref_time()
		);
	});
}

#[test]
fn deferred_aliases_equal_pool_aliases() {
	new_test_ext().execute_with(|| {
		let (item, _) = proof(1, b"a");
		let deferred = verify_or_defer(ring(1), vec![item.clone()]).unwrap();
		ProofBatch::tmp_note_pool_transaction_validation();
		let verified = verify_or_defer(ring(1), vec![item]).unwrap();

		assert_eq!(deferred.aliases, verified.aliases);
	});
}

#[cfg(feature = "runtime-benchmarks")]
#[test]
fn the_bandersnatch_benchmark_helper_builds_distinct_valid_rings() {
	use indiv_support::crypto::BandersnatchVrfVerifiable;
	use verifiable::ring::RingDomainSize;

	frame_support::parameter_types! {
		const Domain: RingDomainSize = RingDomainSize::Domain11;
	}
	type Helper = BandersnatchBenchmarkHelper<Domain>;

	let rings = [0, 1].map(<Helper as BenchmarkHelper<BandersnatchVrfVerifiable>>::ring_with_proof);
	for ring in &rings {
		let proof = &ring.proof;
		let alias = BandersnatchVrfVerifiable::validate(
			ring.config,
			&proof.proof,
			&ring.members,
			&proof.context,
			&proof.message,
		)
		.expect("the helper builds a valid proof");
		assert_eq!(
			BandersnatchVrfVerifiable::extract_unverified_aliases(&proof.proof)
				.unwrap()
				.as_slice(),
			&[alias]
		);
	}
	assert_ne!(rings[0].members, rings[1].members);
}
