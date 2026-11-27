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

//! Random call sequences against the claims chain, checked against the safety properties of
//! `tla/credit-delivery/CreditDelivery.tla` in individuality-formal at `df6014b`. Each property
//! below names its operator in that spec.
//!
//! The reference machine plays the game chain. It holds a fixed list of trees, one per sequence
//! number, and sends them in order. A tree is dated when its batch is sent, less the time the
//! batch waited in the game chain's queue, so a batch can be stale on arrival. A live batch can
//! be lost on the way. A replay resends trees that were sent before, without a sequence number. A
//! conflict resends a block the claims chain holds with another root. Claims, time jumps, failing
//! XCM sends and offchain-worker runs interleave with the deliveries. Every call the offchain
//! worker submits goes through `authorize` before it is dispatched.
//!
//! After each step the test checks:
//!
//! - `TreeMatchesRoot`: a stored tree is the tree the game chain built for its block.
//! - `ClaimOnce`: a credit is minted at most once. A tree that is redelivered after its last claim
//!   mints nothing again, and the chain rejects a tree that is redelivered after its deadline.
//! - `ClaimantBinding`: only the credit's owner mints it, with the credit's own leaf index and
//!   proof.
//! - `GapsAreReal`: a gap never names a sequence the chain accepted. The chain rejects a stale
//!   tree, so a gap can name its sequence.
//! - `ExpectationMonotonic` and `ReplayKeepsSequence`: the expectation never decreases, and a
//!   replay never moves it.
//! - `do_try_state` holds.

use crate::{
	mock::*, ClaimantKind, CollectionMinter, CollectionMinters, CreditTrees, Event, ItemSelection,
	NextExpectedSequence,
};
use alloc::collections::BTreeSet;
use frame_support::{pallet_prelude::TransactionSource, traits::Authorize};
use indiv_pallet_scarcity::CollectionId;
use indiv_support::{
	credit_trees::{
		expiry_deadline, CreditTreeBlock, CreditTreeDelivery, NftClaimCredit, NftClaimCreditTree,
		TreeSequence,
	},
	identity::AccountOrPerson,
};
use proptest::prelude::*;
use proptest_state_machine::{prop_state_machine, ReferenceStateMachine, StateMachineTest};
use sp_runtime::traits::Dispatchable;

const ALICE: u64 = 1;
const BOB: u64 = 2;
const COLLECTION: CollectionId = 3;
const COLLECTION_OWNER: u64 = 50;
/// The first account a claim mints to. Each claim mints to a fresh account, because the mock
/// backend holds one NFT per account.
const FIRST_PURSE: u64 = 1_000;
/// The mock's `TreeTtl`, which `init_test` checks. The strategies run outside the externalities,
/// where the storage that holds it is out of reach.
const TREE_TTL: u64 = 2 * 24 * 60 * 60;
/// The largest time jump of one step. A few jumps put the early trees past their deadline while
/// the chain still holds the later ones.
const MAX_JUMP: u64 = 100_000;
/// Above `2 * TREE_TTL`, the largest age of a batch, so every tree has a timestamp above zero.
const START_TIME: u64 = 1_000_000;

type Claimant = AccountOrPerson<u64>;

/// Returns every identity that owns credits and claims them.
fn claimants() -> Vec<Claimant> {
	vec![
		AccountOrPerson::Account(ALICE),
		AccountOrPerson::Account(BOB),
		AccountOrPerson::Person(PERSON_ALIAS),
	]
}

/// The block of the tree with `sequence`. Blocks are not contiguous, as on chain.
fn block_of(sequence: TreeSequence) -> CreditTreeBlock {
	10 + 3 * sequence as CreditTreeBlock
}

/// The game chain as the claims chain sees it.
#[derive(Clone, Debug)]
struct GameChain {
	/// The awards of each tree, indexed by sequence number.
	trees: Vec<Vec<Award>>,
	/// The timestamp of each tree sent or lost so far, indexed by sequence number.
	timestamps: Vec<u32>,
	now: u64,
}

impl GameChain {
	/// Returns the number of live trees sent or lost so far, which is the next sequence to send.
	fn sent(&self) -> TreeSequence {
		self.timestamps.len() as TreeSequence
	}

	/// Returns the tree of `sequence` as the game chain built it. `sequence` was sent before.
	fn tree(&self, sequence: TreeSequence) -> NftClaimCreditTree {
		NftClaimCreditTree {
			timestamp: self.timestamps[sequence as usize],
			..tree_of(block_of(sequence), &self.trees[sequence as usize])
		}
	}
}

