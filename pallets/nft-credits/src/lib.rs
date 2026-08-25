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

//! # NFT Credits Pallet
//!
//! The credits a game awards, the Merkle trees committing to them and their delivery to the chain
//! that mints the NFTs.
//!
//! A credit is earned by playing, so the game pallet is what triggers an award: `report` awards one
//! per `Person` vote and the attendance backfill completes the set once a player's attendance is
//! final. Everything that happens to a credit afterwards is self-contained and lives here: the
//! per-block tree built over the awards, the queue of trees owed to the claims chain, the XCM that
//! carries them, and the proofs a claimant needs.
//!
//! One earning has three names here, one per stage:
//!
//! - A *credit* ([`indiv_support::credit_trees::NftClaimCredit`]) is the hash of one successful
//!   report. A player earns credits and capacity is counted in them.
//! - An *award* ([`NftClaimCreditAward`]) is a credit paired with its claimant. Storage holds
//!   awards, so chunk and buffer sizes count them.
//! - A *leaf* ([`indiv_support::credit_trees::NftClaimCreditLeaf`]) is the hash of an award, at a
//!   position in a tree. Proofs and tree geometry count leaves.
//!
//! The three are one to one, so the counts agree and only the term differs.
//!
//! ## Why a pallet of its own
//!
//! This is the game's own bookkeeping, but none of it is the game: it has its own calls, its own
//! events and its own storage, and mixing them into the game pallet's put three unrelated
//! dispatchables in the same call enum. It sits *above* the game instead ([`Config`] requires
//! `indiv_pallet_game::Config`), so it reads the game's groups and rounds directly, while the game
//! reaches back through [`indiv_support::credit_trees::AwardCredits`] and knows nothing of credits
//! beyond that trait.
//!
//! ## Awarding
//!
//! [`AwardedNftClaimCredits`] marks which of a game's credit slots a claimant already holds, so a
//! credit is awarded once however many times both award paths reach it. Each award is appended to
//! the buffer of the block that commits it in [`NftClaimCreditAwards`] and emitted as
//! `NftClaimCreditAwarded`, in leaf order.
//!
//! ## Committing
//!
//! `on_initialize` builds one binary Merkle tree over the previous block's buffer and records its
//! root in [`NftClaimCreditRoots`] under that block. Blocks whose buffer is empty are skipped. Each
//! award contributes exactly one leaf, `blake2_256` over the SCALE-encoded `(claimant, credit)`.
//!
//! One root per block, rather than one per game, lets a claimant mint as soon as the root
//! committing to their credit reaches the minting chain. Each root stands alone and never changes
//! afterwards, so no inclusion proof goes stale.
//!
//! A buffer holds [`AWARDS_PER_TREE`] awards, which bounds the hook to one tree a block. A block
//! that awards more credits fills the buffers of later blocks, up to
//! [`MAX_PENDING_CREDIT_TREES`] ahead. Callers check [`Pallet::remaining_credit_capacity`] first,
//! so awarding that outruns the trees defers work instead of losing a credit.
//!
//! A credit's *tree block* is the block whose `on_initialize` commits it. It is at or after the
//! block the credit was earned in, and the two differ exactly when awarding spilled. Every storage
//! item, event and API here names the tree block, `NftClaimCreditAwarded` included, because that is
//! the block whose root a proof verifies against.
//!
//! The tree itself is never stored, only its root, and its leaves are recoverable two ways:
//!
//! - From [`NftClaimCreditAwards`], for as long as the tree block is one of the
//!   [`Config::MaxRetainedCreditTrees`] most recent. This is the intended path and needs nothing
//!   but chain state.
//! - From the `NftClaimCreditAwarded` events that name the tree block, one per awarded credit,
//!   carrying the claimant, the credit and the leaf index. Awarding spills, so an event naming a
//!   tree block is emitted in that block or in an earlier one. This is the fallback once a block's
//!   awards have been pruned.
//!
//! What the chain keeps of an award beyond that window is the leaf inside its block's root, which
//! is kept for good, and the slot in [`AwardedNftClaimCredits`] that stops the credit being awarded
//! a second time. A claimant, or an indexer, that held on to the awards can still mint.
//!
//! ## Delivering
//!
//! Every recorded root is queued in [`CreditTreeDeliveryQueue`] under a contiguous sequence number,
//! and the offchain worker ships a message's worth per block with [`Pallet::send_credit_trees`]. A
//! tree that never arrives is repaired by [`Pallet::replay_credit_trees`], which anyone may call
//! and which carries no sequence number. One replay allowed per [`Config::ReplayCooldownSeconds`],
//! so as not to congest the channel.
//!
//! ## Claiming
//!
//! Claiming happens on the claims chain, which never sees the credits themselves, only one root per
//! block. A claimant proves their entitlement by presenting their credit with an inclusion proof:
//! the sibling hashes that rehash the credit's leaf up to the root held for the credit's tree
//! block. A proof verifies only against that one root, and only for the claimant its leaf
//! binds in, so no one else's credit and no other block's root can be minted against it.
//!
//! A claimant does not have to rebuild the tree themselves. The runtime API in [`runtime_api`]
//! serves the proof material:
//!
//! - `nft_claim_credit_roots` resolves [`NftClaimCreditBlocks`], which maps a claimant to their
//!   tree blocks, against [`NftClaimCreditRoots`], so a claimant finds their roots by one lookup
//!   instead of a scan.
//! - `nft_claim_credit_proofs` returns, for one tree block and one claimant, the inclusion proof of
//!   each credit the claimant holds there: credit, leaf, leaf index, leaf count, root and sibling
//!   hashes, which is what the claims chain verifies. `nft_claim_credit_proof_from_awards` does the
//!   same for a pruned block, from awards the caller supplies.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
#[cfg(test)]
mod mock;
pub mod runtime_api;
#[cfg(test)]
mod tests;
mod types;
pub mod weights;

pub use pallet::*;
pub use types::*;
pub use weights::WeightInfo;

use alloc::{vec, vec::Vec};
use codec::Compact;
use core::marker::PhantomData;
use cumulus_primitives_core::{GetChannelInfo, ParaId};
use frame_support::{
	defensive,
	pallet_prelude::*,
	traits::{Defensive, UnixTime},
};
use frame_system::{
	offchain::{CreateAuthorizedTransaction, SubmitTransaction},
	pallet_prelude::*,
};
use indiv_pallet_game::{
	AttesterPosition, GameIdx, GroupsSetting, IndexToPlayer, PlayerToIndex, RoundIndex,
};
// Only the `integrity_test` weighs a `report`, and it leaves that to the production build.
#[cfg(not(feature = "runtime-benchmarks"))]
use indiv_pallet_game::WeightInfo as GameWeightInfo;
use indiv_support::{
	credit_trees::{
		AwardCredits, CreditProofNode, CreditTreeBlock, CreditTreeDelivery, NftClaimCredit,
		NftClaimCreditLeaf, NftClaimCreditTree, TreeSequence,
	},
	identity::AccountOrPerson,
	tx_priority,
	weight_budget::OcwWeightBudget,
};
use sp_runtime::{traits::BlakeTwo256, SaturatedConversion, Saturating};
use xcm::{
	latest::{
		Instruction::{Transact, UnpaidExecution},
		Junction::Parachain,
		Location, OriginKind, SendXcm, WeightLimit, Xcm,
	},
	prelude::send_xcm,
	VersionedXcm,
};

/// Retry window, in blocks, for the offchain worker's [`Pallet::send_credit_trees`].
/// Retries within one window are byte-identical, so the transaction pool deduplicates them.
/// A new window changes the discriminator and thus the transaction hash, which escapes both that
/// deduplication and the pool rotator's inclusion ban.
const CREDIT_TREE_RETRY_WINDOW: u32 = 8;

/// Finite longevity for [`Pallet::send_credit_trees`] so that a stranded retry self-evicts from the
/// pool rather than lingering until it is mined against state it no longer matches.
const CREDIT_TREE_TX_LONGEVITY: u64 = 64;

/// Period, in blocks, at which a failing [`Pallet::send_credit_trees`] submission is warned about
/// rather than logged at `debug`. A stalled delivery is otherwise only visible as a queue that
/// stops draining.
const CREDIT_TREE_STALL_WARN_PERIOD: u32 = 32;

/// Bytes held back from the claims channel's per-message room for what the router adds to a credit
/// tree message after the pallet has handed it over.
///
/// A router appends a `SetTopic` (33 bytes), and the XCMP queue counts the page format byte and,
/// on a channel that takes opaque fragments, the fragment's own length prefix against the same
/// per-message room. None of that is visible to the pallet, so the size it computes is short by up
/// to about 40 bytes and a channel filled to the byte would reject the message.
const CREDIT_TREE_ROUTER_HEADROOM: usize = 64;

/// The number of awards one [`NftClaimCreditAwards`] chunk holds.
///
/// A read is charged at the chunk's `MaxEncodedLen`, so an award pays for one chunk rather than for
/// a whole tree's leaves. Small enough that a worst-case `report` declares close to what it
/// records, large enough that building a tree reads a few dozen keys rather than one per leaf.
pub const AWARDS_PER_CHUNK: u32 = 32;

/// The number of [`NftClaimCreditAwards`] chunks one tree commits.
///
/// The only bound on `on_initialize`, which reads this many keys and hashes every award in them
/// into a leaf. The `integrity_test` asserts that weight against the runtime's block limits. It is
/// also the `clear_prefix` limit that drops a block's awards, so every chunk a buffer wrote is
/// removed.
pub const CHUNKS_PER_TREE: u32 = 64;

/// The number of awards one full buffer holds, which is the leaf count of the tree built over it.
/// Its last chunk is full.
pub const AWARDS_PER_TREE: u32 = CHUNKS_PER_TREE * AWARDS_PER_CHUNK;

