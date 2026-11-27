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

//! Random call sequences against the game chain's side of credit delivery, checked against the
//! safety properties of `tla/credit-delivery/CreditDelivery.tla` in individuality-formal at
//! `df6014b`. Each property below names its operator in that spec.
//!
//! The reference machine plays the game pallet and the claims chain. The game pallet awards
//! report, attendance and tenure credits, and ends or cancels games. It checks the remaining
//! capacity before a report or an attendance backfill, as its callers do. The claimants are one
//! group in every round, so a report and the backfill reach the same credit. A bulk award adds up
//! to a whole tree of credits, because `AWARDS_PER_TREE` is a constant too large to reach one
//! credit at a time. The claims chain sends deletions only for trees it received.
//! Blocks, time jumps, failing XCM sends, replays and offchain-worker runs interleave with the
//! awards. Every call the offchain worker submits goes through `authorize` before it is
//! dispatched.
//!
//! After each step the test checks:
//!
//! - `AwardOnce`: a credit slot of a claimant in a game is awarded at most once, and so is a credit
//!   of a claimant.
//! - `NoDroppedAward`, `NoClobberedBuffer` and `AwardCommittedOnce`: no award is dropped or
//!   overwritten, each award is in exactly one root, and that root is the Merkle root over the
//!   awards of its block in leaf order. A block that has awards records a root when it ends.
//! - `BuffersBounded`: buffers stay within `MAX_PENDING_CREDIT_TREES` blocks ahead of the current
//!   block.
//! - `SingleGameTrees`: a tree holds the credits of one game only.
//! - `RootsWrittenOnce`: a root is written once per block.
//! - `TreeMatchesRoot`: every tree sent is the root that its block recorded.
//! - The game chain's side of `NoStrandedCredit`: a sweep removes a root only once the claims
//!   chain's TTL and `ROOT_TTL_GRACE` have passed.
//!
//! The spec has no property for the order of live trees. The test also checks that live trees are
//! sent in sequence order, each sequence at most once and with the block it was queued with.

use crate::{
	mock::*, AwardedNftClaimCredits, CreditBuffers, CreditTreeBatch, Event, NftClaimCreditRoots,
	AWARDS_PER_TREE, MAX_PENDING_CREDIT_TREES,
};
use codec::{Decode, Encode};
use frame_support::{
	pallet_prelude::TransactionSource,
	traits::{Authorize, Hooks},
	BoundedVec,
};
use indiv_pallet_game::{IndexToPlayer, PlayerToIndex};
use indiv_pallet_score::AccountOrPerson;
use indiv_support::credit_trees::{
	credit_leaf, AwardCredits, CreditTreeBlock, NftClaimCredit, NftClaimCreditTree, TreeSequence,
	ROOT_TTL_GRACE,
};
use proptest::prelude::*;
use proptest_state_machine::{prop_state_machine, ReferenceStateMachine, StateMachineTest};
use sp_runtime::{
	traits::{BlakeTwo256, Dispatchable},
	AccountId32,
};
use std::{
	collections::{BTreeMap, BTreeSet},
	time::Duration,
};

/// The games credits are awarded in. Several games can award in one block, which starts a new
/// buffer for each change of game.
const GAMES: u32 = 3;
/// The claimants of report, attendance and tenure credits. They are one group in every round, and
/// the index of a claimant is its position in the group. Few, so that awards often reach the same
/// slot.
const CLAIMANTS: u64 = 3;
/// The rounds a report credit is drawn from. Small, so that slots collide.
const ROUNDS: u8 = 2;
/// The most credits one attendance backfill awards: one per other group member in every round.
const BACKFILL_CREDITS: u32 = ROUNDS as u32 * (CLAIMANTS as u32 - 1);
/// The largest time jump of one step. A root expires `root_ttl` (32 days) after its timestamp, so
/// a few jumps reach the sweep of a root.
const MAX_JUMP: u64 = 20 * 24 * 60 * 60;
const START_TIME: u64 = 10_000;

type Claimant = AccountOrPerson<AccountId32>;

fn claimant(index: u64) -> Claimant {
	AccountOrPerson::Account(id_to_account(100 + index))
}

/// The state the game pallet and the claims chain keep about the games.
#[derive(Clone, Debug, Default)]
struct Games {
	/// Games that ended. Nothing awards in an ended game.
	ended: BTreeSet<u32>,
	/// Players whose game was cancelled for them. Nothing awards them in that game again.
	forgotten: BTreeSet<(u32, u64)>,
}

impl Games {
	fn open(&self, game: u32, player: u64) -> bool {
		!self.ended.contains(&game) && !self.forgotten.contains(&(game, player))
	}
}