/// What is wrong with a claim besides its claimant.
#[derive(Clone, Copy, Debug)]
enum Fault {
	None,
	/// The claim names the leaf after the credit's own.
	WrongLeaf,
	/// The claim carries the proof of a leaf of another tree.
	ForeignProof,
}

#[derive(Clone, Debug)]
enum Step {
	/// Sends the next `count` live trees in one batch, after the batch waited `age` seconds in the
	/// game chain's queue. A lost batch never arrives.
	SendLive { count: u64, lost: bool, age: u64 },
	/// Resends the trees of `sequences`, which were all sent before, without sequence numbers.
	Replay { sequences: Vec<TreeSequence> },
	/// Resends the block of `sequence` with another root, if the claims chain holds its tree.
	Conflict { sequence: TreeSequence },
	/// `claimant` claims the credit at `award` of tree `sequence`, with `fault`.
	Claim { sequence: TreeSequence, award: usize, claimant: Claimant, fault: Fault },
	/// Moves the clock forward.
	AdvanceTime { secs: u64 },
	/// Makes the XCM sends of tree deletions fail or succeed from now on.
	FailXcm { fail: bool },
	/// Runs the offchain worker and dispatches each call it submits that `authorize` accepts.
	OffchainWorker,
}

/// The awards of one tree: `1..=5` leaves, so that some trees have odd layers.
fn tree_awards(sequence: usize) -> impl Strategy<Value = Vec<Award>> {
	prop::collection::vec(prop::sample::select(claimants()), 1..=5).prop_map(move |owners| {
		owners
			.into_iter()
			.enumerate()
			.map(|(index, owner)| {
				let mut credit: NftClaimCredit = [0u8; 32];
				credit[0] = sequence as u8;
				credit[1] = index as u8;
				(owner, credit)
			})
			.collect::<Vec<_>>()
	})
}

struct ClaimsChainReference;

impl ReferenceStateMachine for ClaimsChainReference {
	type State = GameChain;
	type Transition = Step;

	fn init_state() -> BoxedStrategy<GameChain> {
		(3usize..=6)
			.prop_flat_map(|count| (0..count).map(tree_awards).collect::<Vec<_>>())
			.prop_map(|trees| GameChain { trees, timestamps: Vec::new(), now: START_TIME })
			.boxed()
	}

	fn transitions(state: &GameChain) -> BoxedStrategy<Step> {
		let total = state.trees.len() as u64;
		let sent = state.sent();
		let remaining = total - sent;
		let mut options: Vec<(u32, BoxedStrategy<Step>)> = vec![
			(1, (1..=MAX_JUMP).prop_map(|secs| Step::AdvanceTime { secs }).boxed()),
			(1, any::<bool>().prop_map(|fail| Step::FailXcm { fail }).boxed()),
			(2, Just(Step::OffchainWorker).boxed()),
		];
		if remaining > 0 {
			let most = remaining.min(u64::from(MaxTreesPerMessage::get()));
			let age = prop_oneof![3 => Just(0), 1 => 0..=2 * TREE_TTL];
			options.push((
				3,
				(1..=most, prop::bool::weighted(0.2), age)
					.prop_map(|(count, lost, age)| Step::SendLive { count, lost, age })
					.boxed(),
			));
		}
		if sent > 0 {
			let most = MaxTreesPerMessage::get() as usize;
			options.push((
				1,
				prop::collection::btree_set(0..sent, 1..=most)
					.prop_map(|sequences| Step::Replay {
						sequences: sequences.into_iter().collect::<Vec<_>>(),
					})
					.boxed(),
			));
			options.push((1, (0..sent).prop_map(|sequence| Step::Conflict { sequence }).boxed()));
			let trees = state.trees.clone();
			let other_claimant =
				prop_oneof![4 => Just(None), 1 => prop::sample::select(claimants()).prop_map(Some)];
			let fault = prop_oneof![
				6 => Just(Fault::None),
				1 => Just(Fault::WrongLeaf),
				1 => Just(Fault::ForeignProof)
			];
			options.push((
				4,
				(0..sent, any::<prop::sample::Index>(), other_claimant, fault)
					.prop_map(move |(sequence, award, other_claimant, fault)| {
						let awards = &trees[sequence as usize];
						let award = award.index(awards.len());
						// Defaulting to the owner, so that most claims can mint.
						let claimant = other_claimant.unwrap_or_else(|| awards[award].0.clone());
						Step::Claim { sequence, award, claimant, fault }
					})
					.boxed(),
			));
		}
		prop::strategy::Union::new_weighted(options).boxed()
	}