/// The number of credit trees that may wait to be built at once, which is how far awarding may run
/// ahead of the one tree a block commits.
///
/// This, times [`AWARDS_PER_TREE`], is what a single block can award, and the state the buffers
/// hold before their trees are built. [`Pallet::remaining_credit_capacity`] reports the room that
/// is left, so a caller that cannot split its awards over blocks waits rather than losing a credit.
///
/// Sized so that waiting stays rare, an order of magnitude above the credits a block of worst-case
/// `report`s declares the weight for, which the `integrity_test` asserts. A runtime running
/// `StorageWeightReclaim` admits reports until what they really record fills the block, which no
/// compile-time figure states exactly.
pub const MAX_PENDING_CREDIT_TREES: u32 = 8;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-credits";

#[frame_support::pallet]
pub mod pallet {
	use super::*;

	#[pallet::pallet]
	pub struct Pallet<T>(PhantomData<T>);

	/// The credits are the game's own bookkeeping, so this pallet is configured on top of it and
	/// reads its groups, rounds and player indices directly.
	#[pallet::config]
	pub trait Config:
		frame_system::Config + indiv_pallet_game::Config + CreateAuthorizedTransaction<Call<Self>>
	{
		/// Weight information for the extrinsics and hooks of this pallet.
		type WeightInfo: WeightInfo;

		/// What the benchmarks cannot set up themselves, because only the runtime knows how its
		/// XCM channels are made.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: crate::benchmarking::BenchmarkHelper;

		/// XCM sender used to deliver the credit trees to [`Config::NftClaimsParaId`].
		type XcmRouter: SendXcm;

		/// The parachain the credit trees are delivered to, which mints the NFTs claimed against
		/// them. There is exactly one, so the runtime fixes it rather than governance registering
		/// it.
		#[pallet::constant]
		type NftClaimsParaId: Get<ParaId>;

		/// Pallet index of indiv-pallet-nft-claims on [`Config::NftClaimsParaId`], used to
		/// encode the `Transact` call the trees are delivered in.
		#[pallet::constant]
		type NftClaimsPalletIndex: Get<u8>;

		/// Channel info provider, used to size a message to what the HRMP channel to
		/// [`Config::NftClaimsParaId`] can carry.
		type ChannelInfo: GetChannelInfo;

		/// The maximum number of credit trees that can wait for delivery in
		/// `CreditTreeDeliveryQueue`.
		///
		/// One block builds at most one tree and the offchain worker drains the queue every block,
		/// so this only has to cover an outage. A tree that does not fit is never queued and needs
		/// [`Pallet::replay_credit_trees`], so size it well past
		/// [`Config::MaxCreditTreesPerMessage`].
		#[pallet::constant]
		type MaxQueuedCreditTrees: Get<u32>;

		/// The maximum number of credit trees carried by one XCM message.
		///
		/// The nft-claims pallet's own bound must be at least this large, otherwise the batches
		/// sent to it fail to decode and the trees in them never arrive.
		#[pallet::constant]
		type MaxCreditTreesPerMessage: Get<u32>;

		/// Cooldown, in seconds, between credit tree replays.
		///
		/// [`Pallet::replay_credit_trees`] is permissionless, so this is what bounds the XCMP
		/// traffic it can cause.
		#[pallet::constant]
		type ReplayCooldownSeconds: Get<u64>;

		/// Per-tree weight surcharge for executing `receive_credit_trees` on
		/// [`Config::NftClaimsParaId`], charged to the caller of
		/// [`Pallet::replay_credit_trees`].
		///
		/// This prices the remote work a replay causes; [`Config::ReplayCooldownSeconds`] is what
		/// bounds how often one can happen. Set it to at least the per-tree cost of
		/// `receive_credit_trees` in the claims chain's own generated weights, proof size
		/// included.
		#[pallet::constant]
		type NftClaimsRemoteWeight: Get<Weight>;

		/// The number of most recently committed credit trees whose [`NftClaimCreditAwards`] stay
		/// on chain.
		///
		/// This is the window in which a claim can be proven from state alone, through
		/// [`Pallet::nft_claim_credit_proofs`]. Once a tree drops out of it, its awards are
		/// removed and a proof has to be rebuilt from the `NftClaimCreditAwarded` events naming
		/// its block and passed to [`Pallet::nft_claim_credit_proof_from_awards`]. The root itself
		/// is kept for good, so dropping out delays no mint that a claimant, or an indexer, kept
		/// the awards of.
		///
		/// It counts trees, not blocks, because only a block that committed one has an entry.
		/// Sized against how long a claimant may take to mint, and paid for in state: the map
		/// holds at most this many trees of [`AWARDS_PER_TREE`] awards each.
		///
		/// Raising the bound in a runtime upgrade is safe. Lowering it orphans the awards of the
		/// blocks beyond the new bound, since `RetainedCreditTreeBlocks` no longer decodes and
		/// the ring is what names the entries to remove, so clear the map first.
		#[pallet::constant]
		type MaxRetainedCreditTrees: Get<u32>;

		/// The maximum number of tree blocks [`NftClaimCreditBlocks`] keeps per claimant.
		///
		/// The index is a lookup aid, not the record of what a claimant is owed, so a full list
		/// drops its oldest block rather than rejecting an award. Size it past the trees a
		/// claimant's credits can land in over the games still worth minting against, and account
		/// for the proof size: a read is charged at the list's maximum
		/// encoded length, one block number per entry, once per distinct claimant an extrinsic
		/// awards to.
		#[pallet::constant]
		type MaxCreditBlocksPerClaimant: Get<u32>;
	}

	/// A batch of credit trees as it is sent to the NFT claims chain.
	pub type CreditTreeBatch<T> =
		indiv_support::credit_trees::CreditTreeBatch<<T as Config>::MaxCreditTreesPerMessage>;

	/// The calls of indiv-pallet-nft-claims that this pallet dispatches over XCM.
	///
	/// The variant's index and its field order must mirror the dispatchable on the claims chain.
	#[derive(Encode)]
	pub(crate) enum NftClaimsCall<T: Config> {
		#[codec(index = 0)]
		ReceiveCreditTrees { batch: CreditTreeBatch<T> },
	}

	/// Which credits a game has already awarded a claimant, keyed by game index and claimant.
	/// The value marks the slots of [`Pallet::credit_slot`] that are taken.
	///
	/// Bookkeeping for [`Pallet::award_nft_claim_credit`], its only reader, and what makes an
	/// award idempotent: `report` awards a `Person` vote's credit on the spot and
	/// `award_attendance_credits` later walks that same credit, which unguarded would give the
	/// claimant a second leaf, in a second block's tree, and two mints from one credit. Off
	/// chain it is not needed, Asset Hub minting against a root it is sent and a wallet reading
	/// [`NftClaimCreditBlocks`] and the blocks' `NftClaimCreditAwarded` events. One word per
	/// claimant rather than an entry per credit, because that entry count is what the backfill's
	/// proof size is made of.
	///
	/// Only the current game is ever in here, entries being drained before
	/// `player_process_step2` kills the game and `new_game` refusing to start one while a game
	/// exists. The game key still earns its place: a slot means nothing outside the game whose
	/// groups it indexes, so were a drain left half done, an unkeyed entry would read as the
	/// next game's awarded slots and silently swallow those credits.
	#[pallet::storage]
	pub type AwardedNftClaimCredits<T: Config> = StorageDoubleMap<
		_,
		Twox64Concat,
		GameIdx,
		Blake2_128Concat,
		AccountOrPerson<T::AccountId>,
		AwardedCredits,
		ValueQuery,
	>;

	/// The tree blocks committing at least one of a claimant's NFT claim credits, in ascending
	/// order and without repeats.
	///
	/// Each entry keys an [`NftClaimCreditRoots`] entry whose tree holds a leaf of the claimant's,
	/// so this answers which roots the claimant has something to mint against without scanning
	/// every block. An entry can name a block that is still to come, because awarding spills into
	/// later buffers; that block gets its root, and its entry here becomes resolvable, once it is
	/// built.
	///
	/// Blocks are appended as credits are awarded and never removed once minted. The list is
	/// therefore a ring bounded by [`Config::MaxCreditBlocksPerClaimant`].
	#[pallet::storage]
	pub type NftClaimCreditBlocks<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		AccountOrPerson<T::AccountId>,
		BoundedVec<BlockNumberFor<T>, T::MaxCreditBlocksPerClaimant>,
		ValueQuery,
	>;

	/// The NFT claim credits each retained tree commits to, in leaf order, which are the preimages
	/// of its Merkle leaves. The first key is the block whose tree holds them, the second the
	/// [`ChunkIndex`] within that block.
	///
	/// A block's chunks double as the buffer the next block's `on_initialize` computes the root
	/// over: `Pallet::award_nft_claim_credit` appends to the last one, and once the root is
	/// recorded the chunks stay as they are, so a claim can be proven from state alone. Every
	/// chunk of a block is removed when a new root pushes it out of the
	/// [`Config::MaxRetainedCreditTrees`] window, which is what bounds the map.
	///
	/// A block holds at most [`CHUNKS_PER_TREE`] chunks. The awards are chunked so that one award
	/// reads one chunk instead of a whole tree's leaves, which keeps a `report`'s declared proof
	/// size close to what it records.
	///
	/// The awards are kept rather than the leaves they hash to, because a mint needs the credit
	/// itself: Asset Hub recomputes the leaf from the claimant and the credit the claimant
	/// presents, so a leaf alone would still leave the credit to be recovered from events.
	#[pallet::storage]
	pub type NftClaimCreditAwards<T: Config> = StorageDoubleMap<
		_,
		Twox64Concat,
		BlockNumberFor<T>,
		Twox64Concat,
		ChunkIndex,
		BoundedVec<NftClaimCreditAward<T::AccountId>, ConstU32<AWARDS_PER_CHUNK>>,
		ValueQuery,
	>;

	/// The tree blocks whose [`NftClaimCreditAwards`] are still on chain, in ascending order.
	///
	/// A ring bounded by [`Config::MaxRetainedCreditTrees`]: recording a root appends its block
	/// and, when that fills the ring, removes the awards of the block that drops off the front.
	/// Keeping the list rather than pruning by block arithmetic means no block ever pays for the
	/// removal of an entry that was never there, tree blocks being sparse.
	#[pallet::storage]
	pub type RetainedCreditTreeBlocks<T: Config> =
		StorageValue<_, BoundedVec<BlockNumberFor<T>, T::MaxRetainedCreditTrees>, ValueQuery>;