#[derive(Clone, Debug)]
enum Step {
	/// `attester` reports `attestee` in `round`, which awards one credit.
	Report {
		game: u32,
		round: u8,
		attester: u64,
		attestee: u64,
	},
	/// `player` is marked as attended, which awards the credits no report awarded them.
	Attend {
		game: u32,
		player: u64,
	},
	/// `player` claims their first `count` tenure credits.
	Tenure {
		game: u32,
		player: u64,
		count: u32,
	},
	/// `count` reports, each awarding one credit to a claimant that holds no other.
	Bulk {
		game: u32,
		count: u32,
	},
	/// The game ends, and its awarded slots are cleared.
	EndGame {
		game: u32,
	},
	/// The game is cancelled for `player`, and their slots are cleared.
	ForgetPlayer {
		game: u32,
		player: u64,
	},
	/// The next block starts and builds the tree of the previous one.
	NextBlock,
	AdvanceTime {
		secs: u64,
	},
	/// Makes XCM sends fail or succeed from now on.
	FailXcm {
		fail: bool,
	},
	/// Anyone replays the trees of up to four blocks that recorded a root. More than
	/// `MaxCreditTreesPerMessage` is refused.
	Replay {
		picks: Vec<prop::sample::Index>,
	},
	/// The claims chain reports trees it received as finished with.
	ReceiveDeletions {
		picks: Vec<prop::sample::Index>,
	},
	/// Runs the offchain worker and dispatches each call it submits that `authorize` accepts.
	OffchainWorker,
}

struct GameChainReference;

impl ReferenceStateMachine for GameChainReference {
	type State = Games;
	type Transition = Step;

	fn init_state() -> BoxedStrategy<Games> {
		Just(Games::default()).boxed()
	}

	fn transitions(_state: &Games) -> BoxedStrategy<Step> {
		let picks = || prop::collection::vec(any::<prop::sample::Index>(), 1..=4);
		prop_oneof![
			6 => (0..GAMES, 0..ROUNDS, 0..CLAIMANTS, 0..CLAIMANTS).prop_map(
				|(game, round, attester, attestee)| Step::Report { game, round, attester, attestee }
			),
			2 => (0..GAMES, 0..CLAIMANTS).prop_map(|(game, player)| Step::Attend { game, player }),
			2 => (0..GAMES, 0..CLAIMANTS, 0..=MaxTenureThresholds::get())
				.prop_map(|(game, player, count)| Step::Tenure { game, player, count }),
			1 => (0..GAMES, 1..=AWARDS_PER_TREE).prop_map(|(game, count)| Step::Bulk { game, count }),
			1 => (0..GAMES).prop_map(|game| Step::EndGame { game }),
			1 => (0..GAMES, 0..CLAIMANTS)
				.prop_map(|(game, player)| Step::ForgetPlayer { game, player }),
			6 => Just(Step::NextBlock),
			3 => (1..=MAX_JUMP).prop_map(|secs| Step::AdvanceTime { secs }),
			1 => any::<bool>().prop_map(|fail| Step::FailXcm { fail }),
			2 => picks().prop_map(|picks| Step::Replay { picks }),
			2 => picks().prop_map(|picks| Step::ReceiveDeletions { picks }),
			4 => Just(Step::OffchainWorker),
		]
		.boxed()
	}

	fn apply(mut state: Games, step: &Step) -> Games {
		match *step {
			Step::EndGame { game } => {
				state.ended.insert(game);
			},
			Step::ForgetPlayer { game, player } => {
				state.forgotten.insert((game, player));
			},
			_ => {},
		}
		state
	}

	fn preconditions(state: &Games, step: &Step) -> bool {
		match *step {
			// A player does not report themselves.
			Step::Report { game, attester, attestee, .. } =>
				attester != attestee && state.open(game, attestee),
			Step::Attend { game, player } | Step::Tenure { game, player, .. } =>
				state.open(game, player),
			Step::Bulk { game, .. } | Step::EndGame { game } => !state.ended.contains(&game),
			Step::ForgetPlayer { game, player } => state.open(game, player),
			_ => true,
		}
	}
}

/// One award as the game chain recorded it.
#[derive(Clone)]
struct RecordedAward {
	game: u32,
	claimant: Claimant,
	credit: NftClaimCredit,
}

