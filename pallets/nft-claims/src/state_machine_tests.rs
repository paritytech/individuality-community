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
//! number, and sends them in order. A live batch can be lost on the way. A replay resends trees
//! that were sent before, without a sequence number. Claims, time jumps and offchain-worker runs
//! interleave with the deliveries. Every call the offchain worker submits goes through
//! `authorize` before it is dispatched.
//!
//! After each step the test checks:
//!
//! - `TreeMatchesRoot`: a stored tree is the tree the game chain built for its block.
//! - `ClaimOnce`: a credit is minted at most once, across expiry and redelivery of its tree.
//! - `ClaimantBinding`: only the credit's owner mints it.
//! - `GapsAreReal`: a gap names only sequences that never arrived.
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
	credit_trees::{CreditTreeBlock, CreditTreeDelivery, NftClaimCredit, TreeSequence},
	identity::AccountOrPerson,
};
use proptest::prelude::*;
use proptest_state_machine::{prop_state_machine, ReferenceStateMachine, StateMachineTest};
use sp_runtime::traits::Dispatchable;

const ALICE: u64 = 1;
const BOB: u64 = 2;
const CLAIMANTS: [u64; 2] = [ALICE, BOB];
const COLLECTION: CollectionId = 3;
const COLLECTION_OWNER: u64 = 50;
/// The first account a claim mints to. Each claim mints to a fresh account, because the mock
/// backend holds one NFT per account.
const FIRST_PURSE: u64 = 1_000;
/// The largest time jump of one step. A tree expires `TreeTtl` (two days) after its timestamp,
/// so a few jumps put the early trees past their deadline.
const MAX_JUMP: u64 = 100_000;

/// The block of the tree with `sequence`. Blocks are not contiguous, as on chain.
fn block_of(sequence: TreeSequence) -> CreditTreeBlock {
	10 + 3 * sequence as CreditTreeBlock
}

/// The game chain as the claims chain sees it.
#[derive(Clone, Debug)]
struct GameChain {
	/// The awards of each tree, indexed by sequence number.
	trees: Vec<Vec<Award>>,
	/// The number of live trees sent or lost so far, which is the next sequence to send.
	sent: TreeSequence,
}

#[derive(Clone, Debug)]
enum Step {
	/// Sends the next `count` live trees in one batch. A lost batch never arrives.
	SendLive { count: u64, lost: bool },
	/// Resends the trees of `sequences`, which were all sent before, without sequence numbers.
	Replay { sequences: Vec<TreeSequence> },
	/// `claimant` claims the credit at `award` of tree `sequence`, at the credit's own leaf index.
	Claim { sequence: TreeSequence, award: usize, claimant: u64 },
	/// Moves the clock forward.
	AdvanceTime { secs: u64 },
	/// Runs the offchain worker and dispatches each call it submits that `authorize` accepts.
	OffchainWorker,
}