	/// The buffers that still owe a tree, keyed by the block whose tree commits each. An entry
	/// holds what its [`NftClaimCreditRoots`] entry will carry besides the root, and how many
	/// awards are in the buffer.
	///
	/// Written when a buffer's first credit is awarded, updated by each one after it and taken
	/// once its root is recorded. Awarding and [`Pallet::build_credit_tree`] read the award count
	/// from here rather than scan the buffer's [`NftClaimCreditAwards`] chunks for its end.
	///
	/// At most [`MAX_PENDING_CREDIT_TREES`] entries, which is the only bound on the awards no
	/// tree commits yet.
	#[pallet::storage]
	pub type CreditBuffers<T: Config> =
		StorageMap<_, Twox64Concat, BlockNumberFor<T>, CreditBuffer>;

	/// The [`CreditBuffers`] entry the next awarded credit goes into, named by its block, which is
	/// the block whose tree commits it.
	///
	/// Ahead of the current block when awarding has spilled. A value at or before the previous
	/// block, zero included, names a buffer whose tree is already built, so awarding starts a fresh
	/// one at the current block.
	#[pallet::storage]
	pub type CreditBufferCursor<T: Config> = StorageValue<_, BlockNumberFor<T>, ValueQuery>;

	/// The Merkle commitment to the NFT claim credits a block committed, keyed by that block.
	/// A block whose buffer held nothing has no entry.
	#[pallet::storage]
	pub type NftClaimCreditRoots<T: Config> =
		StorageMap<_, Twox64Concat, BlockNumberFor<T>, NftClaimCreditTree>;

	/// The tree blocks whose credit trees have not been delivered to the NFT claims chain yet,
	/// in ascending block order, each with the sequence number it is delivered under.
	///
	/// Appended by [`Pallet::build_credit_tree`] and drained from the front by
	/// [`Pallet::send_credit_trees`] once the XCM carrying the trees has been accepted for
	/// delivery. The trees themselves stay in [`NftClaimCreditRoots`]; this only records what
	/// still owes a delivery.
	#[pallet::storage]
	pub type CreditTreeDeliveryQueue<T: Config> = StorageValue<
		_,
		BoundedVec<(TreeSequence, BlockNumberFor<T>), T::MaxQueuedCreditTrees>,
		ValueQuery,
	>;

	/// The time of the last credit tree replay, in seconds.
	///
	/// [`Pallet::replay_credit_trees`] refuses to run again within
	/// [`Config::ReplayCooldownSeconds`] of it.
	#[pallet::storage]
	pub type LastReplayTime<T: Config> = StorageValue<_, u64, OptionQuery>;

	/// The sequence number the next queued credit tree is delivered under.
	///
	/// Only a tree that made it into [`CreditTreeDeliveryQueue`] consumes one, so the sequence the
	/// claims chain sees stays contiguous even when the queue overflows. A gap there therefore
	/// means a message was lost, never that one was not sent.
	#[pallet::storage]
	pub type NextCreditTreeSequence<T: Config> = StorageValue<_, TreeSequence, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// An NFT claim credit was awarded to `claimant` and recorded as leaf `leaf_index` of the
		/// tree of `block`.
		///
		/// `block` is the block whose root commits the credit, which is the current one unless its
		/// tree was already full. An indexer keys the leaf by `block`, not by the block the event
		/// was emitted in.
		///
		/// One event per awarded credit, in leaf order, lets an inclusion proof still be built
		/// once a block's awards have been pruned: the leaf is `blake2_256(claimant ++ credit)`
		/// and the block's leaf set is the `leaf_index`-ordered sequence of the events naming
		/// it, so no block replay is needed.
		NftClaimCreditAwarded {
			claimant: AccountOrPerson<T::AccountId>,
			credit: NftClaimCredit,
			block: BlockNumberFor<T>,
			leaf_index: u32,
		},
		/// The credits `block` committed can be minted from now on: `credit_root`'s root is what
		/// an inclusion proof for any of them verifies against, and never changes.
		NftClaimCreditRootRecorded { block: BlockNumberFor<T>, credit_root: NftClaimCreditTree },
		/// An earned NFT claim credit was not recorded, no buffer having room for it.
		///
		/// The credit is committed to no root and cannot be minted. Its slot is left unset, so the
		/// attendance backfill awards it again if the claimant goes on to attend. Every caller
		/// checks [`Pallet::remaining_credit_capacity`] first, so this event reports a bug.
		NftClaimCreditDropped { claimant: AccountOrPerson<T::AccountId>, credit: NftClaimCredit },
		/// Credit trees were handed to the XCM router for delivery to the NFT claims chain.
		CreditTreesSent {
			/// The tree block of every tree the message carries, in the order they go out.
			/// Empty means every tree of this message had lost its root, so nothing was sent.
			///
			/// The sequence each block travels under is not spelled out: the message takes a
			/// contiguous run of sequences off the queue, starting at the call's `first_sequence`,
			/// minus the ones this block's [`Event::CreditTreeDeliverySkipped`] names.
			trees: BoundedVec<CreditTreeBlock, T::MaxCreditTreesPerMessage>,
		},
		/// A queued credit tree was not sent because its root is no longer recorded.
		///
		/// Its sequence is spent without a tree ever arriving, so the claims chain reports a gap.
		/// No [`Pallet::replay_credit_trees`] can fill it: the root proofs verify against is gone.
		CreditTreeDeliverySkipped { sequence: TreeSequence, block: BlockNumberFor<T> },
		/// Delivering credit trees to the NFT claims chain failed. The trees stay queued and the
		/// next offchain worker cycle retries them.
		CreditTreeSendFailed,
		/// Credit trees were resent to the NFT claims chain out of band.
		CreditTreesReplayed { count: u32 },
		/// A freshly built credit tree could not be queued for delivery because
		/// `CreditTreeDeliveryQueue` is full, which means delivery has been failing for
		/// `MaxQueuedCreditTrees` trees. Its credits stay unmintable until a
		/// [`Pallet::replay_credit_trees`] names `block`.
		CreditTreeDeliveryDropped { block: BlockNumberFor<T> },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// A credit tree replay was requested for an empty list of blocks.
		NoBlocksToReplay,
		/// The blocks to replay are not in strictly ascending order.
		UnsortedReplayBlocks,
		/// None of the blocks to replay has a credit tree.
		NoCreditTreeForBlock,
		/// A credit tree replay ran within `ReplayCooldownSeconds` of the last one. The window is
		/// shared by every caller.
		ReplayCooldownActive,
		/// The replay does not fit what the HRMP channel to the NFT claims chain can carry in
		/// one message.
		ExceedsClaimsChannelCapacity,
		/// Sending the credit trees to the NFT claims chain over XCM failed.
		CreditTreeXcmFailed,
		/// The round is not below `MaxRounds`, or the attester slot is not below `MaxGroupSize`,
		/// so the two name a credit slot no game can use.
		#[cfg(feature = "testnet")]
		CreditSlotOutOfBounds,
		/// No credit was awarded: the claimant already holds the slot's credit for that game, or
		/// no buffer has room for another award.
		#[cfg(feature = "testnet")]
		CreditNotAwarded,
	}

	pub enum AuthorizeInvalidity {
		/// Transaction source is not local or in block.
		TransactionNotLocal = 200,
		/// No credit tree is waiting to be delivered to the NFT claims chain.
		NoQueuedCreditTrees = 201,
	}

	impl From<AuthorizeInvalidity> for TransactionValidityError {
		fn from(e: AuthorizeInvalidity) -> Self {
			InvalidTransaction::Custom(e as u8).into()
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		#[cfg(feature = "std")]
		fn integrity_test() {
			Self::integrity_test_credits();
		}

		/// Builds the tree over the previous block's awards, which are complete by now.
		fn on_initialize(n: BlockNumberFor<T>) -> Weight {
			Self::build_credit_tree(n)
		}

		fn offchain_worker(block_number: BlockNumberFor<T>) {
			Self::submit_credit_tree_delivery(block_number);
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Delivers the queued credit trees that fit one XCM message to the NFT claims chain.
		///
		/// Authorized call submitted by this pallet's offchain worker: it is accepted from a
		/// local or in-block source only, so it cannot be submitted externally.
		///
		/// `first_sequence` must be the sequence at the front of `CreditTreeDeliveryQueue`, which
		/// makes a retry that raced a successful delivery stale rather than a second send.
		#[pallet::authorize(|source, first_sequence, _discriminator| {
			Self::authorize_send_credit_trees(source, first_sequence)
		})]
		#[pallet::call_index(18)]
		#[pallet::weight(<T as Config>::WeightInfo::send_credit_trees(
			T::MaxCreditTreesPerMessage::get()
		))]
		#[pallet::weight_of_authorize(<T as Config>::WeightInfo::authorize_send_credit_trees())]
		pub fn send_credit_trees(
			origin: OriginFor<T>,
			_first_sequence: TreeSequence,
			// Per-window discriminator (the submitting retry window) so that a stalled
			// offchain-worker retry eventually produces a fresh transaction hash.
			_discriminator: BlockNumberFor<T>,
		) -> DispatchResultWithPostInfo {
			ensure_authorized(origin)?;

			Self::do_send_credit_trees()
		}

		/// Resends the credit trees of `blocks` to the NFT claims chain.
		///
		/// Permissionless: a credit tree is a public commitment the claims chain is meant to hold,
		/// and the trees are read from [`NftClaimCreditRoots`] rather than supplied by the caller.
		/// Blocks without a tree are skipped. A resent tree carries no sequence number, so this
		/// cannot disturb the claims chain's tracking of the live stream, and one the claims chain
		/// already holds is ignored there rather than overwriting anything.
		///
		/// One replay runs per [`Config::ReplayCooldownSeconds`], counted from the last one by
		/// [`LastReplayTime`].
		///
		/// The caller pays for the remote work, [`Config::NftClaimsRemoteWeight`] per tree on top
		/// of this call's own weight.
		///
		/// ## Parameters
		/// - `blocks`: The tree blocks to resend, in strictly ascending order.
		#[pallet::call_index(19)]
		#[pallet::weight(
			<T as Config>::WeightInfo::replay_credit_trees(blocks.len() as u32)
				.saturating_add(
					T::NftClaimsRemoteWeight::get().saturating_mul(blocks.len() as u64)
				)
		)]
		pub fn replay_credit_trees(
			origin: OriginFor<T>,
			blocks: BoundedVec<BlockNumberFor<T>, T::MaxCreditTreesPerMessage>,
		) -> DispatchResult {
			ensure_signed(origin)?;

			Self::do_replay_credit_trees(blocks)
		}

		/// Award an NFT claim credit to `claimant` outside of a game.
		///
		/// This action can only be performed by the root origin and is only meant for testing.
		/// It exists because a credit is otherwise only earned by a `Person` vote in a played
		/// game, which makes the claim chain's minting path hard to exercise on its own. The
		/// credit it awards is a normal one: it is recorded in this block's
		/// [`NftClaimCreditAwards`], committed to the block's tree and claimed with the same
		/// proof as any other.
		///
		/// The credit is the one `attester` would earn `claimant` by reporting them a person in
		/// `round` of `game_index`, so it does not need any of those to exist. A slot is used
		/// once per claimant and game: repeating a call with the same `game_index`, `round` and
		/// `attester_position` for the same `claimant` fails rather than awarding a second credit.
		/// Vary any of them to award more than one.
		///
		/// Parameters:
		/// - `claimant`: who the credit is awarded to, and the only identity that can mint it.
		/// - `attester`: the reporter the credit is attributed to, which along with `game_index`
		///   and `round` is what makes the credit distinct from another claimant's.
		/// - `game_index`: the game the credit is attributed to. All the credits a block awards
		///   must name the same game, since the block's tree is labelled with one.
		/// - `round`: the round of that game, below `MaxRounds`.
		/// - `attester_position`: the attester's place in the group, below `MaxGroupSize`. Together
		///   with `round` it picks the claimant's credit slot for the game.
		#[pallet::call_index(103)]
		#[pallet::weight(Weight::zero())]
		#[cfg(feature = "testnet")]
		pub fn testnet_grant_nft_claim_credit(
			origin: OriginFor<T>,
			claimant: AccountOrPerson<T::AccountId>,
			attester: AccountOrPerson<T::AccountId>,
			game_index: GameIdx,
			round: RoundIndex,
			attester_position: AttesterPosition,
		) -> DispatchResult {
			// The `testnet` feature is the only gate, unlike the calls above, which also read
			// `Config::TESTNET`. That constant switches game logic (`acceptable_player_count`)
			// as well, so it cannot be turned on in the mock, and a call behind it is untestable.
			// A grant is root-only, and root can write the same award straight to storage.
			ensure_root(origin)?;

			Self::do_grant_nft_claim_credit(
				claimant,
				attester,
				game_index,
				round,
				attester_position,
			)
		}
	}
}