/// The game chain under test, with the history the properties are checked against.
struct GameChain {
	ext: sp_io::TestExternalities,
	now: u64,
	/// The slots each claimant, keyed by its encoding, was awarded per game across the whole run.
	awarded_slots: BTreeMap<(u32, Vec<u8>), BTreeSet<u32>>,
	/// Every credit awarded across the whole run, with the encoding of its claimant.
	credits: BTreeSet<(Vec<u8>, NftClaimCredit)>,
	/// The awards of each tree block, by leaf index.
	awards: BTreeMap<CreditTreeBlock, BTreeMap<u32, RecordedAward>>,
	/// The root each block recorded.
	roots: BTreeMap<CreditTreeBlock, NftClaimCreditTree>,
	/// The block each queued sequence belongs to.
	queued: BTreeMap<TreeSequence, CreditTreeBlock>,
	next_sequence: TreeSequence,
	/// The last live sequence sent.
	last_sent: Option<TreeSequence>,
	/// The blocks whose tree reached the claims chain.
	delivered: BTreeSet<CreditTreeBlock>,
	/// The XCM messages already checked.
	seen_xcms: usize,
	/// The offchain-worker transactions already dispatched.
	dispatched: usize,
	/// A counter for bulk claimants, so that each holds one credit.
	next_bulk_claimant: u64,
}

impl GameChain {
	/// Runs one award call of `game` for `claimant` and checks `AwardOnce` on the slots it set.
	fn award(&mut self, game: u32, claimant: &Claimant, call: impl FnOnce() -> u32) {
		let before = AwardedNftClaimCredits::<Test>::get(game, claimant);
		let returned = call();
		let after = AwardedNftClaimCredits::<Test>::get(game, claimant);
		let new_slots = (0..crate::slots::SLOT_CAPACITY)
			.filter(|&slot| after.contains(slot) && !before.contains(slot))
			.collect::<Vec<_>>();
		assert_eq!(
			new_slots.len() as u32,
			returned,
			"AwardOnce: the call awarded {returned} credits to {claimant:?} in game {game} but set \
			 {} new slots",
			new_slots.len()
		);
		let history = self.awarded_slots.entry((game, claimant.encode())).or_default();
		for slot in new_slots {
			assert!(
				history.insert(slot),
				"AwardOnce: slot {slot} of {claimant:?} in game {game} was awarded twice"
			);
		}
	}

	fn bulk(&mut self, game: u32, count: u32) {
		// A caller that cannot split its awards checks the capacity for the whole batch.
		if NftCredits::remaining_capacity(game) < count {
			return;
		}
		for _ in 0..count {
			let attestee =
				AccountOrPerson::Account(id_to_account(1_000_000 + self.next_bulk_claimant));
			self.next_bulk_claimant += 1;
			let attester = claimant(0);
			let time = self.now as u32;
			self.award(game, &attestee, || {
				NftCredits::award_report_credit(game, 0, &attester, &attestee, 0, time)
			});
		}
	}

	fn sent_batches(&mut self) -> Vec<CreditTreeBatch<Test>> {
		use xcm::latest::{Instruction, Xcm};

		let sent = sent_credit_tree_xcms();
		let new = sent[self.seen_xcms..].to_vec();
		self.seen_xcms = sent.len();
		new.into_iter()
			.map(|(_, encoded)| {
				let message = Xcm::<()>::decode(&mut &encoded[..]).expect("XCM decodes");
				let call = message
					.0
					.into_iter()
					.find_map(|instruction| match instruction {
						Instruction::Transact { call, .. } => Some(call.encoded().to_vec()),
						_ => None,
					})
					.expect("the XCM carries a Transact");
				CreditTreeBatch::<Test>::decode(&mut &call[2..]).expect("the batch decodes")
			})
			.collect::<Vec<_>>()
	}

	fn run_offchain_worker(&mut self) {
		let block_number = System::block_number();
		<NftCredits as Hooks<_>>::offchain_worker(block_number);
		let calls = submitted_calls();
		for call in calls.into_iter().skip(self.dispatched) {
			self.dispatched += 1;
			let authorized = call
				.authorize(TransactionSource::Local)
				.expect("an offchain-worker call is an authorized call");
			if authorized.is_ok() {
				let roots_before = NftClaimCreditRoots::<Test>::iter().collect::<BTreeMap<_, _>>();
				call.dispatch(RuntimeOrigin::from(frame_system::RawOrigin::Authorized))
					.expect("an authorized call dispatches");
				for (block, root) in roots_before {
					if !NftClaimCreditRoots::<Test>::contains_key(block) {
						assert!(
							self.now >=
								u64::from(root.timestamp) +
									ClaimsChainTreeTtl::get() + ROOT_TTL_GRACE,
							"NoStrandedCredit: the root of block {block} was swept before its TTL"
						);
					}
				}
			}
		}
	}