	fn apply(mut state: GameChain, step: &Step) -> GameChain {
		match *step {
			Step::SendLive { count, age, .. } => {
				let timestamp = (state.now - age) as u32;
				state.timestamps.extend((0..count).map(|_| timestamp));
			},
			Step::AdvanceTime { secs } => state.now += secs,
			_ => {},
		}
		state
	}

	fn preconditions(state: &GameChain, step: &Step) -> bool {
		let total = state.trees.len() as u64;
		match step {
			Step::SendLive { count, age, .. } => state.sent() + count <= total && *age <= state.now,
			Step::Replay { sequences } => sequences.iter().all(|&sequence| sequence < state.sent()),
			Step::Conflict { sequence } => *sequence < state.sent(),
			Step::Claim { sequence, award, .. } =>
				*sequence < state.sent() && *award < state.trees[*sequence as usize].len(),
			Step::AdvanceTime { .. } | Step::FailXcm { .. } | Step::OffchainWorker => true,
		}
	}
}

/// The claims chain under test, with the history the properties are checked against.
struct ClaimsChain {
	ext: sp_io::TestExternalities,
	block_number: u64,
	/// The sequences whose live batch arrived before the deadline of its trees.
	arrived: BTreeSet<TreeSequence>,
	/// The credits minted so far, with the block they were claimed from.
	minted: BTreeSet<(CreditTreeBlock, NftClaimCredit)>,
	next_purse: u64,
	/// The offchain-worker transactions already dispatched.
	dispatched: usize,
}

impl ClaimsChain {
	fn delivery(reference: &GameChain, sequence: TreeSequence, live: bool) -> CreditTreeDelivery {
		CreditTreeDelivery {
			sequence: live.then_some(sequence),
			block: block_of(sequence),
			tree: reference.tree(sequence),
		}
	}

	fn receive(&mut self, deliveries: Vec<CreditTreeDelivery>) {
		self.ext.execute_with(|| {
			let expected = NextExpectedSequence::<Test>::get();
			let replay_only = deliveries.iter().all(|delivery| delivery.sequence.is_none());
			NftClaims::receive_credit_trees(game_chain_origin(), batch(deliveries))
				.expect("the game chain's origin is accepted");
			let after = NextExpectedSequence::<Test>::get();
			assert!(
				after >= expected,
				"ExpectationMonotonic: the expectation went from {expected} to {after}"
			);
			if replay_only {
				assert_eq!(after, expected, "ReplayKeepsSequence: a replay moved the expectation");
			}
		});
	}

	fn claim(
		&mut self,
		reference: &GameChain,
		sequence: TreeSequence,
		award: usize,
		claimant: &Claimant,
		fault: Fault,
	) {
		let awards = &reference.trees[sequence as usize];
		let (owner, credit) = awards[award].clone();
		let block = block_of(sequence);
		let purse = self.next_purse;
		self.next_purse += 1;

		let own_leaf = award as u32;
		let own_proof = proof_of(awards, own_leaf);
		let (leaf_index, proof) = match fault {
			Fault::None => (own_leaf, own_proof.clone()),
			Fault::WrongLeaf => (own_leaf + 1, own_proof.clone()),
			Fault::ForeignProof => {
				let other = &reference.trees[(sequence as usize + 1) % reference.trees.len()];
				(own_leaf, proof_of(other, (award % other.len()) as u32))
			},
		};
		let (origin, kind) = match claimant {
			AccountOrPerson::Account(who) => (RuntimeOrigin::signed(*who), ClaimantKind::Account),
			AccountOrPerson::Person(_) => (RuntimeOrigin::signed(PERSON), ClaimantKind::Person),
		};

		self.ext.execute_with(|| {
			let result = NftClaims::claim(
				origin,
				kind,
				block,
				credit,
				leaf_index,
				proof.clone(),
				COLLECTION,
				purse,
			);
			if result.is_ok() {
				assert_eq!(
					&owner, claimant,
					"ClaimantBinding: {claimant:?} minted the credit of {owner:?}"
				);
				// Two trees of one leaf have the same empty proof, so the proof is compared and
				// not the fault.
				assert!(
					leaf_index == own_leaf && proof == own_proof,
					"ClaimantBinding: a claim with {fault:?} minted the credit at {award} of \
					 block {block}"
				);
				assert!(
					self.minted.insert((block, credit)),
					"ClaimOnce: the credit at {award} of block {block} was minted twice"
				);
			}
		});
	}