impl<T: Config> Pallet<T> {
	/// The [`AwardedNftClaimCredits`] slot of the credit the co-player at `attester_position`
	/// of a group awards in `round`.
	///
	/// `attester_position` is the attester's own place in the group, so both award paths read
	/// the same number off the same list: `report` takes the reporter's place once per
	/// round, the reporter being the attester there, and the backfill each co-member's as it
	/// walks the group. The attestee's own place goes unused.
	///
	/// Spacing slots by the configured `MaxGroupSize` rather than the game's keeps the
	/// mapping independent of the group size, below `max_credit_slots`, which the
	/// `integrity_test` holds to [`AwardedCredits::CAPACITY`].
	pub fn credit_slot(round: RoundIndex, attester_position: AttesterPosition) -> CreditSlot {
		CreditSlot::from(round)
			.saturating_mul(T::MaxGroupSize::get())
			.saturating_add(attester_position)
	}

	/// The number of credit slots any game of this runtime can use.
	pub(crate) fn max_credit_slots() -> u32 {
		T::MaxRounds::get().saturating_mul(T::MaxGroupSize::get())
	}

	/// Compute the NFT claim credit for a successful report.
	///
	/// Blake2 256 hash of
	/// ```txt
	/// "polkadot-pop-game" ++ game index ++ attester ++ attestee ++ round
	/// ```
	/// - `game_index`: unsigned 32bit.
	/// - `attester` and `attestee`:
	///   - if an account-based player: 0 ++ account id.
	///   - if a person-based player: 1 ++ person id.
	/// - `round`: unsigned 8bit.
	pub fn compute_nft_claim_credit(
		game_index: GameIdx,
		round: u8,
		attester: &AccountOrPerson<T::AccountId>,
		attestee: &AccountOrPerson<T::AccountId>,
	) -> NftClaimCredit {
		(b"polkadot-pop-game", game_index, round, attester, attestee)
			.using_encoded(sp_io::hashing::blake2_256)
	}

	/// Compute the Merkle leaf committing to `credit` being owned by `claimant`.
	///
	/// Blake2 256 hash of the SCALE encoding of `(claimant, credit)`.
	///
	/// Hashed by the shared [`indiv_support::credit_trees::credit_leaf`], so the claim chain
	/// recomputes the leaf exactly as it was committed. The claimant is bound in because a
	/// credit is itself a hash and does not say who may mint. Nothing else is added: the
	/// credit already commits to the game index, the round and both players.
	pub fn compute_nft_claim_credit_leaf(
		claimant: &AccountOrPerson<T::AccountId>,
		credit: &NftClaimCredit,
	) -> NftClaimCreditLeaf {
		indiv_support::credit_trees::credit_leaf(claimant, credit)
	}

	/// Award `credit`, the credit of `claimant`'s `credit_slot` in this game, and record it in the
	/// buffer [`CreditBufferCursor`] names.
	///
	/// A credit is awarded once and only ever contributes one leaf: a slot already set in
	/// [`AwardedNftClaimCredits`] awards nothing. Both call sites can reach the same slot —
	/// `report` awards a `Person` vote's credit immediately and
	/// `award_attendance_credits` backfills every co-member's credit once the
	/// attendee is finalised — and awarding one twice would let the claimant mint twice from
	/// it, or leave a tree that can never be fully claimed when both leaves land in the same
	/// block.
	///
	/// A credit earned in one block can be committed by a later block's root, awarding having
	/// spilled, which is the block `Event::NftClaimCreditAwarded` names.
	///
	/// The award is recorded before the slot is marked, so a dropped credit marks nothing: a clear
	/// slot lets a later block award it. The [`CreditBuffers`] entry is written after the award for
	/// the same reason, so that a skipped award cannot leave an entry over a buffer with no awards.
	///
	/// Returns the number of awards recorded, which is one for a fresh credit and zero for one
	/// already awarded or one dropped. Callers reserving capacity use it to debit what was really
	/// spent.
	pub fn award_nft_claim_credit(
		game_index: GameIdx,
		claimant: &AccountOrPerson<T::AccountId>,
		credit: NftClaimCredit,
		credit_slot: CreditSlot,
		award_time: u32,
	) -> u32 {
		if !AwardedCredits::within_capacity(credit_slot) {
			// `credit_slot` is below `max_received_votes()`, which the `integrity_test`
			// holds to the mask's capacity, so this cannot be reached.
			defensive!("indiv-pallet-nft-credits: credit slot must fit the awarded credit mask");
			return 0;
		}
		if AwardedNftClaimCredits::<T>::get(game_index, claimant).contains(credit_slot) {
			return 0;
		}

		let now = frame_system::Pallet::<T>::block_number();
		let cursor = CreditBufferCursor::<T>::get();
		let Some((block, buffer)) = Self::credit_award_buffer(now, cursor, game_index) else {
			// Callers check `Self::remaining_credit_capacity` before they award, so awarding
			// cannot run `MAX_PENDING_CREDIT_TREES` ahead of the trees. A credit that reaches
			// here is committed to no root and stays unmintable, which the event reports.
			defensive!("indiv-pallet-nft-credits: a credit must have a buffer to go into");
			return Self::drop_credit(claimant, credit);
		};

		let award = NftClaimCreditAward { claimant: claimant.clone(), credit };
		let leaf_index = buffer.awards;
		if NftClaimCreditAwards::<T>::try_append(block, leaf_index / AWARDS_PER_CHUNK, award)
			.is_err()
		{
			// `Self::credit_award_buffer` only returns a buffer whose award count is below a full
			// tree's, so the chunk it indexes has room.
			defensive!(
				"indiv-pallet-nft-credits: a buffer's chunk must have room for the awarded credit"
			);
			return Self::drop_credit(claimant, credit);
		}

		CreditBuffers::<T>::insert(
			block,
			CreditBuffer {
				// The buffer's first award dates its tree; later awards leave the date alone.
				timestamp: if leaf_index == 0 { award_time } else { buffer.timestamp },
				awards: leaf_index.saturating_add(1),
				..buffer
			},
		);
		if block != cursor {
			CreditBufferCursor::<T>::put(block);
		}

		AwardedNftClaimCredits::<T>::mutate(game_index, claimant, |awarded| {
			awarded.insert(credit_slot)
		});
		Self::note_credit_block(claimant, block);
		Self::deposit_event(Event::<T>::NftClaimCreditAwarded {
			claimant: claimant.clone(),
			credit,
			block,
			leaf_index,
		});
		1
	}

	/// Report `credit` as earned but recorded nowhere, and award nothing.
	///
	/// The slot is left clear, so the attendance backfill can award the credit again. Every caller
	/// checks [`Self::remaining_credit_capacity`] first, so reaching this is a bug; the event is
	/// what tells a claimant why their credit never appeared.
	fn drop_credit(claimant: &AccountOrPerson<T::AccountId>, credit: NftClaimCredit) -> u32 {
		Self::deposit_event(Event::<T>::NftClaimCreditDropped {
			claimant: claimant.clone(),
			credit,
		});
		0
	}