	/// Checks the events and messages of the last step, then clears the events.
	fn check(&mut self, step_game: Option<u32>) {
		for record in System::events() {
			let RuntimeEvent::NftCredits(event) = record.event else { continue };
			match event {
				Event::NftClaimCreditAwarded { claimant, credit, block, leaf_index } => {
					let game = step_game.expect("only an award step awards");
					let block = block as CreditTreeBlock;
					assert!(
						self.credits.insert((claimant.encode(), credit)),
						"AwardOnce: a credit of {claimant:?} in game {game} was awarded twice"
					);
					let previous = self
						.awards
						.entry(block)
						.or_default()
						.insert(leaf_index, RecordedAward { game, claimant, credit });
					assert!(
						previous.is_none(),
						"NoClobberedBuffer: leaf {leaf_index} of {block} was overwritten"
					);
					assert!(
						!self.roots.contains_key(&block),
						"AwardCommittedOnce: an award joined a built tree"
					);
				},
				Event::NftClaimCreditDropped { .. } =>
					panic!("NoDroppedAward: an award that passed the capacity check was dropped"),
				Event::NftClaimCreditRootRecorded { block, credit_root } => {
					let block = block as CreditTreeBlock;
					let awards = self.awards.get(&block).cloned().unwrap_or_default();
					assert_eq!(
						awards.keys().copied().collect::<Vec<_>>(),
						(0..awards.len() as u32).collect::<Vec<_>>(),
						"AwardCommittedOnce: the leaves of block {block} are contiguous"
					);
					let leaves = awards
						.values()
						.map(|award| credit_leaf(&award.claimant, &award.credit))
						.collect::<Vec<_>>();
					let root: [u8; 32] =
						binary_merkle_tree::merkle_root::<BlakeTwo256, _>(leaves).into();
					assert_eq!(
						credit_root.root.0, root,
						"AwardCommittedOnce: root of block {block}"
					);
					assert_eq!(credit_root.leaf_count, awards.len() as u32);
					for award in awards.values() {
						assert_eq!(
							award.game, credit_root.game_index,
							"SingleGameTrees: block {block} mixes games"
						);
					}
					assert!(
						self.roots.insert(block, credit_root).is_none(),
						"RootsWrittenOnce: block {block} recorded a second root"
					);
					self.queued.insert(self.next_sequence, block);
					self.next_sequence += 1;
				},
				Event::CreditTreeDeliveryDropped { block } => {
					// The root was recorded in the same block and consumed no sequence.
					self.next_sequence -= 1;
					let dropped = self.queued.remove(&self.next_sequence);
					assert_eq!(dropped, Some(block as CreditTreeBlock));
				},
				_ => {},
			}
		}
		System::reset_events();

		let now = System::block_number();
		for block in self.awards.keys().filter(|&&block| u64::from(block) < now) {
			assert!(
				self.roots.contains_key(block),
				"NoDroppedAward: block {block} has awards and recorded no root"
			);
		}
		for (block, _) in CreditBuffers::<Test>::iter() {
			assert!(
				block >= now && block < now + u64::from(MAX_PENDING_CREDIT_TREES),
				"BuffersBounded: a buffer at block {block} is out of bounds at block {now}"
			);
		}

		for batch in self.sent_batches() {
			for delivery in batch.trees {
				assert_eq!(
					Some(&delivery.tree),
					self.roots.get(&delivery.block),
					"TreeMatchesRoot: the tree sent for block {} is not its root",
					delivery.block
				);
				if let Some(sequence) = delivery.sequence {
					assert!(
						self.last_sent.is_none_or(|last| sequence > last),
						"live sequence {sequence} was sent out of order or twice"
					);
					assert_eq!(
						self.queued.get(&sequence),
						Some(&delivery.block),
						"live sequence {sequence} names another block"
					);
					self.last_sent = Some(sequence);
				}
				self.delivered.insert(delivery.block);
			}
		}
	}
}

struct GameChainTest;

impl StateMachineTest for GameChainTest {
	type SystemUnderTest = GameChain;
	type Reference = GameChainReference;