	fn run_offchain_worker(&mut self) {
		self.block_number += 1;
		let block_number = self.block_number;
		self.ext.execute_with(|| {
			run_offchain_worker(block_number);
			let calls = submitted_calls();
			for call in calls.into_iter().skip(self.dispatched) {
				self.dispatched += 1;
				let authorized = call
					.authorize(TransactionSource::Local)
					.expect("an offchain-worker call is an authorized call");
				if authorized.is_ok() {
					call.dispatch(RuntimeOrigin::from(frame_system::RawOrigin::Authorized))
						.expect("an authorized call dispatches");
				}
			}
		});
	}

	fn check(&mut self, reference: &GameChain) {
		self.ext.execute_with(|| {
			for sequence in 0..reference.sent() {
				let block = block_of(sequence);
				if let Some(stored) = CreditTrees::<Test>::get(block) {
					assert_eq!(
						stored,
						reference.tree(sequence),
						"TreeMatchesRoot: block {block} holds a tree the game chain did not build"
					);
				}
			}

			for event in nft_claims_events() {
				if let Event::CreditTreesMissing { from_sequence, to_sequence } = event {
					for sequence in from_sequence..=to_sequence {
						assert!(
							!self.arrived.contains(&sequence),
							"GapsAreReal: sequence {sequence} was accepted but is reported missing"
						);
					}
				}
			}

			NftClaims::do_try_state().expect("try_state holds");
		});
	}
}

struct ClaimsChainTest;

impl StateMachineTest for ClaimsChainTest {
	type SystemUnderTest = ClaimsChain;
	type Reference = ClaimsChainReference;

	fn init_test(_reference: &GameChain) -> ClaimsChain {
		let mut ext = new_test_ext();
		ext.execute_with(|| {
			add_collection(COLLECTION, COLLECTION_OWNER, 1_000);
			CollectionMinters::<Test>::insert(
				COLLECTION,
				CollectionMinter { owner: COLLECTION_OWNER, selection: ItemSelection::Random },
			);
			assert_eq!(TreeTtl::get(), TREE_TTL);
			set_now(START_TIME);
		});
		ClaimsChain {
			ext,
			block_number: 1,
			arrived: BTreeSet::new(),
			minted: BTreeSet::new(),
			next_purse: FIRST_PURSE,
			dispatched: 0,
		}
	}

	fn apply(mut chain: ClaimsChain, reference: &GameChain, step: Step) -> ClaimsChain {
		match &step {
			Step::SendLive { count, lost, .. } => {
				// `reference` is the state after the step, so the batch ends at `sent`.
				let batch = reference.sent() - count..reference.sent();
				if !lost {
					let deliveries = batch
						.clone()
						.map(|sequence| ClaimsChain::delivery(reference, sequence, true))
						.collect::<Vec<_>>();
					// The chain rejects a tree past its deadline and leaves its sequence to the
					// gap check.
					chain.arrived.extend(batch.filter(|&sequence| {
						let timestamp = reference.timestamps[sequence as usize];
						reference.now < expiry_deadline(timestamp, TREE_TTL)
					}));
					chain.receive(deliveries);
				}
			},
			Step::Replay { sequences } => {
				let deliveries = sequences
					.iter()
					.map(|&sequence| ClaimsChain::delivery(reference, sequence, false))
					.collect::<Vec<_>>();
				chain.receive(deliveries);
			},
			Step::Conflict { sequence } => {
				let block = block_of(*sequence);
				// The chain stores any tree for a block it does not hold, so only a held block
				// makes the tree a conflict.
				if chain.ext.execute_with(|| CreditTrees::<Test>::contains_key(block)) {
					let mut delivery = ClaimsChain::delivery(reference, *sequence, false);
					delivery.tree.root.0[0] ^= 1;
					chain.receive(vec![delivery]);
				}
			},
			Step::Claim { sequence, award, claimant, fault } =>
				chain.claim(reference, *sequence, *award, claimant, *fault),
			// `reference` is the state after the step, so its clock includes the jump.
			Step::AdvanceTime { .. } => set_now(reference.now),
			Step::FailXcm { fail } => fail_deletion_xcms(*fail),
			Step::OffchainWorker => chain.run_offchain_worker(),
		}
		chain.check(reference);
		chain
	}
}

prop_state_machine! {
	#![proptest_config(ProptestConfig::with_cases(256))]

	/// Deliveries, replays, conflicts, claims, time jumps, failing sends and offchain-worker runs
	/// keep every property.
	#[test]
	fn claims_chain_keeps_the_spec_properties(sequential 1..60 => ClaimsChainTest);
}