	/// The buffer an award made in block `now` for `game_index` goes into, as it stands before that
	/// award, or `None` once [`MAX_PENDING_CREDIT_TREES`] of them are waiting for their trees.
	///
	/// `cursor` is [`CreditBufferCursor`], and the answer is that buffer unless it is full or
	/// belongs to another game, in which case the next block's. A cursor at or before `now - 1`
	/// names a buffer whose tree is built, so the current block starts a fresh one. One game per
	/// tree keeps `NftClaimCreditTree::game_index` true of every leaf under it.
	///
	/// A buffer that holds nothing yet is returned empty and undated, so the caller writes back one
	/// value either way. Dating it is the caller's, an award being what fixes a tree's timestamp.
	fn credit_award_buffer(
		now: BlockNumberFor<T>,
		cursor: BlockNumberFor<T>,
		game_index: GameIdx,
	) -> Option<(BlockNumberFor<T>, CreditBuffer)> {
		let fresh = CreditBuffer { game_index, timestamp: 0, awards: 0 };
		let buffer = cursor.max(now);
		let (buffer, pending) = match CreditBuffers::<T>::get(buffer) {
			Some(pending)
				if pending.awards < AWARDS_PER_TREE && pending.game_index == game_index =>
				(buffer, pending),
			// A full buffer, one of another game, or none at all. Only the buffer being filled
			// holds anything, so the one after it is empty and needs no read of its own.
			Some(_) => (buffer.saturating_add(One::one()), fresh),
			None => (buffer, fresh),
		};

		// The buffers `now ..= buffer` all owe a tree and a block builds one, so this limit holds
		// their number to `MAX_PENDING_CREDIT_TREES`.
		let limit = now.saturating_add(MAX_PENDING_CREDIT_TREES.into());
		(buffer < limit).then_some((buffer, pending))
	}

	/// Record `tree_block` in `claimant`'s [`NftClaimCreditBlocks`] index.
	///
	/// `tree_block` is the block whose tree commits the credit, not the block the credit was
	/// earned in. The index has to name that block for a proof to be found.
	///
	/// Only the last entry is compared, which is enough to keep the list free of repeats: buffers
	/// are filled in block order, so a block already noted for this claimant can only be the last
	/// one.
	fn note_credit_block(claimant: &AccountOrPerson<T::AccountId>, tree_block: BlockNumberFor<T>) {
		NftClaimCreditBlocks::<T>::mutate(claimant, |blocks| {
			if blocks.last() == Some(&tree_block) {
				return;
			}
			if blocks.try_push(tree_block).is_err() {
				blocks.remove(0);
				let _ = blocks
					.try_push(tree_block)
					.defensive_proof("credit block list must hold one more block after pop");
			}
		});
	}

	/// The number of further NFT claim credits of `game_index` that can be recorded, across the
	/// buffer being filled and the ones that may still be opened.
	///
	/// Falls as credits are awarded and rises by one full buffer with every tree a block builds. A
	/// caller that cannot split its awards over blocks checks this first.
	///
	/// The buffer being filled counts only for the game it holds: a tree carries one game index, so
	/// a credit of another game starts a buffer of its own and what is left of this one is out of
	/// reach. Reporting it as capacity would let the first award of a new game be earned with
	/// nowhere to go.
	pub fn remaining_credit_capacity(game_index: GameIdx) -> u32 {
		let now = frame_system::Pallet::<T>::block_number();
		let cursor = CreditBufferCursor::<T>::get();
		// Asked of the same helper the award itself goes through, so the two cannot disagree about
		// which buffer is next or how much of it is left.
		let Some((buffer, pending)) = Self::credit_award_buffer(now, cursor, game_index) else {
			return 0;
		};
		// The buffers `now ..= buffer` are open already, so the rest is what may still follow.
		let opened = buffer.saturating_sub(now).saturated_into::<u32>().saturating_add(1);
		let further = MAX_PENDING_CREDIT_TREES.saturating_sub(opened);

		AWARDS_PER_TREE
			.saturating_sub(pending.awards)
			.saturating_add(further.saturating_mul(AWARDS_PER_TREE))
	}

	/// The `leaf_count` awards of `block`'s tree, in leaf order.
	///
	/// The caller passes the count rather than the map deciding it, because iterating the chunks
	/// does not come back in leaf order. The last chunk of a tree is short unless the count divides
	/// by [`AWARDS_PER_CHUNK`]; a chunk missing altogether means the awards were pruned, and the
	/// result is then short of `leaf_count`.
	pub(crate) fn block_awards(
		block: BlockNumberFor<T>,
		leaf_count: u32,
	) -> Vec<NftClaimCreditAward<T::AccountId>> {
		(0..leaf_count.div_ceil(AWARDS_PER_CHUNK))
			.flat_map(|chunk| NftClaimCreditAwards::<T>::get(block, chunk))
			.collect::<Vec<_>>()
	}

	/// Build the Merkle tree over the credits buffered for the block before `now` and record
	/// its root.
	///
	/// Runs in `on_initialize`, so that buffer is complete: awarding moves on to a later block's
	/// buffer once a block ends. A block whose buffer holds nothing is skipped entirely and gets no
	/// [`NftClaimCreditRoots`] entry.
	///
	/// One tree a block, over at most [`CHUNKS_PER_TREE`] chunks of awards, bounds this hook. Each
	/// tree stands alone. It is built once, from a complete leaf set, so no root grows over time
	/// and no inclusion proof can go stale. The awards it was built over stay for the retained
	/// window, so a claim can be proven from state; the root itself is never removed.
	///
	/// Every root recorded is also queued for delivery to the NFT claims chain, which the
	/// offchain worker then ships (see [`Pallet::send_credit_trees`]).
	///
	/// Emptiness is decided on [`CreditBuffers`], not on the awards themselves. The
	/// info value is a few bytes and names the award count, so a block that buffered nothing reads
	/// no award chunk.
	pub fn build_credit_tree(now: BlockNumberFor<T>) -> Weight {
		let block = now.saturating_sub(One::one());
		let Some(CreditBuffer { game_index, timestamp, awards: buffered }) =
			CreditBuffers::<T>::take(block)
		else {
			return <T as Config>::WeightInfo::build_credit_tree_empty();
		};

		let awards = Self::block_awards(block, buffered);
		let leaf_count = awards.len() as u32;
		if leaf_count != buffered {
			// A buffer's awards are written with its count, and nothing removes a chunk before the
			// root is recorded, so the two agree. The root commits to what was read either way.
			defensive!("indiv-pallet-nft-credits: buffer must hold every award it counted");
		}
		if leaf_count == 0 {
			defensive!("indiv-pallet-nft-credits: a buffer must hold at least one award");
			return <T as Config>::WeightInfo::build_credit_tree(leaf_count);
		}

		let leaves = Self::nft_claim_credit_leaves(&awards);
		let root = binary_merkle_tree::merkle_root::<BlakeTwo256, _>(leaves).into();
		let credit_root = NftClaimCreditTree { game_index, root, leaf_count, timestamp };
		NftClaimCreditRoots::<T>::insert(block, credit_root);
		Self::retain_credit_awards(block);
		Self::deposit_event(Event::<T>::NftClaimCreditRootRecorded { block, credit_root });
		Self::queue_credit_tree_delivery(block);

		<T as Config>::WeightInfo::build_credit_tree(leaf_count)
	}

	/// Queue the credit tree of `block` for delivery to the NFT claims chain, under the next
	/// delivery sequence number.
	///
	/// A full queue means delivery has been failing for `MaxQueuedCreditTrees` trees. The tree
	/// stays in [`NftClaimCreditRoots`], so [`Self::replay_credit_trees`] can still deliver it.
	/// It consumes no sequence number, so the claims chain never reports a gap for a message
	/// that was never sent.
	pub fn queue_credit_tree_delivery(block: BlockNumberFor<T>) {
		let sequence = NextCreditTreeSequence::<T>::get();
		let mut queued = CreditTreeDeliveryQueue::<T>::get();

		if queued.try_push((sequence, block)).is_err() {
			log::error!(
				target: LOG_TARGET,
				"Credit tree delivery queue is full, the tree of block {block:?} needs a replay",
			);
			Self::deposit_event(Event::<T>::CreditTreeDeliveryDropped { block });
			return;
		}

		CreditTreeDeliveryQueue::<T>::put(queued);
		NextCreditTreeSequence::<T>::put(sequence.saturating_add(1));
	}

	/// Validates a `send_credit_trees` transaction.
	///
	/// The call only ever comes from this pallet's own offchain worker, so it is restricted to
	/// local and in-block sources. `first_sequence` is held to the front of the queue, which
	/// orders retries: a lower sequence has already been delivered and is `Stale`, a higher
	/// one is `Future` until the queue catches up.
	pub fn authorize_send_credit_trees(
		source: TransactionSource,
		first_sequence: &TreeSequence,
	) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
		if !matches!(source, TransactionSource::InBlock | TransactionSource::Local) {
			return Err(AuthorizeInvalidity::TransactionNotLocal.into());
		}

		let Some((queued_sequence, _)) = CreditTreeDeliveryQueue::<T>::get().first().copied()
		else {
			return Err(AuthorizeInvalidity::NoQueuedCreditTrees.into());
		};
		if *first_sequence < queued_sequence {
			return Err(InvalidTransaction::Stale.into());
		}
		if *first_sequence > queued_sequence {
			return Err(InvalidTransaction::Future.into());
		}

		// A finite longevity lets a stranded retry self-evict rather than linger. Propagation
		// is off because peers validate gossiped transactions with a source of `External`,
		// which this call rejects.
		let validity =
			ValidTransaction::with_tag_prefix("game:send-credit-trees")
				.and_provides(queued_sequence)
				.priority(tx_priority::BACKGROUND_PROGRESS.saturating_add(
					frame_system::Pallet::<T>::block_number().saturated_into::<u64>(),
				))
				.longevity(CREDIT_TREE_TX_LONGEVITY)
				.propagate(false)
				.build()
				.expect("tag prefix is not empty; qed");