	fn init_test(_reference: &Games) -> GameChain {
		let mut ext = new_test_ext();
		ext.execute_with(|| {
			MOCK_UNIX_TIME.with(|time| *time.borrow_mut() = Duration::from_secs(START_TIME));
			// `frame_system` records no events in block 0.
			System::set_block_number(1);
			// Small, so that a run of failing sends fills the delivery queue.
			MaxQueuedCreditTrees::set(&2);
			MaxCreditTreesPerMessage::set(&2);
			// No mock helper registers players without a played game, so the group is written
			// directly.
			for index in 0..CLAIMANTS as u32 {
				let player = claimant(u64::from(index));
				let indices = vec![index; usize::from(ROUNDS)];
				PlayerToIndex::<Test>::insert(&player, BoundedVec::truncate_from(indices));
				for round in 0..ROUNDS {
					IndexToPlayer::<Test>::insert((round, index), &player);
				}
			}
			System::reset_events();
		});
		GameChain {
			ext,
			now: START_TIME,
			awarded_slots: BTreeMap::new(),
			credits: BTreeSet::new(),
			awards: BTreeMap::new(),
			roots: BTreeMap::new(),
			queued: BTreeMap::new(),
			next_sequence: 0,
			last_sent: None,
			delivered: BTreeSet::new(),
			seen_xcms: 0,
			dispatched: 0,
			next_bulk_claimant: 0,
		}
	}

	fn apply(mut chain: GameChain, _reference: &Games, step: Step) -> GameChain {
		let mut ext = core::mem::take(&mut chain.ext);
		ext.execute_with(|| {
			let step_game = match step {
				Step::Report { game, .. } |
				Step::Attend { game, .. } |
				Step::Tenure { game, .. } |
				Step::Bulk { game, .. } => Some(game),
				_ => None,
			};
			let time = chain.now as u32;
			match step {
				Step::Report { game, round, attester, attestee } =>
					if NftCredits::remaining_capacity(game) >= 1 {
						let position = attester as u32;
						let (attester, attestee) = (claimant(attester), claimant(attestee));
						chain.award(game, &attestee, || {
							NftCredits::award_report_credit(
								game, round, &attester, &attestee, position, time,
							)
						});
					},
				Step::Attend { game, player } =>
					if NftCredits::remaining_capacity(game) >= BACKFILL_CREDITS {
						let player = claimant(player);
						let group_size = CLAIMANTS as u32;
						chain.award(game, &player, || {
							NftCredits::award_attendance_credits(
								game, ROUNDS, group_size, group_size, &player, time,
							)
						});
					},
				Step::Tenure { game, player, count } => {
					let player = claimant(player);
					chain.award(game, &player, || {
						NftCredits::award_tenure_credits(game, &player, count, time)
					});
				},
				Step::Bulk { game, count } => chain.bulk(game, count),
				Step::EndGame { game } => {
					let mut cursor = None;
					loop {
						cursor = NftCredits::clear_game_credits(game, 2, cursor.as_deref());
						if cursor.is_none() {
							break;
						}
					}
				},
				Step::ForgetPlayer { game, player } =>
					NftCredits::forget_player_credits(game, &claimant(player)),
				Step::NextBlock => {
					let next = System::block_number() + 1;
					System::set_block_number(next);
					NftCredits::on_initialize(next);
				},
				Step::AdvanceTime { secs } => {
					chain.now += secs;
					let now = chain.now;
					MOCK_UNIX_TIME.with(|time| *time.borrow_mut() = Duration::from_secs(now));
				},
				Step::FailXcm { fail } => fail_credit_tree_xcms(fail),
				Step::Replay { picks } => {
					let recorded = chain.roots.keys().copied().collect::<Vec<_>>();
					if !recorded.is_empty() {
						let blocks = picks
							.iter()
							.map(|pick| u64::from(*pick.get(&recorded)))
							.collect::<BTreeSet<_>>()
							.into_iter()
							.collect::<Vec<_>>();
						let blocks = BoundedVec::truncate_from(blocks);
						// The cooldown, a removed root or a failing send refuses a replay.
						let _ = NftCredits::replay_credit_trees(
							RuntimeOrigin::signed(id_to_account(7)),
							blocks,
						);
					}
				},
				Step::ReceiveDeletions { picks } => {
					let delivered = chain.delivered.iter().copied().collect::<Vec<_>>();
					if !delivered.is_empty() {
						let blocks = picks
							.iter()
							.map(|pick| u64::from(*pick.get(&delivered)))
							.collect::<Vec<_>>();
						NftCredits::receive_tree_deletions(
							claims_chain_origin(),
							BoundedVec::truncate_from(blocks),
						)
						.expect("the claims chain's origin is accepted");
					}
				},
				Step::OffchainWorker => chain.run_offchain_worker(),
			}
			chain.check(step_game);
		});
		chain.ext = ext;
		chain
	}
}

prop_state_machine! {
	#![proptest_config(ProptestConfig::with_cases(64))]

	/// Awards, tree building, delivery, replays, deletions and sweeps keep every property.
	#[test]
	fn game_chain_keeps_the_spec_properties(sequential 1..60 => GameChainTest);
}