/// The awards of one tree: `1..=5` leaves, so that some trees have odd layers.
fn tree_awards(sequence: usize) -> impl Strategy<Value = Vec<Award>> {
	prop::collection::vec(prop::sample::select(CLAIMANTS.to_vec()), 1..=5).prop_map(move |owners| {
		owners
			.into_iter()
			.enumerate()
			.map(|(index, owner)| {
				let mut credit: NftClaimCredit = [0u8; 32];
				credit[0] = sequence as u8;
				credit[1] = index as u8;
				(AccountOrPerson::Account(owner), credit)
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
			.prop_map(|trees| GameChain { trees, sent: 0 })
			.boxed()
	}

	fn transitions(state: &GameChain) -> BoxedStrategy<Step> {
		let total = state.trees.len() as u64;
		let remaining = total - state.sent;
		let mut options: Vec<(u32, BoxedStrategy<Step>)> = vec![
			(1, (1..=MAX_JUMP).prop_map(|secs| Step::AdvanceTime { secs }).boxed()),
			(2, Just(Step::OffchainWorker).boxed()),
		];
		if remaining > 0 {
			let most = remaining.min(u64::from(MaxTreesPerMessage::get()));
			options.push((
				3,
				(1..=most, prop::bool::weighted(0.2))
					.prop_map(|(count, lost)| Step::SendLive { count, lost })
					.boxed(),
			));
		}
		if state.sent > 0 {
			let sent = state.sent;
			let most = MaxTreesPerMessage::get() as usize;
			options.push((
				1,
				prop::collection::btree_set(0..sent, 1..=most)
					.prop_map(|sequences| Step::Replay {
						sequences: sequences.into_iter().collect::<Vec<_>>(),
					})
					.boxed(),
			));
			let trees = state.trees.clone();
			options.push((
				4,
				(0..sent, any::<prop::sample::Index>(), 0..10u8)
					.prop_map(move |(sequence, award, roll)| {
						let awards = &trees[sequence as usize];
						let award = award.index(awards.len());
						let AccountOrPerson::Account(owner) = awards[award].0 else {
							unreachable!("the generated awards are account awards")
						};
						// Mostly the owner, sometimes the other claimant.
						let claimant = if roll < 8 { owner } else { ALICE + BOB - owner };
						Step::Claim { sequence, award, claimant }
					})
					.boxed(),
			));
		}
		prop::strategy::Union::new_weighted(options).boxed()
	}

	fn apply(mut state: GameChain, step: &Step) -> GameChain {
		if let Step::SendLive { count, .. } = step {
			state.sent += count;
		}
		state
	}

	fn preconditions(state: &GameChain, step: &Step) -> bool {
		let total = state.trees.len() as u64;
		match step {
			Step::SendLive { count, .. } => state.sent + count <= total,
			Step::Replay { sequences } => sequences.iter().all(|&sequence| sequence < state.sent),
			Step::Claim { sequence, award, .. } =>
				*sequence < state.sent && *award < state.trees[*sequence as usize].len(),
			Step::AdvanceTime { .. } | Step::OffchainWorker => true,
		}
	}
}

/// The claims chain under test, with the history the properties are checked against.
struct ClaimsChain {
	ext: sp_io::TestExternalities,
	now: u64,
	block_number: u64,
	/// The sequences whose live batch arrived.
	arrived: BTreeSet<TreeSequence>,
	/// The credits minted so far, with the block they were claimed from.
	minted: BTreeSet<(CreditTreeBlock, NftClaimCredit)>,
	next_purse: u64,
	/// The offchain-worker transactions already dispatched.
	dispatched: usize,
}

impl ClaimsChain {
	fn delivery(reference: &GameChain, sequence: TreeSequence, live: bool) -> CreditTreeDelivery {
		let block = block_of(sequence);
		CreditTreeDelivery {
			sequence: live.then_some(sequence),
			block,
			tree: tree_of(block, &reference.trees[sequence as usize]),
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
		claimant: u64,
	) {
		let awards = &reference.trees[sequence as usize];
		let (owner, credit) = awards[award].clone();
		let block = block_of(sequence);
		let purse = self.next_purse;
		self.next_purse += 1;

		self.ext.execute_with(|| {
			let result = NftClaims::claim(
				RuntimeOrigin::signed(claimant),
				ClaimantKind::Account,
				block,
				credit,
				award as u32,
				proof_of(awards, award as u32),
				COLLECTION,
				purse,
			);
			if result.is_ok() {
				assert_eq!(
					owner,
					AccountOrPerson::Account(claimant),
					"ClaimantBinding: {claimant} minted the credit of {owner:?}"
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
			for (sequence, awards) in reference.trees.iter().enumerate() {
				let block = block_of(sequence as TreeSequence);
				if let Some(stored) = CreditTrees::<Test>::get(block) {
					assert_eq!(
						stored,
						tree_of(block, awards),
						"TreeMatchesRoot: block {block} holds a tree the game chain did not build"
					);
				}
			}

			for event in nft_claims_events() {
				if let Event::CreditTreesMissing { from_sequence, to_sequence } = event {
					for sequence in from_sequence..=to_sequence {
						assert!(
							!self.arrived.contains(&sequence),
							"GapsAreReal: sequence {sequence} arrived but is reported missing"
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
			set_now(1_000);
		});
		ClaimsChain {
			ext,
			now: 1_000,
			block_number: 1,
			arrived: BTreeSet::new(),
			minted: BTreeSet::new(),
			next_purse: FIRST_PURSE,
			dispatched: 0,
		}
	}

	fn apply(mut chain: ClaimsChain, reference: &GameChain, step: Step) -> ClaimsChain {
		match &step {
			Step::SendLive { count, lost } => {
				// `reference` is the state after the step, so the batch ends at `sent`.
				let first = reference.sent - count;
				if !lost {
					let deliveries = (first..reference.sent)
						.map(|sequence| ClaimsChain::delivery(reference, sequence, true))
						.collect::<Vec<_>>();
					chain.arrived.extend(first..reference.sent);
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
			Step::Claim { sequence, award, claimant } =>
				chain.claim(reference, *sequence, *award, *claimant),
			Step::AdvanceTime { secs } => {
				chain.now += secs;
				let now = chain.now;
				chain.ext.execute_with(|| set_now(now));
			},
			Step::OffchainWorker => chain.run_offchain_worker(),
		}
		chain.check(reference);
		chain
	}
}

prop_state_machine! {
	#![proptest_config(ProptestConfig::with_cases(256))]

	/// Deliveries, replays, claims, time jumps and offchain-worker runs keep every property.
	#[test]
	fn claims_chain_keeps_the_spec_properties(sequential 1..60 => ClaimsChainTest);
}