		Ok((validity, Weight::zero()))
	}

	/// Reads the credit tree of every block in `blocks`, dropping the blocks that have none.
	///
	/// A sequenced block without a tree is inconsistent state, not user error: nothing removes
	/// a tree while its delivery is outstanding. It is logged and reported with
	/// [`Event::CreditTreeDeliverySkipped`], because its sequence is spent either way.
	/// A block from [`Pallet::replay_credit_trees`] carries no sequence and is only logged,
	/// since there a block without a tree is the caller's own choice of argument.
	fn resolve_credit_trees(
		blocks: impl Iterator<Item = (Option<TreeSequence>, BlockNumberFor<T>)>,
	) -> Vec<CreditTreeDelivery> {
		blocks
			.filter_map(|(sequence, block)| {
				let Some(tree) = NftClaimCreditRoots::<T>::get(block) else {
					log::error!(
						target: LOG_TARGET,
						"No credit tree for block {block:?}, skipping its delivery",
					);
					if let Some(sequence) = sequence {
						Self::deposit_event(Event::<T>::CreditTreeDeliverySkipped {
							sequence,
							block,
						});
					}
					return None;
				};
				Some(CreditTreeDelivery {
					sequence,
					block: block.saturated_into::<CreditTreeBlock>(),
					tree,
				})
			})
			.collect::<Vec<_>>()
	}

	/// Sends `updates` to the NFT claims chain in a single XCM message.
	fn send_credit_tree_batch(updates: Vec<CreditTreeDelivery>) -> DispatchResult {
		let trees = BoundedVec::try_from(updates).map_err(|_| {
			defensive!("credit tree batch must fit MaxCreditTreesPerMessage");
			Error::<T>::ExceedsClaimsChannelCapacity
		})?;
		let batch = CreditTreeBatch::<T> { source_time: T::UnixTime::now().as_secs(), trees };

		let call =
			(T::NftClaimsPalletIndex::get(), NftClaimsCall::<T>::ReceiveCreditTrees { batch })
				.encode();
		let destination = Location::new(1, [Parachain(T::NftClaimsParaId::get().into())]);

		send_xcm::<T::XcmRouter>(destination, Self::credit_tree_xcm(call))
			.map_err(|_| Error::<T>::CreditTreeXcmFailed)?;

		Ok(())
	}

	/// The XCM message a credit tree batch travels in.
	fn credit_tree_xcm(encoded_call: Vec<u8>) -> Xcm<()> {
		Xcm(vec![
			UnpaidExecution { weight_limit: WeightLimit::Unlimited, check_origin: None },
			Transact {
				origin_kind: OriginKind::Native,
				call: encoded_call.into(),
				fallback_max_weight: None,
			},
		])
	}

	/// The encoded size of the message carrying `trees` credit trees: a `VersionedXcm<()>` around
	/// the `Transact`, which is what a router compares against the channel's `max_message_size`.
	///
	/// Every field of a delivery is fixed-size, so the batch grows by exactly one
	/// `CreditTreeDelivery` per tree, plus the two compact length prefixes it sits behind: the
	/// batch's tree vector and the `Transact` call's own bytes.
	fn credit_tree_xcm_size(trees: u32) -> usize {
		let empty = CreditTreeBatch::<T> { source_time: 0, trees: BoundedVec::default() };
		let empty_call =
			(u8::MAX, NftClaimsCall::<T>::ReceiveCreditTrees { batch: empty }).encode();

		// The call once the batch holds `trees` deliveries: the empty vector's length prefix gives
		// way to the one for `trees`.
		let call_len = empty_call.len() - Compact(0u32).encoded_size() +
			Compact(trees).encoded_size() +
			trees as usize * CreditTreeDelivery::max_encoded_len();
		// The XCM around the call, whose size is measured with the empty call in it and taken
		// apart again, since only the router knows what the envelope itself encodes to.
		let envelope = VersionedXcm::<()>::from(Self::credit_tree_xcm(empty_call.clone()))
			.encode()
			.len() - empty_call.encoded_size();

		envelope + Compact(call_len as u32).encoded_size() + call_len
	}

	/// The number of credit trees that fit one XCM message to the NFT claims chain, or `None`
	/// when there is no channel to it or not even one tree fits.
	pub fn max_credit_trees_per_message() -> Option<u32> {
		let max_message_size = T::ChannelInfo::get_channel_info(T::NftClaimsParaId::get())
			.map(|info| info.max_message_size as usize)?
			.saturating_sub(CREDIT_TREE_ROUTER_HEADROOM);
		let available = max_message_size.saturating_sub(Self::credit_tree_xcm_size(0));

		// The compact prefixes grow with the batch, so dividing the room by a delivery's size can
		// land one tree over what the channel takes. At most one step walks that back.
		let mut count = (available / CreditTreeDelivery::max_encoded_len()) as u32;
		count = count.min(T::MaxCreditTreesPerMessage::get());
		while count > 0 && Self::credit_tree_xcm_size(count) > max_message_size {
			count -= 1;
		}

		(count > 0).then_some(count)
	}

	/// The per-message room the claims channel needs to carry `trees` credit trees and no more,
	/// which inverts [`Pallet::max_credit_trees_per_message`].
	///
	/// Only tests and benchmarks size a channel, and they need the same headroom the capacity
	/// keeps back for the router, or the message they set the channel up for does not fit it.
	pub fn credit_tree_channel_size(trees: u32) -> u32 {
		(Self::credit_tree_xcm_size(trees) + CREDIT_TREE_ROUTER_HEADROOM) as u32
	}

	/// Submits a [`Pallet::send_credit_trees`] transaction for the queued trees, if any.
	///
	/// Runs every block, so a delivery that failed is retried on the next one. Submission
	/// failures are expected: within a retry window every attempt is byte-identical and the
	/// transaction pool deduplicates them. A streak of them is not, so every
	/// [`CREDIT_TREE_STALL_WARN_PERIOD`] blocks the failure is warned about instead, a stalled
	/// delivery being otherwise visible only as a queue that stops draining.
	pub(crate) fn submit_credit_tree_delivery(block_number: BlockNumberFor<T>) {
		let Some((first_sequence, _)) = CreditTreeDeliveryQueue::<T>::get().first().copied() else {
			return;
		};

		let call = Call::<T>::send_credit_trees {
			first_sequence,
			discriminator: block_number / CREDIT_TREE_RETRY_WINDOW.into(),
		};
		let tx =
			<T as CreateAuthorizedTransaction<Call<T>>>::create_authorized_transaction(call.into());
		if SubmitTransaction::<T, Call<T>>::submit_transaction(tx).is_ok() {
			return;
		}

		if (block_number % CREDIT_TREE_STALL_WARN_PERIOD.into()).is_zero() {
			log::warn!(
				target: LOG_TARGET,
				"offchain worker: `send_credit_trees` repeatedly rejected by the \
				 transaction pool, possible stall",
			);
		} else {
			log::debug!(
				target: LOG_TARGET,
				"offchain worker: failed to submit `send_credit_trees`",
			);
		}
	}

	/// Note `block` as the newest tree block whose [`NftClaimCreditAwards`] are retained,
	/// dropping the oldest one when that exceeds [`Config::MaxRetainedCreditTrees`].
	///
	/// Only one block can drop out per call, one being added, so the ring is walked one entry
	/// at a time and the map holds at most the bound.
	fn retain_credit_awards(block: BlockNumberFor<T>) {
		RetainedCreditTreeBlocks::<T>::mutate(|blocks| {
			if blocks.try_push(block).is_ok() {
				return;
			}
			if blocks.is_empty() {
				// `MaxRetainedCreditTrees` is asserted non-zero by the `integrity_test`, so
				// an empty ring always has room.
				defensive!("indiv-pallet-nft-credits: retained tree ring must hold one block");
				return;
			}
			let dropped = blocks.remove(0);
			// Clears every chunk of the block: a tree holds `CHUNKS_PER_TREE` of them at most. A
			// leftover cursor leaves chunks on chain with nothing naming the block to remove them
			// under.
			if NftClaimCreditAwards::<T>::clear_prefix(dropped, CHUNKS_PER_TREE, None)
				.maybe_cursor
				.is_some()
			{
				defensive!("indiv-pallet-nft-credits: a block's awards must fit one tree's chunks");
			}
			let _ = blocks
				.try_push(block)
				.defensive_proof("retained tree ring must hold one more block after pop");
		});
	}

	/// The Merkle leaves `awards` commit to, in award order.
	fn nft_claim_credit_leaves(
		awards: &[NftClaimCreditAward<T::AccountId>],
	) -> Vec<NftClaimCreditLeaf> {
		awards
			.iter()
			.map(|award| Self::compute_nft_claim_credit_leaf(&award.claimant, &award.credit))
			.collect::<Vec<_>>()
	}

	/// The inclusion proofs of every NFT claim credit of `claimant` that `tree_block` committed,
	/// in leaf order, which is what Asset Hub verifies a mint against.
	///
	/// Reads the block's awards from [`NftClaimCreditAwards`], so a claimant needs nothing
	/// but their own identity: neither the block's other awards, nor the leaf format, nor the
	/// tree layout. Empty if the block committed nothing of `claimant`'s.
	///
	/// Only blocks inside the [`Config::MaxRetainedCreditTrees`] window can be served this
	/// way. An older block gives [`NftClaimCreditProofError::AwardsPruned`], its root being
	/// kept but its awards not, and has to go through
	/// [`Self::nft_claim_credit_proof_from_awards`] instead.
	///
	/// Exposed through the runtime API rather than as a call: nothing is written, and a whole
	/// block's awards in an extrinsic's proof would be paid for by every other extrinsic in
	/// the block.
	pub fn nft_claim_credit_proofs(
		tree_block: BlockNumberFor<T>,
		claimant: &AccountOrPerson<T::AccountId>,
	) -> Result<Vec<NftClaimCreditProof>, NftClaimCreditProofError> {
		let recorded = NftClaimCreditRoots::<T>::get(tree_block)
			.ok_or(NftClaimCreditProofError::UnknownCreditTree)?;
		// A block with a root awarded at least one credit, so no awards means they were
		// pruned rather than that the block never had any.
		let awards = Self::block_awards(tree_block, recorded.leaf_count);
		if awards.is_empty() {
			return Err(NftClaimCreditProofError::AwardsPruned);
		}

		let leaves = Self::nft_claim_credit_leaves(&awards);
		awards
			.iter()
			.enumerate()
			.filter(|(_, award)| &award.claimant == claimant)
			.map(|(leaf_index, award)| {
				Self::credit_proof(&recorded, &leaves, award.credit, leaf_index as u32)
			})
			.collect::<Result<Vec<_>, _>>()
	}

	/// Build the inclusion proof of the credit at `leaf_index` against the
	/// [`NftClaimCreditRoots`] entry of `tree_block`, from `awards` given by the caller.
	///
	/// The fallback for a block whose awards are no longer retained: `awards` are every credit
	/// `tree_block` committed, in leaf order, which a wallet or an indexer rebuilds from the
	/// `NftClaimCreditAwarded` events naming that block. Those events are emitted in it or in an
	/// earlier block, awarding having spilled. Prefer
	/// [`Self::nft_claim_credit_proofs`], which needs no such input.
	///
	/// The recomputed root is checked against the recorded one, so awards that are incomplete
	/// or out of order give [`NftClaimCreditProofError::RootMismatch`] here instead of a proof
	/// Asset Hub silently rejects.
	pub fn nft_claim_credit_proof_from_awards(
		tree_block: BlockNumberFor<T>,
		awards: Vec<NftClaimCreditAward<T::AccountId>>,
		leaf_index: u32,
	) -> Result<NftClaimCreditProof, NftClaimCreditProofError> {
		let recorded = NftClaimCreditRoots::<T>::get(tree_block)
			.ok_or(NftClaimCreditProofError::UnknownCreditTree)?;
		if awards.len() as u32 != recorded.leaf_count {
			return Err(NftClaimCreditProofError::LeafCountMismatch {
				expected: recorded.leaf_count,
			});
		}
		let credit = awards
			.get(leaf_index as usize)
			.ok_or(NftClaimCreditProofError::LeafIndexOutOfBounds)?
			.credit;

		let leaves = Self::nft_claim_credit_leaves(&awards);
		Self::credit_proof(&recorded, &leaves, credit, leaf_index)
	}

	/// The inclusion proof of `leaf_index` in `leaves`, checked against `recorded`.
	///
	/// `leaves` must be the block's complete leaf set in award order; the root check is what
	/// establishes that it is.
	fn credit_proof(
		recorded: &NftClaimCreditTree,
		leaves: &[NftClaimCreditLeaf],
		credit: NftClaimCredit,
		leaf_index: u32,
	) -> Result<NftClaimCreditProof, NftClaimCreditProofError> {
		if leaf_index >= recorded.leaf_count {
			return Err(NftClaimCreditProofError::LeafIndexOutOfBounds);
		}

		let proof =
			binary_merkle_tree::merkle_proof::<BlakeTwo256, _, _>(leaves.to_vec(), leaf_index);
		if CreditProofNode::from(proof.root) != recorded.root {
			return Err(NftClaimCreditProofError::RootMismatch);
		}

		Ok(NftClaimCreditProof {
			root: recorded.root,
			credit,
			leaf: proof.leaf,
			leaf_index,
			leaf_count: recorded.leaf_count,
			proof: proof.proof.into_iter().map(CreditProofNode::from).collect::<Vec<_>>(),
		})
	}

	/// The NFT claim credit roots `claimant` has at least one credit under, in ascending
	/// block order.
	///
	/// Resolves [`NftClaimCreditBlocks`] against [`NftClaimCreditRoots`], so a wallet learns
	/// in one query which tree blocks to ask [`Self::nft_claim_credit_proofs`] about and what
	/// root each of them commits to. A tree block is left out until its root is recorded, which
	/// is in the block after it, so a credit that spilled into a later buffer appears only once
	/// that buffer is committed.
	pub fn nft_claim_credit_roots(
		claimant: &AccountOrPerson<T::AccountId>,
	) -> Vec<(BlockNumberFor<T>, NftClaimCreditTree)> {
		NftClaimCreditBlocks::<T>::get(claimant)
			.into_iter()
			.filter_map(|block| NftClaimCreditRoots::<T>::get(block).map(|root| (block, root)))
			.collect::<Vec<_>>()
	}

	/// Award every NFT claim credit a freshly-attended `attendee` is entitled to for the
	/// current game.
	///
	/// An attendee earns one credit per other member of their group, in every round
	/// they played — irrespective of whether each co-member submitted a report or
	/// what they voted. During the reporting phase the `report` extrinsic awards
	/// these on the fly, but the early-attendance optimisation lets losing players
	/// skip reporting entirely, so attendees can end up missing credits from
	/// non-reporting co-members. This helper closes that gap by walking, for each
	/// round the attendee played in, every other member of their group and
	/// inserting the corresponding credit entry.
	///
	/// Credits already awarded by a real `Person` report are walked again here.
	/// [`Self::award_nft_claim_credit`] leaves those entries untouched, so each credit
	/// keeps its first tree block and is committed to exactly one Merkle root.
	///
	/// The caller must have checked that [`Self::remaining_credit_capacity`] covers
	/// `indiv_pallet_game::Pallet::max_attestations`, the most this can award for one attendee.
	/// Returns how many credits were really awarded, which is fewer whenever a credit was already
	/// awarded during reporting.
	pub(crate) fn award_attendance_credits(
		game_index: GameIdx,
		rounds: u8,
		max_group_size: u32,
		player_count: u32,
		attendee: &AccountOrPerson<T::AccountId>,
		award_time: u32,
	) -> u32 {
		let Some(attendee_indices) = PlayerToIndex::<T>::get(attendee) else {
			// Unregistered attendee (should not happen for `attendance == true`,
			// since `determine_attendance` short-circuits non-registered players).
			defensive!("indiv-pallet-nft-credits: attended player must have round indices");
			return 0;
		};

		let mut awarded = 0;

		let groups_setting = GroupsSetting { max_per_group: max_group_size, player_count };

		for round in 0..rounds {
			let Some(&attendee_index) = attendee_indices.get(round as usize) else {
				defensive!(
					"indiv-pallet-nft-credits: attendee should have an index for each round"
				);
				continue;
			};

			let group_index = groups_setting.group_index_from_player_index(attendee_index);
			// Enumerating the whole group before dropping the attendee is what makes the
			// position the attester's own place in it, the same one `report` reads.
			let co_members = groups_setting
				.group_members(group_index)
				.enumerate()
				.filter(|&(_, index)| index != attendee_index);

			for (attester_position, co_member_index) in co_members {
				let Some(co_member) = IndexToPlayer::<T>::get((round, co_member_index)) else {
					defensive!(
						"indiv-pallet-nft-credits: index should map to a player when awarding credits"
					);
					continue;
				};
				let credit =
					Self::compute_nft_claim_credit(game_index, round, &co_member, attendee);
				let credit_slot = Self::credit_slot(round, attester_position as AttesterPosition);
				awarded = awarded.saturating_add(Self::award_nft_claim_credit(
					game_index,
					attendee,
					credit,
					credit_slot,
					award_time,
				));
			}
		}

		awarded
	}

	/// Delivers the queued credit trees that fit one XCM message, as
	/// [`Pallet::send_credit_trees`] does once its origin is checked.
	///
	/// A message that cannot be built or sent leaves the queue untouched and reports
	/// `CreditTreeSendFailed`, so the next offchain-worker cycle retries the same front.
	pub(crate) fn do_send_credit_trees() -> DispatchResultWithPostInfo {
		let queued = CreditTreeDeliveryQueue::<T>::get();
		debug_assert!(!queued.is_empty(), "authorize should have rejected: nothing queued");

		let Some(message_capacity) = Self::max_credit_trees_per_message() else {
			log::warn!(
				target: LOG_TARGET,
				"No channel capacity to the NFT claims chain, retrying next offchain worker cycle",
			);
			Self::deposit_event(Event::<T>::CreditTreeSendFailed);
			return Ok(Some(<T as Config>::WeightInfo::send_credit_trees(0)).into());
		};

		let taken = (message_capacity as usize).min(queued.len());
		let updates = Self::resolve_credit_trees(
			queued[..taken].iter().map(|(sequence, block)| (Some(*sequence), *block)),
		);

		// The blocks the message delivers, in sequence order, which the event carries so that a
		// gap the claims chain reports can be turned back into the block a replay needs.
		// Truncation is unreachable: `updates` came out of `queued[..taken]`, which
		// `message_capacity` already held to this very bound.
		let sent = BoundedVec::<_, T::MaxCreditTreesPerMessage>::truncate_from(
			updates.iter().map(|update| update.block).collect::<Vec<_>>(),
		);

		// An empty batch means every queued block has lost its tree in the meantime, so there
		// is nothing to send, but the queue entries still have to go.
		let count = updates.len() as u32;
		if count > 0 {
			if let Err(e) = Self::send_credit_tree_batch(updates) {
				log::warn!(
					target: LOG_TARGET,
					"Credit tree XCM failed: {e:?}, retrying next offchain worker cycle",
				);
				Self::deposit_event(Event::<T>::CreditTreeSendFailed);
				return Ok(Some(<T as Config>::WeightInfo::send_credit_trees(count)).into());
			}
		}

		CreditTreeDeliveryQueue::<T>::mutate(|queued| {
			queued.drain(..taken.min(queued.len()));
		});
		Self::deposit_event(Event::<T>::CreditTreesSent { trees: sent });

		Ok(Some(<T as Config>::WeightInfo::send_credit_trees(taken as u32)).into())
	}

	/// Resends the credit trees of `blocks`, as [`Pallet::replay_credit_trees`] does once its
	/// origin is checked.
	///
	/// The cooldown holds how often this may run.
	pub(crate) fn do_replay_credit_trees(
		blocks: BoundedVec<BlockNumberFor<T>, T::MaxCreditTreesPerMessage>,
	) -> DispatchResult {
		ensure!(!blocks.is_empty(), Error::<T>::NoBlocksToReplay);
		ensure!(blocks.windows(2).all(|w| w[0] < w[1]), Error::<T>::UnsortedReplayBlocks);

		let message_capacity =
			Self::max_credit_trees_per_message().ok_or(Error::<T>::ExceedsClaimsChannelCapacity)?;
		ensure!(blocks.len() as u32 <= message_capacity, Error::<T>::ExceedsClaimsChannelCapacity);

		let updates = Self::resolve_credit_trees(blocks.iter().map(|block| (None, *block)));
		ensure!(!updates.is_empty(), Error::<T>::NoCreditTreeForBlock);

		let now = T::UnixTime::now().as_secs();
		if let Some(last) = LastReplayTime::<T>::get() {
			ensure!(
				now.saturating_sub(last) >= T::ReplayCooldownSeconds::get(),
				Error::<T>::ReplayCooldownActive
			);
		}

		let count = updates.len() as u32;
		Self::send_credit_tree_batch(updates)?;
		LastReplayTime::<T>::put(now);

		Self::deposit_event(Event::<T>::CreditTreesReplayed { count });

		Ok(())
	}

	/// Awards `claimant` the credit `attester` would earn them in `round` of `game_index`, as
	/// [`Pallet::testnet_grant_nft_claim_credit`] does once its origin is checked.
	#[cfg(feature = "testnet")]
	pub(crate) fn do_grant_nft_claim_credit(
		claimant: AccountOrPerson<T::AccountId>,
		attester: AccountOrPerson<T::AccountId>,
		game_index: GameIdx,
		round: RoundIndex,
		attester_position: AttesterPosition,
	) -> DispatchResult {
		// The bounds a played game keeps the two within. Checked here so that the call
		// reports an out-of-range round or slot, rather than deriving a credit slot the
		// defensive guard in `award_nft_claim_credit` rejects.
		ensure!(
			u32::from(round) < T::MaxRounds::get() && attester_position < T::MaxGroupSize::get(),
			Error::<T>::CreditSlotOutOfBounds
		);

		let credit_slot = Self::credit_slot(round, attester_position);
		let credit = Self::compute_nft_claim_credit(game_index, round, &attester, &claimant);
		let award_time = T::UnixTime::now().as_secs() as u32;
		let awarded =
			Self::award_nft_claim_credit(game_index, &claimant, credit, credit_slot, award_time);
		ensure!(awarded > 0, Error::<T>::CreditNotAwarded);

		Ok(())
	}

	/// Asserts the configuration invariants of the NFT claim credits, as the pallet's
	/// `integrity_test` runs them.
	pub(crate) fn integrity_test_credits() {
		// Every credit a claimant can earn in a game needs its own slot in
		// `AwardedNftClaimCredits`, otherwise the overflowing ones would be awarded twice,
		// once by `report` and once by the attendance backfill.
		assert!(
			Self::max_credit_slots() <= AwardedCredits::CAPACITY,
			"a game uses up to {slots} credit slots per claimant, more than the {capacity} \
		of `AwardedNftClaimCredits`",
			slots = Self::max_credit_slots(),
			capacity = AwardedCredits::CAPACITY,
		);

		// `report` awards up to `e_max` credits and is refused while fewer than that can be
		// recorded. Empty buffers must hold that many, otherwise every report is refused for good.
		let e_max = indiv_pallet_game::Pallet::<T>::max_enactments().saturating_sub(1);
		let buffered = MAX_PENDING_CREDIT_TREES.saturating_mul(AWARDS_PER_TREE);
		assert!(
			e_max <= buffered,
			"one `report` awards up to {e_max} credits, more than the {buffered} the credit \
		buffers hold",
		);

		// Empty buffers must also absorb a whole block of such reports, which is the state an idle
		// chain leaves them in. Below that, the first busy block refuses a `report` that a later
		// block accepts.
		//
		// The benchmarking build widens the bounds on a game, so the credits it derives are not the
		// ones a live chain awards.
		#[cfg(not(feature = "runtime-benchmarks"))]
		{
			// `Normal.max_total` is the room a block gives the reports, and `report(e_max, 0)` is
			// the cheapest report awarding `e_max` credits, so the two bound how many credits one
			// block awards. Only `ref_time` is compared: `StorageWeightReclaim` replaces the proof
			// size a report declares with what the block recorded, so it is not what fills a block.
			let normal_ref_time = <T as frame_system::Config>::BlockWeights::get()
				.per_class
				.get(DispatchClass::Normal)
				.max_total
				.map(|max_total| max_total.ref_time());
			let report_ref_time =
				<T as indiv_pallet_game::Config>::WeightInfo::report(e_max, 0).ref_time();

			// A `report` that declares no `ref_time`, as a mock's weights do, bounds no number of
			// reports per block.
			if let (Some(normal_ref_time), 1..) = (normal_ref_time, report_ref_time) {
				let per_block =
					(normal_ref_time / report_ref_time).saturating_mul(u64::from(e_max));
				assert!(
					per_block <= u64::from(buffered),
					"a block of `report`s awards up to {per_block} credits, more than the \
				{buffered} the credit buffers hold, so the first busy block of an idle chain \
				refuses a `report`",
				);
			}
		}

		// `CHUNKS_PER_TREE` is the only bound on `on_initialize`, awarding filling a later block's
		// buffer rather than stopping at the block. A tree that does not fit the block would
		// overweigh every block that commits one.
		let build_worst_case = <T as Config>::WeightInfo::build_credit_tree(AWARDS_PER_TREE);
		OcwWeightBudget::from_normal_max::<T>().assert_fits("build_credit_tree", build_worst_case);

		// A message must be fillable from a full queue, otherwise the queue's tail could
		// never be drained in one send.
		assert!(
			T::MaxCreditTreesPerMessage::get() > 0,
			"MaxCreditTreesPerMessage must be greater than zero",
		);
		assert!(
			T::MaxQueuedCreditTrees::get() >= T::MaxCreditTreesPerMessage::get(),
			"MaxQueuedCreditTrees ({queued}) must be >= MaxCreditTreesPerMessage \
		 ({per_message})",
			queued = T::MaxQueuedCreditTrees::get(),
			per_message = T::MaxCreditTreesPerMessage::get(),
		);

		// `send_credit_trees` is an offchain-worker transaction: a worst case above
		// `Normal.max_extrinsic` is dropped at the transaction-pool level, which stalls
		// credit tree delivery for good. `replay_credit_trees` is the manual repair for
		// exactly that case, so it has to stay submittable too.
		let max_trees = T::MaxCreditTreesPerMessage::get();
		let budget = OcwWeightBudget::from_normal_max::<T>();
		budget.assert_fits(
			"send_credit_trees",
			<T as Config>::WeightInfo::send_credit_trees(max_trees)
				.saturating_add(<T as Config>::WeightInfo::authorize_send_credit_trees()),
		);
		budget.assert_fits(
			"replay_credit_trees",
			<T as Config>::WeightInfo::replay_credit_trees(max_trees)
				.saturating_add(T::NftClaimsRemoteWeight::get().saturating_mul(max_trees.into())),
		);

		assert!(
			!T::ReplayCooldownSeconds::get().is_zero(),
			"`ReplayCooldownSeconds` must be at least one",
		);

		// A ring with no room retains no awards at all, leaving every claim to be rebuilt
		// from events, which is the fallback rather than the intended path.
		assert!(
			!T::MaxRetainedCreditTrees::get().is_zero(),
			"`MaxRetainedCreditTrees` must be at least one",
		);

		// Both bounds count trees and gain one per recorded root, so a queue wider than
		// the ring holds trees whose awards have already been pruned. Their delivery still
		// arrives, but the claims it carries are then provable from the block's events only.
		assert!(
			T::MaxRetainedCreditTrees::get() >= T::MaxQueuedCreditTrees::get(),
			"MaxRetainedCreditTrees ({retained}) must be >= MaxQueuedCreditTrees ({queued})",
			retained = T::MaxRetainedCreditTrees::get(),
			queued = T::MaxQueuedCreditTrees::get(),
		);
	}
}

/// What the game triggers, and nothing more: the awards, the block's remaining room for them, and
/// the slots a game gives up when it ends or is cancelled.
///
/// Everything else about a credit — the tree, the delivery, the proofs — is this pallet's own and
/// the game never sees it.
impl<T: Config> AwardCredits<T::AccountId> for Pallet<T> {
	fn award_report_credit(
		game_index: GameIdx,
		round: RoundIndex,
		attester: &AccountOrPerson<T::AccountId>,
		attestee: &AccountOrPerson<T::AccountId>,
		attester_position: AttesterPosition,
		award_time: u32,
	) -> u32 {
		let credit = Self::compute_nft_claim_credit(game_index, round, attester, attestee);
		Self::award_nft_claim_credit(
			game_index,
			attestee,
			credit,
			Self::credit_slot(round, attester_position),
			award_time,
		)
	}

	fn award_attendance_credits(
		game_index: GameIdx,
		rounds: RoundIndex,
		max_group_size: u32,
		player_count: u32,
		attendee: &AccountOrPerson<T::AccountId>,
		award_time: u32,
	) -> u32 {
		Self::award_attendance_credits(
			game_index,
			rounds,
			max_group_size,
			player_count,
			attendee,
			award_time,
		)
	}

	fn remaining_capacity(game_index: GameIdx) -> u32 {
		Self::remaining_credit_capacity(game_index)
	}

	fn clear_game_credits(
		game_index: GameIdx,
		limit: u32,
		cursor: Option<&[u8]>,
	) -> Option<Vec<u8>> {
		AwardedNftClaimCredits::<T>::clear_prefix(game_index, limit, cursor).maybe_cursor
	}

	fn forget_player_credits(game_index: GameIdx, player: &AccountOrPerson<T::AccountId>) {
		AwardedNftClaimCredits::<T>::remove(game_index, player);
	}

	#[cfg(feature = "runtime-benchmarks")]
	fn benchmark_award_every_slot(game_index: GameIdx, player: &AccountOrPerson<T::AccountId>) {
		AwardedNftClaimCredits::<T>::insert(game_index, player, AwardedCredits::FULL);
	}
}
