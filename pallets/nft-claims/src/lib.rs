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

//! # NFT Claims Pallet
//!
//! Holds the Merkle roots committing to the NFT claim credits the game pallet awards on the
//! People chain. A claim is verified against them, by an inclusion proof of the credit's leaf
//! under the root of the block it was awarded in.
//!
//! The commitments and the minting live in one pallet on purpose: the roots have exactly one
//! consumer, the claim, so there is nothing to be gained from splitting the two apart.
//!
//! ## Receiving the roots
//!
//! A root arrives in a `receive_credit_trees` batch that the game pallet sends over XCM, and is
//! stored under the People-chain block whose credits it commits to. Only that pallet's chain can
//! submit the call, through [`Config::EnsureGameChainOrigin`]. Receiving is idempotent: a tree
//! already held is left as it is, so a resend of a tree that did arrive changes nothing, and a
//! root can never be swapped out from under the proofs built against it.
//!
//! ## Claiming
//!
//! [`Pallet::claim`] mints the NFT of one credit. The claimant presents the credit, its leaf index
//! and the sibling hashes the game chain's `nft_claim_credit_proofs` returns, and this pallet
//! rehashes the leaf, `blake2_256(claimant ++ credit)`, up to the root it holds for the award
//! block. The claimant is the signer's own identity under the [`ClaimantKind`] the call names, so
//! a credit awarded to somebody else hashes to a leaf that is in no tree, and the root and leaf
//! count are the stored ones rather than anything the call carries.
//!
//! A claim is a signed transaction, whichever identity it is made under: the call names the
//! [`ClaimantKind`] and [`Config::EnsureClaimant`] resolves the person alias the signer is bound
//! to. The signer pays the transaction fee, in PGAS as any other call, so a failing claim always
//! costs its submitter.
//!
//! The NFT itself is an `indiv-pallet-scarcity` instance, minted with no storage deposit: the
//! credit is what bounds the state a claim creates, since the game chain awards a credit once and
//! [`ClaimedLeaves`] spends it once. Scarcity purse keys hold one NFT each and take no
//! destination consent, so the call names the key to mint to rather than minting to the
//! claimant's own account.
//!
//! ## Claiming privately
//!
//! A game whose schedule opted into the private path mints through [`Pallet::claim_private`].
//! [`Pallet::claim`] refuses that game's trees, which carry a non-zero `private_slots` and say so
//! before the game's ring arrives.
//!
//! The game chain sends one ring per private game through [`Pallet::receive_private_rings`], built
//! over the keys of every claimant that registered. A claim names a slot and proves membership in
//! the game's ring under the context of the game and the slot, and [`Config::RingVrf`] returns the
//! alias of that context. The alias is the nullifier: one key yields one alias per slot, and
//! [`SpentPrivateClaims`] spends each once.
//!
//! The same call carries the other outcome: a game that built no ring is recorded in
//! [`AbandonedPrivateGames`], which reopens [`Pallet::claim`] for its trees. A game reaches one
//! outcome only, and the one that arrives first is kept, so a credit cannot be minted on both
//! paths.
//!
//! Every registrant holds the same slots, so every claim of a game hides in the same set. With a
//! ring per claimant, the set a claim proves against would be a fact about its maker, and the
//! claims of one claimant could be intersected down to the narrowest of them.
//!
//! [`Pallet::claim_private`] is an authorized call. The proof authorizes it, so the transaction
//! carries no signer and pays no fee; a fee payer would be an account that ties together the
//! claims it funded. `authorize` runs the verification, and the dispatch spends the alias it
//! matched.
//!
//! A ring proof costs far more to verify than a Merkle path, so
//! [`Config::MaxPrivateClaimsPerBlock`] bounds how many one block runs. A claim past the cap stays
//! in the pool for a block with room.
//!
//! ## The claim window
//!
//! A ring's claims run in one window: they open [`Config::PrivateClaimDelay`] after the ring
//! arrives and close [`Config::PrivateClaimWindow`] later. The delay opens every member's claims
//! at the same block, and the close keeps them inside one interval a wallet can pick a moment at
//! random from. Without it, claims trail off indefinitely and a late one has the members who had
//! not claimed yet as its anonymity set, however large the ring is.
//!
//! A member who does not claim inside the window mints nothing. The ring is the only path a
//! private game's credits mint on, and the credits their registration spent are not returned.
//! `PrivateRingReceived` names both bounds, so a wallet knows them as soon as the ring lands.
//!
//! Once the window is closed, [`Pallet::close_private_ring`] drops the ring, the aliases spent
//! against it and the game's credit trees. No claim can be made by then, so nothing is kept to
//! stop one. This pallet's offchain worker submits the call, as it submits the tree sweep, so the
//! state goes at the deadline rather than when somebody pays to reclaim it. The game is recorded
//! in [`ClosedPrivateGames`], which refuses a later outcome for it and a later delivery of its
//! trees: without the aliases the dropped ring's slots would mint again.
//!
//! [`PrivateRingCloses`] files every held ring under the block it closes in and iterates in that
//! order, so the worker finds the next game to close in one read.
//!
//! ## Collections and item selection
//!
//! The claimant names the collection a claim mints into, and a collection accepts claims only
//! once its owner has registered it through [`Pallet::set_collection_minter`], choosing an
//! [`ItemSelection`]: deposit-free minting inflates a collection's supply, so it takes the
//! owner's opt-in. A registration remains valid only while that owner holds the collection, so a
//! new owner has to register it again. The registration decides which of the collection's items a
//! claim mints:
//!
//! - [`ItemSelection::Contract`] asks the named contract, `mint(uint32 collection, bytes32
//!   entropy)`, with the credit as the only entropy, and mints the item index it returns. The
//!   current collection owner makes the bounded call and collateralizes its storage writes. Any
//!   failure fails the claim and leaves the credit unspent because the contract is how an owner
//!   gates their collection.
//! - [`ItemSelection::Random`] needs no contract: the item index is the credit modulo the
//!   collection's next item index. A claimant chooses which collection to claim into, but not the
//!   item within it: for a fixed collection and item set the credit maps to one item and the credit
//!   is fixed by game events before any claim. This assumes the owner defines the collection's
//!   items before opening it to claims, since the next item index is the modulus: adding items
//!   shifts which item a credit maps to, and deleting one leaves a hole a credit can still land on
//!   and fail.
//!
//! ## Missing trees
//!
//! Award blocks are not contiguous, since a block that awarded no credit has no tree, so a
//! missing tree cannot be spotted from the block numbers. Each tree of the live stream instead
//! carries a contiguous sequence number, and a batch whose first sequence is ahead of the one
//! expected means the trees in between never arrived: [`Event::CreditTreesMissing`] names them.
//! Recovering them is a `replay_credit_trees` call on the game pallet, naming the award blocks,
//! which anyone can submit. A resent tree carries no sequence number and leaves the tracking of
//! the live stream alone.
//!
//! The sequences a gap names are turned back into those award blocks on the game chain. Its
//! `CreditTreesSent` event lists the blocks one message delivered, in the order they go out, and
//! its `send_credit_trees` call names the sequence the run starts at, so walking the run pairs
//! each sequence with a block. The sequences left out of it are the ones the game pallet spent on
//! a tree whose root it had already dropped, named one by one by `CreditTreeDeliverySkipped`. No
//! replay recovers those: the root a proof would verify against no longer exists on either chain.
//!
//! ## Removing trees
//!
//! Three paths remove a tree. All of them tell the game chain to drop its own copy, so the trees
//! each chain holds will be able to process all open claims.
//!
//! - **Fully claimed.** The set bits of [`ClaimedLeaves`] reach the tree's `leaf_count`. Every
//!   credit the tree commits to has been minted, so no proof can be built against it again, and the
//!   claim that completes it removes it.
//! - **Private game closed.** A private game's claim window closes and its ring is dropped. A
//!   private claim proves against the ring, never against a tree, and [`Pallet::claim`] refuses a
//!   tree that names slots unless the game was abandoned. A game that built a ring reaches no
//!   second outcome, so its trees are unclaimable from the moment the ring arrives.
//!   [`PrivateGameTrees`] names them, and [`Pallet::close_private_ring`] removes them with the
//!   ring.
//! - **Expiry.** A tree that neither of those paths removed outlives [`Config::TreeTtl`]. The TTL
//!   runs from the award block's own wall-clock time, which the game chain records in the tree, not
//!   from the time the tree arrived here. [`Pallet::claim`] does not check the TTL, so the sweep
//!   that removes the tree is what ends claimability, and [`Event::CreditTreesExpired`] reports
//!   that those unclaimed credits are unmintable from then on. A private game's tree reaches the
//!   sweep when the game was abandoned and its credits went unclaimed, or when no outcome arrived
//!   at all and they were mintable on neither path.
//!
//! [`Pallet::sweep_expired_trees`] performs the expiry, and this pallet's offchain worker submits
//! it. [`TreeExpiries`] files each tree under the timestamp its deadline runs from and iterates in
//! that order, so a sweep reads only the trees that are due. A delivery whose tree is already past
//! its deadline is not stored at all: the game chain holds its root for longer, and anyone can call
//! `replay_credit_trees` there to deliver an expired tree again.
//!
//! [`ClaimedLeaves`] outlives the tree it belongs to. A replay on the game chain can bring a
//! removed tree back, because anyone can call it and the root outlives this chain's copy. The spent
//! leaves stop that tree from minting its credits a second time.
//!
//! One bitmap covers a whole award block, so the sweep drops it with a single removal. The expiry
//! entry of a fully claimed tree therefore stays behind after the tree goes, and the sweep of that
//! entry is what removes the bitmap. That is also the point where a replay stops mattering: a
//! delivery past the deadline is refused, so nothing can spend those leaves again.
//!
//! A closed private game is the one case where a replay is refused before the deadline. Its
//! redelivered tree is unclaimable, so storing it leaves state that only the sweep removes.
//! [`Event::CreditTreePrivateGameClosed`] reports the refusal.
//!
//! The deletions owed to the game chain queue in [`PendingTreeDeletions`] and travel in a
//! [`Pallet::send_tree_deletions`] message, which the offchain worker submits as well. A deletion
//! is idempotent and carries no sequence number. The game chain's own TTL covers a deletion that is
//! lost, or that the queue had no room for, so no repair call exists.
//!
//! [`Pallet::sweep_expired_trees`], [`Pallet::send_tree_deletions`] and
//! [`Pallet::close_private_ring`] take a local or in-block transaction source only, so nobody can
//! submit them from outside the node that authored them. [`Pallet::claim_private`] is the one
//! authorized call of this pallet that any source may submit.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
pub mod migration;
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

use alloc::vec;
use frame_support::{
	dispatch::{PostDispatchInfo, WithPostDispatchInfo},
	traits::{EnsureOrigin, EnsureOriginWithArg, Get, UnixTime},
	weights::Weight,
};
use frame_system::offchain::CreateAuthorizedTransaction;
use indiv_pallet_scarcity::{
	CollectionId, InspectCollection, InstanceId, ItemIndex, MintWithoutDeposit,
};
use indiv_support::{
	context::{build_product_context, private_nft_claims, ProductContextNetworkSuffix},
	credit_trees::{
		authorize_expiry_sweep, credit_leaf, drain_due_expiries, expiry_deadline, oldest_expiry,
		AwardBlock, CreditProofNode, ExpirySweepTx, ExpiryTimestamp, NftClaimCredit,
		NftClaimCreditLeaf, NftClaimCreditTree, PrivateClaimSlot, PrivateGameOutcome,
		PrivateRingBatch, PrivateRingDelivery, TreeSequence,
	},
	identity::AccountOrPerson,
	offchain::{submit_authorized, RETRY_WINDOW, TX_LONGEVITY},
	traits::{Alias, RingExponent},
	tx_priority,
	utils::BigEndianU64,
	weight_budget::OcwWeightBudget,
};
use sp_core::{H160, H256};
use sp_runtime::{traits::BlakeTwo256, DispatchError, SaturatedConversion, Saturating};
use verifiable::GenerateVerifiable;
use xcm::{
	latest::{
		Instruction::{Transact, UnpaidExecution},
		Location, OriginKind, SendError, SendXcm, WeightLimit, Xcm,
	},
	prelude::send_xcm,
};

pub use indiv_support::credit_trees::GameIdx;

/// The per-message room assumed for the channel to the game chain. The `integrity_test` holds a
/// full deletion message to it.
///
/// The real figure comes from the relay chain's channel configuration, which is unknown at build
/// time. Every HRMP channel between system parachains sits well above this, so a message that
/// passes the check fits the channel.
const MIN_CHANNEL_MESSAGE_SIZE: usize = 4096;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-claims";

/// Number of metadata entries a claim mints with, which `mint_hook_weight` prices per entry.
///
/// The weight annotation runs before the dispatch builds that metadata, so this cannot read the
/// vector's length. It is also the ceiling, because a dispatch may refund weight but never add
/// any, so a mint passing more entries than this undercharges and reports nothing. Raise it in
/// the same change that gives the mint metadata to pass.
const CLAIM_METADATA_PAIRS: u32 = 0;

/// How many spent aliases one [`Pallet::close_private_ring`] call removes.
///
/// A closed window's ring holds one alias per claim that was made, so the removal runs in bounded
/// steps. It is a pallet constant because only the weight of one call depends on it. The trees the
/// same call removes are bounded by [`Config::MaxTreeDeletionsPerMessage`] instead, one step
/// queueing at most one deletion message's worth.
pub const PRIVATE_CLOSE_ITEMS: u32 = 32;

/// How many blocks a `claim_private` submission stays valid in the pool.
///
/// Only a full block turns a claim away, so a claim has to outlive a burst. A claim that outlives
/// this window is dropped, and its alias stays unspent.
const PRIVATE_CLAIM_TX_LONGEVITY: u64 = 64;

/// Successful output of a collection's minter contract.
pub struct Selection {
	/// The item, within the collection the contract was asked about, the claim mints.
	pub item: ItemIndex,
	/// Weight the selection really consumed, refunded against
	/// [`CollectionSelector::max_weight`]. Must not exceed it.
	pub weight_consumed: Weight,
}

/// Failure of a collection's minter contract call.
pub struct SelectionError {
	/// What failed, which fails the claim.
	pub error: DispatchError,
	/// Weight the call really consumed before it failed, charged against
	/// [`CollectionSelector::max_weight`]. Must not exceed it.
	pub weight_consumed: Weight,
}

/// Runtime adapter calling a collection's minter contract as its current owner.
///
/// The contract exposes `mint(uint32 collection, bytes32 entropy) returns (uint32 item)` and uses
/// the claimed credit as its only entropy. The runtime limits execution and storage deposits.
pub trait CollectionSelector<AccountId> {
	/// Worst-case weight of one selection, reserved before dispatch.
	fn max_weight() -> Weight;

	/// Confirm `contract` can be registered as a minter, which is that code is deployed at the
	/// address.
	///
	/// Run once at registration to fail typos and not-yet-deployed contracts there, with a
	/// clear error, rather than on every claim. It is a courtesy, not a guarantee: nothing
	/// on-chain proves the code implements the minter interface, so [`Self::select`] still
	/// validates every call's outcome.
	fn validate(contract: H160) -> Result<(), DispatchError>;

	/// Ask `contract` as `owner` which of `collection`'s items the claim of `entropy` mints.
	///
	/// A failure reports the weight the call consumed before failing, so the claim charges it.
	fn select(
		owner: AccountId,
		contract: H160,
		collection: CollectionId,
		entropy: NftClaimCredit,
	) -> Result<Selection, SelectionError>;
}

/// What the benchmarks cannot set up themselves, because only the runtime knows how its NFT
/// backend is administered.
#[cfg(feature = "runtime-benchmarks")]
pub trait BenchmarkHelper<AccountId, Crypto: GenerateVerifiable> {
	/// Make `collection` exist owned by `owner`, with `item` defined in it, as the owner would
	/// have done before the first claim.
	fn prepare_collection(owner: &AccountId, collection: CollectionId, item: ItemIndex);

	/// Deploy a contract that the collection registration benchmark can validate.
	fn prepare_contract(owner: &AccountId) -> H160;

	/// A private claim ring, a proof of membership in it made for `context` over `message`, and
	/// the alias the proof yields.
	///
	/// The prover paths are off-chain, so only the runtime can build these. Without them a
	/// benchmark of `authorize_claim_private` measures a verification that fails early. The call
	/// carries the alias, so the benchmark needs it too.
	fn private_ring_and_proof(
		context: &[u8; 32],
		message: &[u8],
	) -> (Crypto::Members, Crypto::Proof, Alias);

	/// Moves [`Config::UnixTime`] to `secs` since the UNIX epoch.
	///
	/// A sweep's validity depends on the clock, so a benchmark of it sets the clock. Only the
	/// runtime knows which pallet holds it.
	fn set_unix_time(secs: u64);

	/// Opens a channel to the game chain that carries `max_message_size` bytes per message.
	///
	/// A benchmarked send reaches [`Config::XcmRouter`], which refuses a destination it has no
	/// channel to. Only the runtime knows how its channels are made.
	fn open_game_chain_channel(max_message_size: u32);
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use alloc::vec::Vec;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	/// The current storage version.
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config + CreateAuthorizedTransaction<Call<Self>> {
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Origin check for the XCM messages carrying credit trees, which authenticates the
		/// chain the game pallet runs on.
		type EnsureGameChainOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// Wall clock a tree's `timestamp` is measured against, which decides expiry.
		///
		/// The `timestamp` comes from the game chain's clock, so expiry compares two chains'
		/// clocks. A [`Config::TreeTtl`] of weeks or months exceeds any real skew between them.
		type UnixTime: UnixTime;

		/// How long, in seconds after the `timestamp` its tree commits to, a credit stays
		/// claimable.
		///
		/// [`Pallet::claim`] does not check this, so a claim succeeds until a sweep removes the
		/// tree, which happens in the first block past the deadline that includes a sweep.
		#[pallet::constant]
		type TreeTtl: Get<u64>;

		/// The maximum number of award blocks that can wait for a deletion message in
		/// [`PendingTreeDeletions`].
		///
		/// One message takes [`Config::MaxTreeDeletionsPerMessage`] blocks off the front, and the
		/// offchain worker gets one message into the pool per block, so the queue holds what
		/// claims and sweeps add above that rate. A deletion that does not fit is dropped,
		/// because the game chain's TTL, not this queue, removes its copy. Set it at least as high
		/// as [`Config::MaxTreeDeletionsPerMessage`], otherwise one sweep drops deletions of its
		/// own.
		#[pallet::constant]
		type MaxQueuedTreeDeletions: Get<u32>;

		/// The maximum number of award blocks carried by one deletion message, which is also how
		/// many trees one [`Pallet::sweep_expired_trees`] removes.
		///
		/// One bound serves both, so a sweep queues exactly one message's worth and those deletions
		/// go out in the block after it. It also bounds the sweep's weight against the block, and
		/// the offchain worker submits one sweep per block until nothing is due.
		/// The game pallet's own bound must be at least this large, otherwise the message fails
		/// to decode there and its deletions never arrive.
		#[pallet::constant]
		type MaxTreeDeletionsPerMessage: Get<u32>;

		/// XCM sender used to tell [`Config::GameChainLocation`] which trees to delete.
		type XcmRouter: SendXcm;

		/// Where the game pallet runs, and where the deletions go.
		/// [`Config::EnsureGameChainOrigin`] authenticates the same chain, so both must name it.
		type GameChainLocation: Get<Location>;

		/// Pallet index of indiv-pallet-nft-credits on [`Config::GameChainLocation`], used to
		/// encode the `Transact` the deletions are delivered in.
		#[pallet::constant]
		type GameChainPalletIndex: Get<u8>;

		/// Maximum number of credit trees accepted in one batch.
		///
		/// Must be at least the game pallet's `MaxCreditTreesPerMessage`, otherwise the batches
		/// it sends fail to decode and the trees in them are lost.
		#[pallet::constant]
		type MaxTreesPerMessage: Get<u32>;

		/// Origin check for a claim, resolving the signer to the identity a credit's leaf binds
		/// under the [`ClaimantKind`] the call names.
		///
		/// The origin has to stay a signed one, so that the signer pays the transaction's fee.
		/// Resolving [`ClaimantKind::Person`] means looking up the alias the signer is bound to,
		/// which fails for a signer that has none.
		type EnsureClaimant: EnsureOriginWithArg<
			Self::RuntimeOrigin,
			ClaimantKind,
			Success = AccountOrPerson<Self::AccountId>,
		>;

		/// The NFTs a claim mints, which is `indiv-pallet-scarcity`.
		///
		/// Minting is deposit-free: the credit is what bounds the state a claim creates, since a
		/// credit is awarded once by the game chain and this pallet spends it once. The inspect
		/// side gates [`Pallet::set_collection_minter`] on the collection's owner and sizes the
		/// [`ItemSelection::Random`] draw.
		type Nfts: MintWithoutDeposit<Self::AccountId> + InspectCollection<Self::AccountId>;

		/// Executes a registered [`ItemSelection::Contract`] minter as the current collection
		/// owner.
		///
		/// Every claim reserves `max_weight` and refunds it down to what the selection really
		/// consumed, whether the claim succeeds or fails. The runtime also limits the storage
		/// deposit the owner can pay for one call.
		type CollectionSelector: CollectionSelector<Self::AccountId>;

		/// Maximum number of sibling hashes an inclusion proof may carry.
		///
		/// A tree of `n` leaves needs `ceil(log2(n))` of them, so this must cover the game
		/// chain's `MaxCreditsPerBlock`: a lower bound leaves the tail of a large tree unclaimable.
		#[pallet::constant]
		type MaxProofNodes: Get<u32>;

		/// The most credits one award block commits to, which is the game chain's
		/// `MaxCreditsPerBlock`.
		///
		/// [`ClaimedLeaves`] holds one bit per leaf, so this sizes that bitmap. A tree committing
		/// to more leaves is not stored, because the leaves past the bitmap could be claimed
		/// twice. Set it to the game chain's own bound, which a lower value makes large trees
		/// undeliverable.
		#[pallet::constant]
		type MaxCreditsPerAwardBlock: Get<u32>;

		/// The ring VRF a private claim is proven with. Set it to the suite the game chain builds
		/// its private claim rings with.
		type RingVrf: GenerateVerifiable<
			Proof: Send + Sync + DecodeWithMemTracking,
			Member: DecodeWithMemTracking,
			Members: DecodeWithMemTracking + verifiable::DecodeUnchecked,
			Config: Send + Sync + DecodeWithMemTracking + TryFrom<RingExponent>,
		>;

		/// The ring capacity exponent the game chain builds its private claim rings at.
		///
		/// A proof is verified against this configuration, so any other value rejects every
		/// private claim.
		#[pallet::constant]
		type PrivateRingExponent: Get<RingExponent>;

		/// The network suffix the private claim contexts are built with.
		///
		/// Both chains and every wallet derive the same contexts from it. A runtime that changes
		/// it invalidates every proof made under the old one.
		type PrivateClaimNetworkSuffix: Get<ProductContextNetworkSuffix>;

		/// Maximum number of private claim rings accepted in one batch.
		#[pallet::constant]
		type MaxPrivateRingsPerMessage: Get<u32>;

		/// The most private claims one block executes.
		///
		/// A ring VRF verification is far heavier than a Merkle path, so this keeps a burst of
		/// them inside the block budget. A claim past the cap is rejected, not queued, and its
		/// sender retries in a later block.
		#[pallet::constant]
		type MaxPrivateClaimsPerBlock: Get<u32>;

		/// Blocks between a private game's ring arriving and its claims opening.
		///
		/// Every member's claims open in the same block, so claiming early says nothing about who
		/// claimed. Set it to what a wallet needs to see the ring and pick a moment inside the
		/// window; zero opens the claims in the block the ring arrives in.
		#[pallet::constant]
		type PrivateClaimDelay: Get<BlockNumberFor<Self>>;

		/// Blocks a private game's claim window stays open, counted from the block its claims
		/// open in.
		///
		/// It is the interval every claim of the game falls in, and therefore the span the claims
		/// of one member can be spread over. A member who does not claim inside it mints nothing,
		/// so weigh the anonymity a narrow window buys against the mints a wide one saves.
		#[pallet::constant]
		type PrivateClaimWindow: Get<BlockNumberFor<Self>>;

		/// Setup the claim benchmarks need from the NFT backend and the ring VRF prover.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: BenchmarkHelper<Self::AccountId, Self::RingVrf>;
	}

	/// The calls of indiv-pallet-nft-credits that this pallet dispatches over XCM.
	///
	/// The variant's index and its field order must mirror the dispatchable on the game chain.
	#[derive(Encode)]
	pub(crate) enum NftCreditsCall<T: Config> {
		#[codec(index = 20)]
		ReceiveTreeDeletions { blocks: BoundedVec<AwardBlock, T::MaxTreeDeletionsPerMessage> },
	}

	/// The Merkle commitment to the NFT claim credits awarded in one People-chain block, keyed
	/// by that block.
	#[pallet::storage]
	pub type CreditTrees<T: Config> =
		StorageMap<_, Twox64Concat, AwardBlock, NftClaimCreditTree, OptionQuery>;

	/// The sequence number of the next tree expected from the game pallet's live stream.
	///
	/// A batch starting above it means the trees in between were lost on the way.
	#[pallet::storage]
	pub type NextExpectedSequence<T: Config> = StorageValue<_, TreeSequence, ValueQuery>;

	/// The byte length of one award block's [`ClaimedLeaves`] bitmap, one bit per leaf.
	pub struct ClaimedLeafBytes<T>(core::marker::PhantomData<T>);

	impl<T: Config> Get<u32> for ClaimedLeafBytes<T> {
		fn get() -> u32 {
			T::MaxCreditsPerAwardBlock::get().div_ceil(8)
		}
	}

	/// Which of an award block's leaves have been claimed, bit `leaf_index` per leaf, least
	/// significant bit first.
	///
	/// A proof binds a leaf to its index, so the index names the claim as the leaf itself does.
	/// The bitmap outlives the tree, because a replay on the game chain can deliver the tree again
	/// until its deadline; the sweep of the block's expiry entry is what removes it.
	#[pallet::storage]
	pub type ClaimedLeaves<T: Config> =
		StorageMap<_, Twox64Concat, AwardBlock, BoundedVec<u8, ClaimedLeafBytes<T>>, ValueQuery>;

	/// The ring VRF proof a private claim carries.
	pub type RingProofOf<T> = <<T as Config>::RingVrf as GenerateVerifiable>::Proof;

	/// A game's private claim ring as this chain holds it.
	pub type PrivateRingOf<T> =
		PrivateRing<<<T as Config>::RingVrf as GenerateVerifiable>::Members, BlockNumberFor<T>>;

	/// A batch of private claim rings as the game chain sends it.
	pub type PrivateRingBatchOf<T> = PrivateRingBatch<
		<<T as Config>::RingVrf as GenerateVerifiable>::Members,
		<T as Config>::MaxPrivateRingsPerMessage,
	>;

	/// The private claim ring of each private game, keyed by game.
	///
	/// A ring arrives once and never changes, so a proof built against it stays valid. A game that
	/// too few claimants registered for has no ring, because the game chain builds none below its
	/// anonymity floor, and none of its claims can be made. The entry carries the window its
	/// claims are taken in and is dropped by [`Pallet::close_private_ring`] once that window is
	/// closed.
	#[pallet::storage]
	pub type PrivateRings<T: Config> =
		StorageMap<_, Twox64Concat, GameIdx, PrivateRingOf<T>, OptionQuery>;

	/// The private games that were abandoned, whose credits mint over the public path.
	///
	/// The game chain builds no ring for a game too few claimants registered for, and none for a
	/// game whose ring failed to build. It says so, and this is what reopens [`Pallet::claim`]
	/// for the game's trees. No private claim of such a game exists, there being no ring to prove
	/// against, so no credit mints twice.
	#[pallet::storage]
	pub type AbandonedPrivateGames<T: Config> =
		StorageMap<_, Twox64Concat, GameIdx, (), OptionQuery>;

	/// The aliases already spent in a game's private claims.
	///
	/// One member yields one alias per slot context, which makes the alias the nullifier: it says
	/// a claim was made without saying by whom. An entry is removed only with its game's ring,
	/// once the claim window is closed: while a claim can still be made, dropping one would mint
	/// a second NFT from the same slot.
	#[pallet::storage]
	pub type SpentPrivateClaims<T: Config> =
		StorageDoubleMap<_, Twox64Concat, GameIdx, Identity, Alias, (), OptionQuery>;

	/// The award blocks whose stored tree belongs to a private game, keyed by that game.
	///
	/// A tree carries its game but is keyed by its award block, so this is the only way to find a
	/// game's trees when its window closes. Every removal of a tree that names slots removes its
	/// entry too, so an entry always names a stored tree.
	#[pallet::storage]
	pub type PrivateGameTrees<T: Config> =
		StorageDoubleMap<_, Twox64Concat, GameIdx, Twox64Concat, AwardBlock, (), OptionQuery>;

	/// Every private game holding a ring, filed under the block its claim window closes in.
	///
	/// The key is hashed with `Identity` and encoded big-endian, so the map iterates from the
	/// earliest close to the latest, which [`PrivateRings`] does not. The offchain worker reads
	/// the first entry to find a game to close and never decodes a ring whose window is open. An
	/// entry is filed with the ring and removed with it.
	#[pallet::storage]
	pub type PrivateRingCloses<T: Config> =
		StorageDoubleMap<_, Identity, BigEndianU64, Twox64Concat, GameIdx, (), OptionQuery>;

	/// The private games whose claim window is closed and whose ring is dropped.
	///
	/// It is what stops a redelivered ring reopening a game: the aliases spent against the
	/// dropped ring are gone with it, so a second ring would mint every slot of the game again.
	/// One marker per game replaces a whole ring and its aliases, so closing still reclaims the
	/// space it costs.
	#[pallet::storage]
	pub type ClosedPrivateGames<T: Config> = StorageMap<_, Twox64Concat, GameIdx, (), OptionQuery>;

	/// How many private claims the current block has executed, against
	/// [`Config::MaxPrivateClaimsPerBlock`]. Reset at the start of every block.
	#[pallet::storage]
	pub type PrivateClaimsThisBlock<T: Config> = StorageValue<_, u32, ValueQuery>;

	/// The collections whose owners accept claims, each bound to the registering owner and the
	/// [`ItemSelection`] deciding the item. A collection with no entry cannot be claimed into.
	#[pallet::storage]
	pub type CollectionMinters<T: Config> =
		StorageMap<_, Twox64Concat, CollectionId, CollectionMinter<T::AccountId>, OptionQuery>;

	/// Every award block a sweep still has to reach, filed under the timestamp its tree commits to.
	///
	/// The key is hashed with `Identity` and encoded big-endian, so the map iterates from the
	/// oldest deadline to the newest, which [`CreditTrees`] does not. A sweep takes the trees that
	/// are due and stops at the first that is not. Only a sweep removes an entry. A fully claimed
	/// tree leaves its entry behind, and the sweep of that entry removes its bitmap.
	#[pallet::storage]
	pub type TreeExpiries<T: Config> =
		StorageDoubleMap<_, Identity, ExpiryTimestamp, Twox64Concat, AwardBlock, (), OptionQuery>;

	/// The award blocks whose deletion the game chain has not been told about yet, in the order
	/// this chain removed them.
	///
	/// Both removal paths add to it, and [`Pallet::send_tree_deletions`] drains it from the front.
	/// A block that does not fit is dropped, because the deletion only saves the game chain from
	/// waiting out its own TTL; that TTL is what removes its copy.
	#[pallet::storage]
	pub type PendingTreeDeletions<T: Config> =
		StorageValue<_, BoundedVec<AwardBlock, T::MaxQueuedTreeDeletions>, ValueQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Credit trees were received and stored.
		CreditTreesReceived { count: u32, stored: u32 },
		/// Trees of the live stream never arrived. The game pallet's `CreditTreesSent` events
		/// resolve these sequences to the award blocks they were delivered under, and a
		/// `replay_credit_trees` naming those blocks recovers the trees.
		///
		/// A sequence that resolves to no block is one the game pallet spent on a tree it had
		/// already dropped the root of, and no replay brings it back.
		CreditTreesMissing { from_sequence: TreeSequence, to_sequence: TreeSequence },
		/// A tree was received for a block that already holds a different root. The stored
		/// root is kept, so the proofs built against it stay valid.
		CreditTreeConflict { block: AwardBlock },
		/// A credit awarded in `block` was claimed, minting `instance` of `collection`'s `item`
		/// to the purse key `owner`.
		CreditClaimed {
			block: AwardBlock,
			leaf: NftClaimCreditLeaf,
			collection: CollectionId,
			item: ItemIndex,
			owner: T::AccountId,
			instance: InstanceId,
		},
		/// Every credit committed to by `block`'s tree has been claimed. This chain removed the
		/// tree and queued its deletion for the game chain.
		TreeFullyClaimed { block: AwardBlock },
		/// `collection`'s owner registered it for claims with `selection`, or withdrew it with
		/// `None`.
		CollectionMinterSet { collection: CollectionId, selection: Option<ItemSelection> },
		/// `count` trees outlived [`Config::TreeTtl`], and this chain removed them with credits
		/// left unclaimed. Nothing can mint those credits again, on this chain or any other.
		///
		/// A private game whose ring arrived has its trees removed at its close and reports
		/// [`Event::PrivateGameTreesRemoved`] instead. A private game counted here was abandoned,
		/// or reached no outcome at all, in which case no claimant could have minted its credits.
		///
		/// The blocks are not named here. The same trees travel in a deletion message, and the
		/// [`Event::TreeDeletionsSent`] carrying them names every one.
		CreditTreesExpired { count: u32 },
		/// A tree arrived for `block` past its deadline, so this chain did not store it. Its
		/// credits were already unmintable when the delivery arrived.
		CreditTreeStale { block: AwardBlock },
		/// Award blocks whose deletion was handed to the XCM router for the game chain.
		TreeDeletionsSent { blocks: BoundedVec<AwardBlock, T::MaxTreeDeletionsPerMessage> },
		/// Delivery of the deletions to the game chain failed. The blocks stay queued, and the
		/// next offchain worker cycle retries them.
		TreeDeletionSendFailed,
		/// [`PendingTreeDeletions`] is full, so this chain dropped the deletions of `blocks`.
		/// Delivery has failed for [`Config::MaxQueuedTreeDeletions`] trees. The game chain
		/// removes its own copies when its TTL runs out.
		TreeDeletionsDropped { blocks: BoundedVec<AwardBlock, T::MaxTreeDeletionsPerMessage> },
		/// A tree arrived for `block` committing to more leaves than
		/// [`Config::MaxCreditsPerAwardBlock`], so this chain did not store it. None of its
		/// credits can be claimed here until that bound covers the game chain's own.
		CreditTreeOversized { block: AwardBlock },
		/// A private claim ring arrived for `game_index`. `key_count` is the anonymity set each
		/// of its claims hides in, and `slots` is how many claims each member of the ring may
		/// make.
		///
		/// Claims are taken from `opens_at` until `closes_at`, and a member who misses that
		/// window mints nothing. A wallet picks its moment inside it at random: claims spread
		/// over the window cover each other.
		PrivateRingReceived {
			game_index: GameIdx,
			slots: PrivateClaimSlot,
			key_count: u32,
			opens_at: BlockNumberFor<T>,
			closes_at: BlockNumberFor<T>,
		},
		/// `game_index`'s ring and the aliases spent against it are dropped, its claim window
		/// having closed. No claim of the game is taken from now on, and none was taken since
		/// the window closed.
		PrivateRingClosed { game_index: GameIdx },
		/// `count` trees of `game_index` were removed as its claim window closed, and their
		/// deletion was queued for the game chain. Their credits minted through the game's ring
		/// or not at all: a public claim of them was never possible.
		///
		/// The blocks are not named here. The same trees travel in a deletion message, and the
		/// [`Event::TreeDeletionsSent`] carrying them names every one.
		PrivateGameTreesRemoved { game_index: GameIdx, count: u32 },
		/// A tree arrived for `block` whose private game is closed, so this chain did not store
		/// it. The game's ring and the aliases spent against it are gone, so the tree would take
		/// a claim on neither path.
		CreditTreePrivateGameClosed { block: AwardBlock },
		/// `game_index` built no ring, so its credits mint over the public path from now on.
		/// `key_count` is how many claimants had registered for the ring it did not build.
		PrivateGameAbandoned { game_index: GameIdx, key_count: u32 },
		/// A second, different outcome arrived for a game: another ring, or an abandonment of a
		/// game that holds one. The stored outcome is kept, because claims may already rest on
		/// it.
		PrivateOutcomeConflict { game_index: GameIdx },
		/// A private claim of `game_index` spent `slot`, minting `instance` of `collection`'s
		/// `item` to the purse key `owner`.
		///
		/// The claimant is left out. The alias the claim spent is in storage, and says only that
		/// some member of the ring claimed.
		PrivateCreditClaimed {
			game_index: GameIdx,
			slot: PrivateClaimSlot,
			collection: CollectionId,
			item: ItemIndex,
			owner: T::AccountId,
			instance: InstanceId,
		},
	}

	#[pallet::error]
	pub enum Error<T> {
		/// No tree is held for the award block, so nothing can be proven against it. The tree may
		/// still be on its way, or have been lost, in which case a `replay_credit_trees` on the
		/// game pallet delivers it.
		UnknownAwardBlock,
		/// The leaf index is not one of the tree's leaves.
		LeafIndexOutOfBounds,
		/// The credit has already been claimed and mints one NFT only.
		AlreadyClaimed,
		/// The proof does not rehash the credit's leaf to the tree's root, so the origin holds no
		/// such credit in that block.
		InvalidProof,
		/// The collection does not exist in Scarcity.
		UnknownCollection,
		/// Only the collection's owner may register or withdraw its minter.
		NotCollectionOwner,
		/// The collection's owner has not registered it for claims, so no claim can mint into
		/// it.
		CollectionNotRegistered,
		/// The collection has changed owners since registration, so its current owner must
		/// register it again.
		CollectionOwnerChanged,
		/// The collection has no item definitions for [`ItemSelection::Random`] to draw from.
		NoItems,
		/// The block's game mints through its private claim ring, so it takes no public claim.
		/// A game that built no ring is the exception: its abandonment reopens this path.
		PrivateGame,
		/// No ring is held for the game, so it has nothing to close. The ring may not have
		/// arrived yet, or it may be closed and dropped already.
		UnknownPrivateRing,
		/// The game's claim window is still open, so its ring is what its claims are proven
		/// against and its spent aliases are what stops a second mint.
		PrivateClaimWindowOpen,
	}

	/// Why a `claim_private` submission, or one of this pallet's offchain worker submissions, is
	/// not valid.
	///
	/// Reported as [`InvalidTransaction::Custom`], so a caller can tell the causes apart. The
	/// block's allowance is the exception and reports [`InvalidTransaction::Future`]: the claim is
	/// valid and only waits for a block with room, so the pool keeps it.
	#[repr(u8)]
	pub enum AuthorizeInvalidity {
		/// No ring is held for the game the claim names.
		UnknownPrivateRing = 200,
		/// The slot is not one the game grants.
		SlotOutOfRange = 201,
		/// The runtime's ring exponent is not one the crypto accepts.
		InvalidRingExponent = 202,
		/// The proof does not verify against the game's ring, or yields another alias than the
		/// one the call names.
		InvalidRingProof = 203,
		/// The alias is spent, so the slot behind it has already minted.
		SlotAlreadyClaimed = 204,
		/// The game's claim window is closed, so it takes no further claim. A claim before the
		/// window opens is not this: it reports [`InvalidTransaction::Future`] and waits.
		PrivateClaimWindowClosed = 205,
		/// Transaction source is not local or in block.
		TransactionNotLocal = 210,
		/// No tree is filed for expiry, so there is nothing to sweep.
		NothingToSweep = 211,
		/// No tree deletion is waiting to be sent to the game chain.
		NoQueuedTreeDeletions = 212,
		/// No ring is held for the game, so it has nothing left to close. An open window is a
		/// separate case and reports [`InvalidTransaction::Future`] instead, which waits.
		NoPrivateRingToClose = 213,
	}

	impl From<AuthorizeInvalidity> for TransactionValidityError {
		fn from(e: AuthorizeInvalidity) -> Self {
			InvalidTransaction::Custom(e as u8).into()
		}
	}

	#[pallet::call(weight = <T as Config>::WeightInfo)]
	impl<T: Config> Pallet<T> {
		/// Stores the credit trees of a batch sent by the game pallet.
		///
		/// ## Origin
		/// Requires the game chain's XCM origin (`EnsureGameChainOrigin`).
		///
		/// This does not store a tree past its deadline, nor one for a block whose tree it holds
		/// already.
		///
		/// ## Parameters
		/// - `batch`: The credit trees to store, in ascending block order.
		#[pallet::call_index(0)]
		#[pallet::weight(T::WeightInfo::receive_credit_trees(batch.trees.len() as u32))]
		pub fn receive_credit_trees(
			origin: OriginFor<T>,
			batch: CreditTreeBatch<T>,
		) -> DispatchResult {
			T::EnsureGameChainOrigin::ensure_origin(origin)?;

			let count = batch.trees.len() as u32;
			let mut stored = 0u32;
			let now = T::UnixTime::now().as_secs();

			for update in batch.trees.iter() {
				if update.tree.leaf_count == 0 || update.tree.root.0 == [0u8; 32] {
					// The game pallet only commits blocks that awarded at least one credit and
					// a Blake2 root of real leaves is never zero, so neither can be genuine. An
					// empty tree would be unclaimable and a zero root is not a commitment.
					log::error!(
						target: LOG_TARGET,
						"Invalid credit tree for block {}: root {:?}, leaf count {}",
						update.block,
						update.tree.root,
						update.tree.leaf_count,
					);
					continue;
				}

				if update.tree.leaf_count > T::MaxCreditsPerAwardBlock::get() {
					// `ClaimedLeaves` holds one bit per leaf up to this bound, so the leaves past
					// it could never be spent and would mint again and again. The bound is the
					// game chain's own, so a tree over it means the two runtimes disagree.
					log::error!(
						target: LOG_TARGET,
						"Oversized credit tree for block {}: {} leaves against a bound of {}",
						update.block,
						update.tree.leaf_count,
						T::MaxCreditsPerAwardBlock::get(),
					);
					Self::deposit_event(Event::CreditTreeOversized { block: update.block });
					continue;
				}

				// Drop a tree past its deadline instead of storing it for a later sweep. The game
				// chain holds its root for longer than this chain holds the tree, and anyone
				// can call `replay_credit_trees` there, so storing it makes its credits
				// mintable again until the sweep reaches them.
				if Self::tree_has_expired(update.tree.timestamp, now) {
					Self::deposit_event(Event::CreditTreeStale { block: update.block });
					continue;
				}

				// A closed private game built a ring, so `claim` refuses this tree for good and a
				// private claim proves against the ring, which is gone. Storing it would leave
				// state that only the sweep removes.
				if update.tree.private_slots != 0 &&
					ClosedPrivateGames::<T>::contains_key(update.tree.game_index)
				{
					Self::deposit_event(Event::CreditTreePrivateGameClosed { block: update.block });
					continue;
				}

				if let Some(existing) = CreditTrees::<T>::get(update.block) {
					if existing != update.tree {
						// A block's credits are committed once and the root never changes
						// afterwards, so two roots for one block mean the chains disagree about
						// what that block awarded.
						log::error!(
							target: LOG_TARGET,
							"Conflicting credit tree for block {}: kept {:?}, ignored {:?}",
							update.block,
							existing.root,
							update.tree.root,
						);
						Self::deposit_event(Event::CreditTreeConflict { block: update.block });
					}
				} else {
					CreditTrees::<T>::insert(update.block, update.tree);
					Self::note_tree_expiry(update.block, update.tree.timestamp);
					if update.tree.private_slots != 0 {
						// The close removes a game's trees, and the tree is keyed by its award
						// block, so the game needs an index of its own to find them by.
						PrivateGameTrees::<T>::insert(update.tree.game_index, update.block, ());
					}
					stored = stored.saturating_add(1);
				}
			}

			Self::note_sequences(&batch);

			Self::deposit_event(Event::CreditTreesReceived { count, stored });

			Ok(())
		}

		/// Mints the NFT of one NFT claim credit the game chain awarded in `block`.
		///
		/// The credit is spent by the claim: its leaf is recorded, and a second claim of the same
		/// credit fails, whoever submits it.
		///
		/// ## Origin
		/// The signer of the claimant the credit was awarded to ([`Config::EnsureClaimant`]).
		///
		/// ## Parameters
		/// - `claimant`: Which of the signer's identities the credit was awarded to. A person
		///   claims as [`ClaimantKind::Person`], which resolves to the alias their account is bound
		///   to.
		/// - `block`: The People-chain block the credit was awarded in, which names the tree the
		///   proof is verified against.
		/// - `credit`: The credit being claimed. Hashed together with the origin's identity into
		///   the leaf, so a credit of somebody else's rehashes to a leaf that is in no tree.
		/// - `leaf_index`: The position of that leaf in the block's leaves, in award order.
		/// - `proof`: The sibling hashes that rehash the leaf up to the tree's root, bottom layer
		///   first, as the game chain's `nft_claim_credit_proofs` returns them.
		/// - `collection`: The Scarcity collection the NFT is minted into, which has to be
		///   registered through [`Pallet::set_collection_minter`]. Its [`ItemSelection`] decides
		///   the item.
		/// - `mint_to`: The Scarcity purse key the NFT is minted to. A purse key holds one NFT, so
		///   this has to be an empty one, and holders are meant to use a fresh key they control
		///   rather than an account that already holds something.
		///
		/// The claim that spends the tree's last credit also removes the tree and queues its
		/// deletion for the game chain, and the call is charged for that. The claims that came
		/// before decide which claim that is, not the call's arguments, so a claim that leaves
		/// credits behind is refunded down to a plain claim of the kind the call names. The charge
		/// also reserves [`CollectionSelector::max_weight`] whatever the collection's selection is,
		/// refunded down to what the selection consumed, including on the error path.
		#[pallet::call_index(1)]
		// Resolving a person claimant reads the signer's alias binding, which an account
		// claimant does not, so the kind the call names picks the weight.
		#[pallet::weight(
			match claimant {
				ClaimantKind::Account => T::WeightInfo::claim_last_account(proof.len() as u32),
				ClaimantKind::Person => T::WeightInfo::claim_last_person(proof.len() as u32),
			}
			.saturating_add(T::CollectionSelector::max_weight())
			.saturating_add(T::Nfts::mint_hook_weight(CLAIM_METADATA_PAIRS))
		)]
		pub fn claim(
			origin: OriginFor<T>,
			claimant: ClaimantKind,
			block: AwardBlock,
			credit: NftClaimCredit,
			leaf_index: u32,
			proof: BoundedVec<CreditProofNode, T::MaxProofNodes>,
			collection: CollectionId,
			mint_to: T::AccountId,
		) -> DispatchResultWithPostInfo {
			// Every failure carries `actual_weight`, which refunds the selector ceiling and the
			// tree removal on the error path. A failed claim charges a plain claim's weight plus
			// what the failed contract selection consumed, not the whole reservation.
			let (base, base_last) = match claimant {
				ClaimantKind::Account => (
					T::WeightInfo::claim_account(proof.len() as u32),
					T::WeightInfo::claim_last_account(proof.len() as u32),
				),
				ClaimantKind::Person => (
					T::WeightInfo::claim_person(proof.len() as u32),
					T::WeightInfo::claim_last_person(proof.len() as u32),
				),
			};
			let claimant = T::EnsureClaimant::ensure_origin(origin, &claimant)
				.map_err(|e| e.with_weight(base))?;

			let tree = CreditTrees::<T>::get(block)
				.ok_or(Error::<T>::UnknownAwardBlock.with_weight(base))?;
			// A private game mints through its ring only. The tree carries the slot count, so a
			// public claim is refused before the game's ring arrives. A game that built no ring
			// mints here instead, its abandonment being what says so.
			ensure!(
				tree.private_slots == 0 ||
					AbandonedPrivateGames::<T>::contains_key(tree.game_index),
				Error::<T>::PrivateGame.with_weight(base)
			);
			ensure!(
				leaf_index < tree.leaf_count,
				Error::<T>::LeafIndexOutOfBounds.with_weight(base)
			);

			let leaf = credit_leaf(&claimant, &credit);
			ensure!(
				!Self::leaf_is_claimed(&ClaimedLeaves::<T>::get(block), leaf_index),
				Error::<T>::AlreadyClaimed.with_weight(base)
			);

			// The root and the leaf count are the stored ones, never the claimant's: the count
			// decides how an odd layer was rehashed, so a caller-supplied one would select which
			// path is verified.
			ensure!(
				binary_merkle_tree::verify_proof::<BlakeTwo256, _, _>(
					&H256::from(tree.root),
					proof.iter().map(|node| H256::from(*node)),
					tree.leaf_count,
					leaf_index,
					&leaf,
				),
				Error::<T>::InvalidProof.with_weight(base)
			);

			// Spent before the selection so that a minter contract reentering with the same
			// credit fails `AlreadyClaimed`. A failure anywhere below unwinds the whole
			// dispatch, the bit included.
			Self::spend_leaf(block, leaf_index, tree.leaf_count)
				.map_err(|()| Error::<T>::LeafIndexOutOfBounds.with_weight(base))?;

			let selection = Self::select_item(collection, credit).map_err(|error| {
				let error = error.into_claim_error::<T>();
				error.error.with_weight(base.saturating_add(error.weight_consumed))
			})?;
			let SelectedItem { item, weight_consumed: selection_weight, .. } = selection;
			let instance =
				T::Nfts::mint_without_deposit(collection, item, mint_to.clone(), Vec::new())
					.map_err(|e| e.with_weight(base.saturating_add(selection_weight)))?;

			// Counted after the selection: a contract may reenter with another credit of the
			// same block, and counting from a snapshot taken before it would drop that claim's
			// bit.
			let claimed = Self::claimed_leaf_count(&ClaimedLeaves::<T>::get(block));

			Self::deposit_event(Event::CreditClaimed {
				block,
				leaf,
				collection,
				item,
				owner: mint_to,
				instance,
			});

			// Both success paths run the mint and its runtime hooks, so both pay for them. Every
			// failure above returns before the mint.
			if claimed < tree.leaf_count {
				return Ok(Some(
					base.saturating_add(selection_weight)
						.saturating_add(T::Nfts::mint_hook_weight(CLAIM_METADATA_PAIRS)),
				)
				.into());
			}

			// No proof can be built against a fully claimed tree again, so remove it and tell the
			// game chain to drop its root. The spent leaves stay, because a replay there can
			// deliver the tree again before the deletion arrives, and only those leaves keep its
			// credits spent.
			Self::remove_tree(block, &tree);
			Self::deposit_event(Event::TreeFullyClaimed { block });

			Ok(Some(
				base_last
					.saturating_add(selection_weight)
					.saturating_add(T::Nfts::mint_hook_weight(CLAIM_METADATA_PAIRS)),
			)
			.into())
		}

		/// Stores the private game outcomes of a batch sent by the game pallet.
		///
		/// An outcome is a ring, which opens the private path for the game, or an abandonment,
		/// which reopens the public one. A game already holding one outcome keeps it.
		///
		/// ## Origin
		/// Requires the game chain's XCM origin (`EnsureGameChainOrigin`).
		///
		/// ## Parameters
		/// - `batch`: The outcomes to store, in ascending game order.
		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::receive_private_rings(batch.rings.len() as u32))]
		pub fn receive_private_rings(
			origin: OriginFor<T>,
			batch: PrivateRingBatchOf<T>,
		) -> DispatchResult {
			T::EnsureGameChainOrigin::ensure_origin(origin)?;

			for update in batch.rings.iter() {
				Self::store_private_outcome(update);
			}

			Ok(())
		}

		/// Mints an NFT against a private game's ring, without naming the claimant.
		///
		/// The proof shows that its maker holds a key of `game_index`'s ring and yields `alias`,
		/// the alias of `slot`'s context. The claim spends that alias. One key yields one alias
		/// per slot, so a claimant mints once per slot the game grants, and no two of those mints
		/// can be tied to each other.
		///
		/// ## Origin
		/// Authorized: the proof authorizes the call, so the transaction carries no signer and
		/// pays no fee. A signed origin would name the account that funds the mint, and the claims
		/// one account paid for could be intersected to narrow their maker down inside the ring.
		///
		/// ## Parameters
		/// - `game_index`: The private game the credit was earned in.
		/// - `slot`: Which of the game's slots is being spent, which picks the proof's context.
		///   Every member may name any of them, so spend them in a random order: a claimant who
		///   walks the slots in order leaves a pattern that ties their own claims together.
		/// - `alias`: The alias the proof yields. `authorize` checks the proof against it and the
		///   dispatch spends it, so the ring verification runs once per claim instead of once in
		///   `authorize` and again here.
		/// - `proof`: The ring VRF proof, made under the context of `game_index` and `slot`, over
		///   the message this call builds from `collection` and `mint_to`.
		/// - `collection`: The Scarcity collection the NFT is minted into. The proof commits to it.
		/// - `mint_to`: The Scarcity purse key the NFT is minted to. The proof commits to it too,
		///   so an observed proof cannot be replayed into another purse or another collection.
		#[pallet::authorize(|source, game_index, slot, alias, proof, collection, mint_to| {
			Self::authorize_claim_private(source, game_index, slot, alias, proof, collection,
				mint_to)
		})]
		#[pallet::call_index(6)]
		#[pallet::weight(
			T::WeightInfo::claim_private()
				.saturating_add(T::CollectionSelector::max_weight())
				.saturating_add(T::Nfts::mint_hook_weight(CLAIM_METADATA_PAIRS))
		)]
		#[pallet::weight_of_authorize(T::WeightInfo::authorize_claim_private())]
		pub fn claim_private(
			origin: OriginFor<T>,
			game_index: GameIdx,
			slot: PrivateClaimSlot,
			alias: Alias,
			proof: RingProofOf<T>,
			collection: CollectionId,
			mint_to: T::AccountId,
		) -> DispatchResultWithPostInfo {
			let base = T::WeightInfo::claim_private();
			ensure_authorized(origin).map_err(|e| e.with_weight(base))?;

			// `authorize` ran on this state in the same block. It verified the proof against the
			// game's ring, matched `alias` to it, held the claim to the block's allowance and
			// found the alias unspent. Only spending the alias is left.
			let _ = proof;

			// Spent before the selection, so that a minter contract that reenters with the same
			// proof finds the alias gone. A failure below unwinds the whole dispatch.
			SpentPrivateClaims::<T>::insert(game_index, alias, ());
			PrivateClaimsThisBlock::<T>::mutate(|executed| *executed = executed.saturating_add(1));

			// A private claim spends its credit on the game chain, at registration, so there is
			// no credit here to draw the item from. The alias stands in for it.
			let selection = Self::select_item(collection, alias).map_err(|error| {
				let error = error.into_claim_error::<T>();
				error.error.with_weight(base.saturating_add(error.weight_consumed))
			})?;
			let SelectedItem { item, weight_consumed: selection_weight, .. } = selection;
			let instance =
				T::Nfts::mint_without_deposit(collection, item, mint_to.clone(), Vec::new())
					.map_err(|e| e.with_weight(base.saturating_add(selection_weight)))?;

			Self::deposit_event(Event::PrivateCreditClaimed {
				game_index,
				slot,
				collection,
				item,
				owner: mint_to,
				instance,
			});

			Ok(Some(
				base.saturating_add(selection_weight)
					.saturating_add(T::Nfts::mint_hook_weight(CLAIM_METADATA_PAIRS)),
			)
			.into())
		}

		/// Registers `collection` for claims with `selection` deciding the minted item, or
		/// withdraws it with `None`.
		///
		/// Registration is the owner's opt-in to deposit-free supply growth: without it no claim
		/// can mint into the collection. Withdrawing stops further claims and spends nothing
		/// already claimed. Deleting the collection clears its registration through
		/// [`indiv_pallet_scarcity::OnCollectionDeleted`], so an unknown collection can be neither
		/// registered nor withdrawn. A contract selection is validated through
		/// [`CollectionSelector::validate`], so an address with no code fails here rather than on
		/// the first claim.
		///
		/// ## Origin
		/// The collection's Scarcity owner.
		///
		/// ## Parameters
		/// - `collection`: The Scarcity collection to register or withdraw.
		/// - `selection`: How claims pick the item to mint, or `None` to withdraw.
		#[pallet::call_index(2)]
		pub fn set_collection_minter(
			origin: OriginFor<T>,
			collection: CollectionId,
			selection: Option<ItemSelection>,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let owner =
				T::Nfts::collection_owner(collection).ok_or(Error::<T>::UnknownCollection)?;
			ensure!(who == owner, Error::<T>::NotCollectionOwner);
			match selection {
				Some(selection) => {
					if let ItemSelection::Contract(contract) = selection {
						T::CollectionSelector::validate(contract)?;
					}
					CollectionMinters::<T>::insert(
						collection,
						CollectionMinter { owner, selection },
					);
				},
				None => CollectionMinters::<T>::remove(collection),
			}
			Self::deposit_event(Event::CollectionMinterSet { collection, selection });
			Ok(())
		}

		/// Removes the trees whose deadline has passed, oldest first, and queues their deletion
		/// for the game chain.
		///
		/// This pallet's offchain worker submits this authorized call. It is accepted from a local
		/// or in-block source only, so no external submission reaches it.
		///
		/// `oldest` must be the timestamp [`TreeExpiries`] holds its oldest entry under, which
		/// makes a retry that raced a successful sweep stale instead of a second pass. One call
		/// removes at most [`Config::MaxTreeDeletionsPerMessage`] trees, so clearing more trees
		/// than that takes several blocks.
		#[pallet::call_index(3)]
		#[pallet::authorize(|source, oldest, _discriminator| {
			Self::authorize_sweep_expired_trees(source, oldest)
		})]
		#[pallet::weight(T::WeightInfo::sweep_expired_trees(T::MaxTreeDeletionsPerMessage::get()))]
		#[pallet::weight_of_authorize(T::WeightInfo::authorize_sweep_expired_trees())]
		pub fn sweep_expired_trees(
			origin: OriginFor<T>,
			_oldest: u32,
			// The submitting block, which gives each block's sweep a transaction hash of its own.
			// See `Pallet::submit_expiry_sweep`.
			_discriminator: BlockNumberFor<T>,
		) -> DispatchResultWithPostInfo {
			ensure_authorized(origin)?;

			Ok(Self::do_sweep_expired_trees())
		}

		/// Tells the game chain about the queued tree deletions that fit one XCM message.
		///
		/// This pallet's offchain worker submits this authorized call. It is accepted from a local
		/// or in-block source only, so no external submission reaches it.
		///
		/// `front` must be the block at the front of [`PendingTreeDeletions`], which a successful
		/// send replaces. A retry that raced that send is stale instead of a second send. The next
		/// batch's send names a new front, so it carries a transaction hash of its own.
		#[pallet::call_index(4)]
		#[pallet::authorize(|source, front, _discriminator| {
			Self::authorize_send_tree_deletions(source, front)
		})]
		#[pallet::weight(T::WeightInfo::send_tree_deletions(T::MaxTreeDeletionsPerMessage::get()))]
		#[pallet::weight_of_authorize(T::WeightInfo::authorize_send_tree_deletions())]
		pub fn send_tree_deletions(
			origin: OriginFor<T>,
			_front: AwardBlock,
			// Per-window discriminator. A stalled retry of one front gets a fresh transaction
			// hash once the window changes. See `indiv_support::offchain`.
			_discriminator: BlockNumberFor<T>,
		) -> DispatchResultWithPostInfo {
			ensure_authorized(origin)?;

			Ok(Self::do_send_tree_deletions())
		}

		/// Drops a private game's ring, the aliases spent against it and its credit trees, once
		/// its claim window is closed.
		///
		/// One call removes at most [`PRIVATE_CLOSE_ITEMS`] aliases and one deletion message's
		/// worth of trees, and refunds the rest, so a game is dropped over as many calls as it
		/// takes. The ring goes last, because it holds the window that says the removal is
		/// allowed. Nothing removed here can gate a claim: a closed window takes none, and
		/// [`Pallet::claim`] refuses a tree that names slots.
		///
		/// This pallet's offchain worker submits this authorized call. It is accepted from a local
		/// or in-block source only, so no external submission reaches it.
		///
		/// ## Origin
		/// Authorized: the game's closed window authorizes the call.
		///
		/// ## Parameters
		/// - `game_index`: The private game whose ring is dropped.
		#[pallet::call_index(7)]
		#[pallet::authorize(|source, game_index, _discriminator| {
			Self::authorize_close_private_ring(source, game_index)
		})]
		#[pallet::weight(T::WeightInfo::close_private_ring(
			PRIVATE_CLOSE_ITEMS,
			T::MaxTreeDeletionsPerMessage::get()
		))]
		#[pallet::weight_of_authorize(T::WeightInfo::authorize_close_private_ring())]
		pub fn close_private_ring(
			origin: OriginFor<T>,
			game_index: GameIdx,
			// The submitting block, which gives each block's step a transaction hash of its own.
			// A game takes as many steps as its state needs, and `game_index` alone cannot tell
			// them apart. See `Pallet::submit_private_ring_close`.
			_discriminator: BlockNumberFor<T>,
		) -> DispatchResultWithPostInfo {
			ensure_authorized(origin)?;

			let (aliases, trees) = Self::do_close_private_ring(game_index)?;

			Ok(Some(T::WeightInfo::close_private_ring(aliases, trees)).into())
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		/// Reopens the block's private claim allowance.
		///
		/// The counter is per block and is cleared rather than carried. A carried counter would
		/// close the path for good once one block filled the cap.
		fn on_initialize(_now: BlockNumberFor<T>) -> Weight {
			// Read first. Only a block that ran a private claim leaves a counter to clear, and
			// every other block would pay for the write.
			if PrivateClaimsThisBlock::<T>::exists() {
				PrivateClaimsThisBlock::<T>::kill();
				return T::DbWeight::get().reads_writes(1, 1);
			}

			T::DbWeight::get().reads(1)
		}

		#[cfg(feature = "std")]
		fn integrity_test() {
			assert!(
				T::MaxTreesPerMessage::get() > 0,
				"MaxTreesPerMessage must be greater than zero"
			);

			// `ClaimedLeaves` sizes its bitmap from this, and the benchmarked trees take its
			// base-two logarithm.
			let max_credits = T::MaxCreditsPerAwardBlock::get();
			assert!(max_credits > 0, "MaxCreditsPerAwardBlock must be greater than zero");

			// A tree of `max_credits` leaves needs this many sibling hashes. A lower bound leaves
			// the tail of a full award block unclaimable, because its proof does not decode.
			let max_proof_nodes = max_credits.next_power_of_two().ilog2();
			assert!(
				T::MaxProofNodes::get() >= max_proof_nodes,
				"MaxProofNodes ({}) is below the {max_proof_nodes} sibling hashes a tree of \
				 {max_credits} leaves needs",
				T::MaxProofNodes::get(),
			);

			// An XCM `Transact` or the transaction pool dispatches every call below. A worst case
			// above the block's per-extrinsic limit never executes: a batch fails and loses
			// every tree in it until a replay, and the pool drops a sweep or a send. The budget
			// is the half of `Normal.max_extrinsic` the game pallet holds its sending side to,
			// so both ends of the delivery use one yardstick.
			let budget = OcwWeightBudget::from_normal_max::<T>();
			budget.assert_fits(
				"receive_credit_trees",
				T::WeightInfo::receive_credit_trees(T::MaxTreesPerMessage::get()),
			);

			// A claim reserves the selector's ceiling on top of its own worst case, whether or not
			// the collection uses a contract. A worst case above the limit therefore blocks
			// every claim, not only the contract-selected ones.
			budget.assert_fits(
				"claim",
				T::WeightInfo::claim_last_account(T::MaxProofNodes::get())
					.max(T::WeightInfo::claim_last_person(T::MaxProofNodes::get()))
					.saturating_add(T::CollectionSelector::max_weight())
					.saturating_add(T::Nfts::mint_hook_weight(CLAIM_METADATA_PAIRS)),
			);

			let max_deletions_per_message = T::MaxTreeDeletionsPerMessage::get();
			budget.assert_fits(
				"sweep_expired_trees",
				T::WeightInfo::sweep_expired_trees(max_deletions_per_message)
					.saturating_add(T::WeightInfo::authorize_sweep_expired_trees()),
			);
			budget.assert_fits(
				"send_tree_deletions",
				T::WeightInfo::send_tree_deletions(max_deletions_per_message)
					.saturating_add(T::WeightInfo::authorize_send_tree_deletions()),
			);

			assert!(
				max_deletions_per_message > 0,
				"MaxTreeDeletionsPerMessage must be greater than zero"
			);
			// A sweep removes a message's worth of trees and queues one deletion for each. A
			// narrower queue drops deletions of that sweep's own, and the game chain then waits
			// out its own TTL for trees this chain has already removed.
			assert!(
				max_deletions_per_message <= T::MaxQueuedTreeDeletions::get(),
				"MaxTreeDeletionsPerMessage ({max_deletions_per_message}) exceeds \
				 MaxQueuedTreeDeletions ({queue}), so one sweep can drop its own deletions",
				queue = T::MaxQueuedTreeDeletions::get(),
			);

			// The deletion message has to fit the channel to the game chain. That channel's real
			// per-message room comes from the relay chain's configuration, which is unknown at
			// build time, so the check uses a size no channel between system parachains sits
			// below.
			let message = Self::tree_deletion_message_size(max_deletions_per_message);
			assert!(
				message <= MIN_CHANNEL_MESSAGE_SIZE,
				"a full deletion message is {message} bytes, more than the {MIN_CHANNEL_MESSAGE_SIZE} \
				 bytes a channel is assumed to carry, so `MaxTreeDeletionsPerMessage` is too high",
			);

			// A private claim carries no signer and pays no fee, so the block budget is the only
			// bound on it. Both halves count, and the ring verification in `authorize` is the
			// heavier one.
			budget.assert_fits(
				"claim_private",
				T::WeightInfo::claim_private()
					.saturating_add(T::WeightInfo::authorize_claim_private())
					.saturating_add(T::CollectionSelector::max_weight())
					.saturating_add(T::Nfts::mint_hook_weight(CLAIM_METADATA_PAIRS)),
			);

			// A cap of zero takes no private claim at all.
			assert!(
				!T::MaxPrivateClaimsPerBlock::get().is_zero(),
				"MaxPrivateClaimsPerBlock must be at least one",
			);

			// A window of no blocks closes before the block its claims open in, so every claim
			// of every private game is refused and no credit of one mints on either path.
			assert!(
				!T::PrivateClaimWindow::get().is_zero(),
				"PrivateClaimWindow must be at least one block",
			);

			// The whole worst case is charged before the refund, so a worst case above the limit
			// leaves the ring undroppable and the pool drops every step.
			budget.assert_fits(
				"close_private_ring",
				T::WeightInfo::close_private_ring(PRIVATE_CLOSE_ITEMS, max_deletions_per_message)
					.saturating_add(T::WeightInfo::authorize_close_private_ring()),
			);
		}

		fn offchain_worker(block_number: BlockNumberFor<T>) {
			Self::submit_expiry_sweep(block_number);
			Self::submit_tree_deletions(block_number);
			Self::submit_private_ring_close(block_number);
		}

		#[cfg(feature = "try-runtime")]
		fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
			Self::do_try_state()
		}
	}

	#[cfg(any(test, feature = "try-runtime"))]
	impl<T: Config> Pallet<T> {
		/// Check that the pallet's records agree with each other and with Scarcity: a block whose
		/// tree is still held has no more claimed leaves than the tree has leaves, every held tree
		/// and every claimed-leaf bitmap is filed for expiry, every held private tree is indexed
		/// under its own game, every held ring is filed under the block it closes in, and no
		/// registration outlives the collection it names.
		///
		/// A bitmap outlives the tree it belongs to. A block with claimed leaves and no tree is
		/// therefore the state a fully claimed tree leaves behind, not an inconsistency, and its
		/// expiry entry is what gets the bitmap removed at the deadline.
		pub(crate) fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
			use alloc::collections::BTreeSet;
			use sp_runtime::TryRuntimeError;

			let filed =
				TreeExpiries::<T>::iter().map(|(_, block, ())| block).collect::<BTreeSet<_>>();
			for (block, bitmap) in ClaimedLeaves::<T>::iter() {
				if let Some(tree) = CreditTrees::<T>::get(block) {
					if Self::claimed_leaf_count(&bitmap) > tree.leaf_count {
						return Err(TryRuntimeError::Other(
							"a block has more claimed leaves than its tree has leaves",
						));
					}
				}
				// Only a sweep of an expiry entry removes a bitmap, so a bitmap with no entry is
				// never removed.
				if !filed.contains(&block) {
					return Err(TryRuntimeError::Other("claimed leaves have no expiry entry"));
				}
			}

			// A held tree is filed under the timestamp it commits to, which is where a sweep
			// finds it. The other direction does not hold: a fully claimed tree leaves its entry
			// behind for its bitmap.
			for (timestamp, block, ()) in TreeExpiries::<T>::iter() {
				if let Some(tree) = CreditTrees::<T>::get(block) {
					if tree.timestamp != timestamp.0 {
						return Err(TryRuntimeError::Other(
							"tree is filed under the wrong timestamp",
						));
					}
				}
			}
			for (block, tree) in CreditTrees::<T>::iter() {
				if !TreeExpiries::<T>::contains_key(ExpiryTimestamp::from(tree.timestamp), block) {
					return Err(TryRuntimeError::Other("held tree has no expiry entry"));
				}
			}

			// An alias is spent against a ring and removed with it, so an orphan is a ring that
			// was dropped while its claims could still be made, which mints a slot twice.
			for (game_index, _alias, ()) in SpentPrivateClaims::<T>::iter() {
				if !PrivateRings::<T>::contains_key(game_index) {
					return Err(TryRuntimeError::Other("spent private claim has no ring"));
				}
			}

			// A game reaches one outcome. An abandoned game never held a ring, so it has none to
			// close, and a closed one is a game that did hold one.
			for (game_index, ()) in ClosedPrivateGames::<T>::iter() {
				if AbandonedPrivateGames::<T>::contains_key(game_index) {
					return Err(TryRuntimeError::Other(
						"a private game is both abandoned and closed",
					));
				}
				if PrivateRings::<T>::contains_key(game_index) {
					return Err(TryRuntimeError::Other("a closed private game still holds a ring"));
				}
				if PrivateGameTrees::<T>::iter_key_prefix(game_index).next().is_some() {
					return Err(TryRuntimeError::Other("a closed private game still holds trees"));
				}
			}

			// The offchain worker finds a game to close through this index, so a ring that is not
			// filed under its own closing block is never closed, and an entry naming no ring
			// leaves the worker submitting a close that `authorize` refuses.
			for (game_index, ring) in PrivateRings::<T>::iter() {
				if !PrivateRingCloses::<T>::contains_key(
					Self::close_key(ring.closes_at),
					game_index,
				) {
					return Err(TryRuntimeError::Other("a private ring is not filed for closing"));
				}
			}
			for (closes_at, game_index, ()) in PrivateRingCloses::<T>::iter() {
				match PrivateRings::<T>::get(game_index) {
					None => return Err(TryRuntimeError::Other("close index has no ring")),
					Some(ring) if Self::close_key(ring.closes_at) != closes_at =>
						return Err(TryRuntimeError::Other(
							"close index names another block than the ring closes in",
						)),
					Some(_) => {},
				}
			}

			// The index is what the close finds a game's trees by, so an entry naming no tree
			// leaves state nothing removes, and a private tree that is not indexed outlives its
			// game's close.
			for (game_index, block, ()) in PrivateGameTrees::<T>::iter() {
				match CreditTrees::<T>::get(block) {
					None => return Err(TryRuntimeError::Other("private tree index has no tree")),
					Some(tree) if tree.private_slots == 0 || tree.game_index != game_index =>
						return Err(TryRuntimeError::Other(
							"private tree index names another game's tree",
						)),
					Some(_) => {},
				}
			}
			for (block, tree) in CreditTrees::<T>::iter() {
				if tree.private_slots != 0 &&
					!PrivateGameTrees::<T>::contains_key(tree.game_index, block)
				{
					return Err(TryRuntimeError::Other("a private game's tree is not indexed"));
				}
			}

			// Registration requires a live collection and deletion clears it through
			// `indiv_pallet_scarcity::OnCollectionDeleted`, so an entry naming a collection that no
			// longer exists means the runtime did not wire that hook to `ClearCollectionMinter`.
			// The registered owner is deliberately not compared against the current one: an
			// ownership handover leaves the registration stale on purpose, and claims reject it.
			for (collection, _) in CollectionMinters::<T>::iter() {
				if T::Nfts::collection_owner(collection).is_none() {
					return Err(TryRuntimeError::Other(
						"a collection minter registration outlived its collection",
					));
				}
			}
			Ok(())
		}
	}

	struct SelectedItem {
		item: ItemIndex,
		kind: crate::runtime_api::SelectionKind,
		weight_consumed: Weight,
	}

	enum ItemSelectionError {
		CollectionNotRegistered,
		UnknownCollection,
		CollectionOwnerChanged,
		NoItems,
		Contract(SelectionError),
	}

	impl ItemSelectionError {
		fn into_claim_error<T: Config>(self) -> SelectionError {
			let error = match self {
				Self::CollectionNotRegistered => Error::<T>::CollectionNotRegistered.into(),
				Self::UnknownCollection => Error::<T>::UnknownCollection.into(),
				Self::CollectionOwnerChanged => Error::<T>::CollectionOwnerChanged.into(),
				Self::NoItems => Error::<T>::NoItems.into(),
				Self::Contract(error) => return error,
			};
			SelectionError { error, weight_consumed: Weight::zero() }
		}

		fn into_preview_failure(self) -> crate::runtime_api::PreviewFailure {
			use crate::runtime_api::PreviewFailure;

			match self {
				Self::CollectionNotRegistered => PreviewFailure::CollectionNotRegistered,
				Self::UnknownCollection => PreviewFailure::UnknownCollection,
				Self::CollectionOwnerChanged => PreviewFailure::CollectionOwnerChanged,
				Self::NoItems => PreviewFailure::NoItems,
				Self::Contract(error) =>
					PreviewFailure::ContractSelectionFailed { error: error.error },
			}
		}
	}

	impl<T: Config> Pallet<T> {
		/// Previews the item the real claim selection path chooses for one credit and collection.
		/// Contract execution can change the current storage overlay, so runtime API callers must
		/// discard that overlay after the request.
		pub fn preview_mint(
			credit: NftClaimCredit,
			collection: CollectionId,
		) -> crate::runtime_api::PreviewOutcome {
			use crate::runtime_api::{PreviewFailure, PreviewOutcome};

			match Self::select_item(collection, credit) {
				Ok(selection) => {
					if !T::Nfts::item_exists(collection, selection.item) {
						return PreviewOutcome::Fails {
							reason: PreviewFailure::UnknownItem { item: selection.item },
						};
					}
					PreviewOutcome::Mints { item: selection.item, via: selection.kind }
				},
				Err(error) => PreviewOutcome::Fails { reason: error.into_preview_failure() },
			}
		}

		/// Previews a positionally aligned batch through the real claim selection path.
		/// Oversized batches fail explicitly before any selector runs.
		pub fn preview_mints(
			queries: Vec<crate::runtime_api::PreviewQuery>,
		) -> Result<Vec<crate::runtime_api::PreviewOutcome>, crate::runtime_api::BatchError> {
			if queries.len() > crate::runtime_api::MAX_PREVIEW_QUERIES as usize {
				return Err(crate::runtime_api::BatchError::TooLarge {
					max: crate::runtime_api::MAX_PREVIEW_QUERIES,
				});
			}
			Ok(queries
				.into_iter()
				.map(|query| Self::preview_mint(query.credit, query.collection))
				.collect::<Vec<_>>())
		}

		/// Store one delivered private game outcome, keeping the one already held on a conflict.
		fn store_private_outcome(
			update: &PrivateRingDelivery<<T::RingVrf as GenerateVerifiable>::Members>,
		) {
			if update.slots == 0 {
				// A game that grants no slot is a public game, which reaches no outcome.
				log::error!(
					target: LOG_TARGET,
					"Invalid private outcome for game {}: no slots",
					update.game_index,
				);
				return;
			}

			match &update.outcome {
				PrivateGameOutcome::Ring { root, key_count } => {
					if *key_count == 0 {
						// The game chain builds no ring below its own key floor, so an empty
						// ring cannot be genuine.
						log::error!(
							target: LOG_TARGET,
							"Invalid private ring for game {}: no keys",
							update.game_index,
						);
						return;
					}

					// A closed game's spent aliases went with its ring, so a fresh window over
					// the same keys would mint every slot of the game a second time.
					if AbandonedPrivateGames::<T>::contains_key(update.game_index) ||
						ClosedPrivateGames::<T>::contains_key(update.game_index)
					{
						Self::note_private_outcome_conflict(update.game_index);
						return;
					}

					// The window runs from this block, so every member of the ring gets the same
					// one. A redelivery keeps the window the first one set: extending it would
					// leave the last claims of a game standing alone in time.
					let opens_at = frame_system::Pallet::<T>::block_number()
						.saturating_add(T::PrivateClaimDelay::get());
					let closes_at = opens_at.saturating_add(T::PrivateClaimWindow::get());
					let ring = PrivateRing {
						root: root.clone(),
						slots: update.slots,
						key_count: *key_count,
						opens_at,
						closes_at,
					};

					match PrivateRings::<T>::get(update.game_index) {
						Some(existing)
							if existing.root != ring.root ||
								existing.slots != ring.slots ||
								existing.key_count != ring.key_count =>
						{
							// A game's ring is built once and never changes, so two rings for one
							// game mean the chains disagree about who registered.
							Self::note_private_outcome_conflict(update.game_index);
						},
						Some(_) => {},
						None => {
							PrivateRings::<T>::insert(update.game_index, ring);
							PrivateRingCloses::<T>::insert(
								Self::close_key(closes_at),
								update.game_index,
								(),
							);
							Self::deposit_event(Event::PrivateRingReceived {
								game_index: update.game_index,
								slots: update.slots,
								key_count: *key_count,
								opens_at,
								closes_at,
							});
						},
					}
				},
				PrivateGameOutcome::Abandoned { key_count } => {
					if PrivateRings::<T>::contains_key(update.game_index) ||
						ClosedPrivateGames::<T>::contains_key(update.game_index)
					{
						// Claims may already rest on the ring, and reopening the public path
						// would mint a second NFT for every credit they spent. A closed game
						// held a ring too, whatever was claimed against it.
						Self::note_private_outcome_conflict(update.game_index);
						return;
					}
					if AbandonedPrivateGames::<T>::contains_key(update.game_index) {
						return;
					}

					AbandonedPrivateGames::<T>::insert(update.game_index, ());
					Self::deposit_event(Event::PrivateGameAbandoned {
						game_index: update.game_index,
						key_count: *key_count,
					});
				},
			}
		}

		/// The body of [`Pallet::close_private_ring`], returning the aliases and the trees it
		/// removed.
		///
		/// The trees go with the ring because a game that built one reaches no second outcome, so
		/// [`Pallet::claim`] refuses them for good and a private claim never proves against a
		/// tree. Their [`TreeExpiries`] entries stay, as they do for a fully claimed tree, and the
		/// sweep of those entries removes the bitmaps.
		fn do_close_private_ring(game_index: GameIdx) -> Result<(u32, u32), DispatchError> {
			let ring = PrivateRings::<T>::get(game_index).ok_or(Error::<T>::UnknownPrivateRing)?;
			ensure!(
				frame_system::Pallet::<T>::block_number() >= ring.closes_at,
				Error::<T>::PrivateClaimWindowOpen
			);

			// One step removes a deletion message's worth of trees, so it queues no more than one
			// send drains. A queue that is already full still drops them, and
			// `queue_tree_deletions` reports that.
			let tree_budget = T::MaxTreeDeletionsPerMessage::get();
			let blocks = PrivateGameTrees::<T>::iter_key_prefix(game_index)
				.take(tree_budget as usize)
				.collect::<Vec<_>>();
			let trees = blocks.len() as u32;
			for block in &blocks {
				PrivateGameTrees::<T>::remove(game_index, block);
				CreditTrees::<T>::remove(block);
			}
			Self::queue_tree_deletions(&blocks);
			if trees > 0 {
				Self::deposit_event(Event::PrivateGameTreesRemoved { game_index, count: trees });
			}

			// The aliases are read before they are removed, rather than cleared by prefix, so
			// that the count the refund is measured in is exact.
			let aliases = SpentPrivateClaims::<T>::iter_key_prefix(game_index)
				.take(PRIVATE_CLOSE_ITEMS as usize)
				.collect::<Vec<_>>();
			let removed = aliases.len() as u32;
			for alias in &aliases {
				SpentPrivateClaims::<T>::remove(game_index, alias);
			}

			// A step that spent a whole budget leaves the rest to the next one. The ring is what
			// says the removal is still owed, so it goes with the last of them.
			if removed < PRIVATE_CLOSE_ITEMS && trees < tree_budget {
				PrivateRings::<T>::remove(game_index);
				PrivateRingCloses::<T>::remove(Self::close_key(ring.closes_at), game_index);
				ClosedPrivateGames::<T>::insert(game_index, ());
				Self::deposit_event(Event::PrivateRingClosed { game_index });
			}

			Ok((removed, trees))
		}

		/// Validates a [`Pallet::close_private_ring`] transaction.
		///
		/// This accepts local and in-block sources only, as
		/// [`Pallet::authorize_sweep_expired_trees`] does. A game whose ring is gone has nothing
		/// left to close, and a window that is still open reports [`InvalidTransaction::Future`]:
		/// the closing block alone is what makes that call valid, so the pool keeps it.
		pub fn authorize_close_private_ring(
			source: TransactionSource,
			game_index: &GameIdx,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			if !matches!(source, TransactionSource::InBlock | TransactionSource::Local) {
				return Err(AuthorizeInvalidity::TransactionNotLocal.into());
			}

			let ring = PrivateRings::<T>::get(game_index)
				.ok_or(AuthorizeInvalidity::NoPrivateRingToClose)?;
			let now = frame_system::Pallet::<T>::block_number();
			if now < ring.closes_at {
				return Err(InvalidTransaction::Future.into());
			}

			// The tag is the game, so every step of one close shares it and the pool keeps one
			// attempt. A step of another game carries a tag of its own.
			//
			// A close only frees storage, so it yields to every other transaction. No claim of the
			// game is taken once the window has shut, whether the close has run or not.
			let validity = ValidTransaction::with_tag_prefix("nft-claims:close-private-ring")
				.and_provides(game_index)
				// The block number rises with every step, so a fresh step outranks the attempt
				// holding the same tag. The pool replaces that attempt only for a strictly
				// higher priority.
				.priority(tx_priority::add_tie_break(
					tx_priority::CLEANUP,
					now.saturated_into::<u64>(),
				))
				.longevity(TX_LONGEVITY)
				.propagate(false)
				.build()
				.expect("tag prefix is not empty; qed");

			Ok((validity, Weight::zero()))
		}

		/// Report a second, different outcome for a game. The stored one is kept.
		fn note_private_outcome_conflict(game_index: GameIdx) {
			log::error!(
				target: LOG_TARGET,
				"Conflicting private outcome for game {game_index}, keeping the stored one",
			);
			Self::deposit_event(Event::PrivateOutcomeConflict { game_index });
		}

		/// Validate a [`Pallet::claim_private`] submission.
		///
		/// The proof authorizes the call, so everything the dispatch relies on is checked here:
		/// the ring exists, the slot is one the game grants, the proof verifies under that slot's
		/// context and yields `alias`, and `alias` is unspent. The dispatch runs on the same state
		/// straight after and only spends the alias.
		///
		/// Any transaction source is taken. A claimant need not hold an account, so a claim has to
		/// be able to arrive over the network.
		///
		/// A claim whose dispatch fails, on a minter contract that reverts or a collection with
		/// no items, spends neither its alias nor the block's allowance and can be submitted
		/// again for nothing. The alias is the `provides` tag and a member holds one alias per
		/// slot, so the claims retried this way number no more than the claims those members
		/// would make anyway.
		pub(crate) fn authorize_claim_private(
			_source: TransactionSource,
			game_index: &GameIdx,
			slot: &PrivateClaimSlot,
			alias: &Alias,
			proof: &RingProofOf<T>,
			collection: &CollectionId,
			mint_to: &T::AccountId,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			// The allowance is read before the proof, because checking it costs a verification
			// itself. `Future` keeps the claim in the pool, a later block being what makes it
			// valid.
			if PrivateClaimsThisBlock::<T>::get() >= T::MaxPrivateClaimsPerBlock::get() {
				return Err(InvalidTransaction::Future.into());
			}

			let ring = PrivateRings::<T>::get(game_index)
				.ok_or(AuthorizeInvalidity::UnknownPrivateRing)?;

			// Checked before the proof, which is the dear part. `Future` keeps a claim made
			// ahead of the window in the pool, the opening block being what makes it valid,
			// whereas a closed window never takes one again.
			let now = frame_system::Pallet::<T>::block_number();
			if now < ring.opens_at {
				return Err(InvalidTransaction::Future.into());
			}
			ensure!(now < ring.closes_at, AuthorizeInvalidity::PrivateClaimWindowClosed);

			ensure!(*slot < ring.slots, AuthorizeInvalidity::SlotOutOfRange);
			ensure!(
				!SpentPrivateClaims::<T>::contains_key(game_index, alias),
				AuthorizeInvalidity::SlotAlreadyClaimed
			);

			let config = T::PrivateRingExponent::get()
				.try_into()
				.map_err(|_| AuthorizeInvalidity::InvalidRingExponent)?;
			let proven = T::RingVrf::validate(
				config,
				proof,
				&ring.root,
				&Self::private_claim_context(*game_index, *slot),
				&Self::private_claim_message(*collection, mint_to),
			)
			.map_err(|_| AuthorizeInvalidity::InvalidRingProof)?;
			ensure!(proven == *alias, AuthorizeInvalidity::InvalidRingProof);

			Ok((
				ValidTransaction {
					priority: indiv_support::tx_priority::USER_DEFAULT,
					requires: Vec::new(),
					// The alias is the nullifier, so one alias is one claim in the pool as it is
					// on chain, whatever collection or purse key a resubmission names.
					provides: Vec::from(
						[(b"nft-claims/claim-private", game_index, alias).encode()],
					),
					longevity: PRIVATE_CLAIM_TX_LONGEVITY,
					propagate: true,
				},
				Weight::zero(),
			))
		}

		/// The context a claim of `slot` in `game_index` is proven under.
		///
		/// There is one context per game and slot. A member's aliases in two slots are therefore
		/// unlinkable, and a proof for one game does not verify in another.
		pub fn private_claim_context(game_index: GameIdx, slot: PrivateClaimSlot) -> [u8; 32] {
			build_product_context(
				private_nft_claims::PRODUCT_NAME,
				&T::PrivateClaimNetworkSuffix::get(),
				private_nft_claims::slot(game_index, slot),
			)
		}

		/// The message a private claim's proof commits to.
		///
		/// It binds the collection and the purse key. Without them, anyone who saw a pending
		/// claim could resubmit its proof and spend the alias on an item and a purse of their own
		/// choosing.
		pub fn private_claim_message(collection: CollectionId, mint_to: &T::AccountId) -> Vec<u8> {
			(b"nft-claims/private", collection, mint_to).encode()
		}

		/// The item of `collection` that claiming `credit` mints, per the collection's
		/// registered [`ItemSelection`], with the weight the selection consumed.
		///
		/// A contract selection's failure is returned as its error: the contract is how the
		/// collection's owner gates minting, so no fallback overrides it. A failure carries the
		/// weight it consumed, so the claim charges what really ran.
		fn select_item(
			collection: CollectionId,
			credit: NftClaimCredit,
		) -> Result<SelectedItem, ItemSelectionError> {
			let registration = CollectionMinters::<T>::get(collection)
				.ok_or(ItemSelectionError::CollectionNotRegistered)?;
			let owner = T::Nfts::collection_owner(collection)
				.ok_or(ItemSelectionError::UnknownCollection)?;
			ensure!(owner == registration.owner, ItemSelectionError::CollectionOwnerChanged);
			match registration.selection {
				ItemSelection::Random => {
					let next_item = T::Nfts::next_item_index(collection)
						.ok_or(ItemSelectionError::UnknownCollection)?;
					ensure!(next_item > 0, ItemSelectionError::NoItems);
					let draw = u32::from_le_bytes(
						credit[..4].try_into().expect("a credit holds at least four bytes"),
					);
					Ok(SelectedItem {
						item: draw % next_item,
						kind: crate::runtime_api::SelectionKind::Random,
						weight_consumed: Weight::zero(),
					})
				},
				ItemSelection::Contract(contract) =>
					T::CollectionSelector::select(owner, contract, collection, credit)
						.map(|selection| SelectedItem {
							item: selection.item,
							kind: crate::runtime_api::SelectionKind::Contract(contract),
							weight_consumed: selection.weight_consumed,
						})
						.map_err(ItemSelectionError::Contract),
			}
		}

		/// Advances the expected sequence over the sequenced trees of `batch` and reports the
		/// ones that were skipped.
		///
		/// Only the highest sequence in the batch matters: trees arrive in ascending order, so
		/// anything below the expected sequence has already been accounted for, and one gap
		/// event covers a whole run of lost trees.
		fn note_sequences(batch: &CreditTreeBatch<T>) {
			let Some(highest) = batch.trees.iter().filter_map(|update| update.sequence).max()
			else {
				// A batch of resent trees only, which says nothing about the live stream.
				return;
			};

			let expected = NextExpectedSequence::<T>::get();
			if highest < expected {
				return;
			}

			let lowest =
				batch.trees.iter().filter_map(|update| update.sequence).min().unwrap_or(highest);
			if lowest > expected {
				Self::deposit_event(Event::CreditTreesMissing {
					from_sequence: expected,
					to_sequence: lowest.saturating_sub(1),
				});
			}

			NextExpectedSequence::<T>::put(highest.saturating_add(1));
		}

		/// Whether `now` has reached the deadline [`Config::TreeTtl`] puts on a tree committed to
		/// at `tree_timestamp`. Both are seconds since the UNIX epoch.
		pub(crate) fn tree_has_expired(tree_timestamp: u32, now: u64) -> bool {
			now >= expiry_deadline(tree_timestamp, T::TreeTtl::get())
		}

		/// Files the tree of `block` under the timestamp it commits to, so a sweep finds it once
		/// that timestamp is [`Config::TreeTtl`] old.
		pub(crate) fn note_tree_expiry(block: AwardBlock, timestamp: u32) {
			TreeExpiries::<T>::insert(ExpiryTimestamp::from(timestamp), block, ());
		}

		/// Whether `leaf_index` is set in `bitmap`, which holds one bit per leaf of an award
		/// block's tree.
		pub fn leaf_is_claimed(bitmap: &[u8], leaf_index: u32) -> bool {
			let byte = (leaf_index / 8) as usize;
			bitmap.get(byte).is_some_and(|bits| bits & (1u8 << (leaf_index % 8)) != 0)
		}

		/// How many leaves `bitmap` records as claimed.
		pub(crate) fn claimed_leaf_count(bitmap: &[u8]) -> u32 {
			bitmap.iter().map(|bits| bits.count_ones()).sum()
		}

		/// Sets the bit of `leaf_index` in `block`'s bitmap, widening it to `leaf_count` bits.
		///
		/// `leaf_index` must be below `leaf_count`, and `leaf_count` must be within
		/// [`Config::MaxCreditsPerAwardBlock`], which [`Pallet::receive_credit_trees`] holds every
		/// stored tree to. Both are checked here, so a bit outside the bitmap is an error rather
		/// than a silent no-op.
		fn spend_leaf(block: AwardBlock, leaf_index: u32, leaf_count: u32) -> Result<(), ()> {
			if leaf_index >= leaf_count || leaf_count > T::MaxCreditsPerAwardBlock::get() {
				return Err(());
			}

			let bytes = leaf_count.div_ceil(8) as usize;
			let byte = (leaf_index / 8) as usize;
			ClaimedLeaves::<T>::mutate(block, |bitmap| {
				if bitmap.len() < bytes {
					// `bytes` covers `MaxCreditsPerAwardBlock` bits at most, which is the bound.
					let mut bits = core::mem::take(bitmap).into_inner();
					bits.resize(bytes, 0);
					*bitmap = BoundedVec::truncate_from(bits);
				}
				if let Some(bits) = bitmap.as_mut().get_mut(byte) {
					*bits |= 1u8 << (leaf_index % 8);
				}
			});

			Ok(())
		}

		/// Removes `tree`, the stored tree of `block`, and queues its deletion for the game chain.
		///
		/// The expiry entry stays, and so does [`ClaimedLeaves`]. The game chain holds its root
		/// for longer than this chain holds the tree, and anyone can replay the tree from there,
		/// so only the spent leaves stop it from minting its credits twice. The sweep of that
		/// entry removes the bitmap once the deadline has passed.
		fn remove_tree(block: AwardBlock, tree: &NftClaimCreditTree) {
			CreditTrees::<T>::remove(block);
			if tree.private_slots != 0 {
				// A claim reaches a private tree only when its game was abandoned, and such a
				// game holds no ring for a close to remove the entry with.
				PrivateGameTrees::<T>::remove(tree.game_index, block);
			}
			Self::queue_tree_deletions(&[block]);
		}

		/// Queues `blocks` for the next deletion message and drops the ones the queue has no room
		/// for. Pass at most [`Config::MaxTreeDeletionsPerMessage`] blocks, which is what the
		/// dropped ones are reported in.
		///
		/// A dropped deletion leaves the game chain waiting for its own TTL. That TTL removes its
		/// copy, so nothing on this chain needs repair.
		pub(crate) fn queue_tree_deletions(blocks: &[AwardBlock]) {
			if blocks.is_empty() {
				return;
			}

			let dropped = PendingTreeDeletions::<T>::mutate(|queued| {
				blocks
					.iter()
					.filter(|block| queued.try_push(**block).is_err())
					.copied()
					.collect::<Vec<_>>()
			});
			if dropped.is_empty() {
				return;
			}

			log::error!(
				target: LOG_TARGET,
				"Tree deletion queue is full, the game chain has to expire blocks {dropped:?} \
				 itself",
			);
			Self::deposit_event(Event::TreeDeletionsDropped {
				blocks: BoundedVec::truncate_from(dropped),
			});
		}

		/// Retires up to [`Config::MaxTreeDeletionsPerMessage`] blocks whose deadline has passed,
		/// as [`Pallet::sweep_expired_trees`] does once its origin is checked.
		///
		/// A retired block's deadline has passed, so no replay delivers its tree again and its
		/// bitmap goes. A block whose tree was fully claimed holds none here and had its deletion
		/// queued then, so only the trees still held are expired and named to the game chain.
		pub(crate) fn do_sweep_expired_trees() -> PostDispatchInfo {
			let retired = drain_due_expiries::<TreeExpiries<T>, AwardBlock>(
				T::TreeTtl::get(),
				T::UnixTime::now().as_secs(),
				T::MaxTreeDeletionsPerMessage::get(),
			);

			let mut expired = Vec::with_capacity(retired.len());
			for block in &retired {
				ClaimedLeaves::<T>::remove(block);
				if let Some(tree) = CreditTrees::<T>::take(block) {
					if tree.private_slots != 0 {
						// A private game whose ring arrived has its trees removed at its close,
						// so this game was abandoned or reached no outcome. No close and no claim
						// removes the entry, so the sweep does.
						PrivateGameTrees::<T>::remove(tree.game_index, block);
					}
					expired.push(*block);
				}
			}
			Self::queue_tree_deletions(&expired);

			let count = expired.len() as u32;
			if count > 0 {
				Self::deposit_event(Event::CreditTreesExpired { count });
			}

			Some(T::WeightInfo::sweep_expired_trees(retired.len() as u32)).into()
		}

		/// Validates a [`Pallet::sweep_expired_trees`] transaction, as
		/// [`authorize_expiry_sweep`] does, the deadline being the one [`Config::TreeTtl`] names.
		pub fn authorize_sweep_expired_trees(
			source: TransactionSource,
			oldest: &u32,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			authorize_expiry_sweep::<T, TreeExpiries<T>, AwardBlock>(
				ExpirySweepTx {
					tag: "nft-claims:sweep-expired-trees",
					not_local: AuthorizeInvalidity::TransactionNotLocal.into(),
					nothing_to_sweep: AuthorizeInvalidity::NothingToSweep.into(),
				},
				source,
				*oldest,
				T::TreeTtl::get(),
				T::UnixTime::now().as_secs(),
			)
		}

		/// Sends the queued deletions that fit one message, as [`Pallet::send_tree_deletions`] does
		/// once its origin is checked.
		///
		/// A message that fails to send leaves the queue unchanged and reports
		/// [`Event::TreeDeletionSendFailed`]. The next offchain-worker cycle retries the same
		/// front.
		pub(crate) fn do_send_tree_deletions() -> PostDispatchInfo {
			let queued = PendingTreeDeletions::<T>::get();
			debug_assert!(!queued.is_empty(), "authorize should have rejected: nothing queued");

			let taken = (T::MaxTreeDeletionsPerMessage::get() as usize).min(queued.len());
			// `taken` is at most the bound this vector carries, so nothing truncates.
			let blocks = BoundedVec::<AwardBlock, T::MaxTreeDeletionsPerMessage>::truncate_from(
				queued[..taken].to_vec(),
			);

			if let Err(e) = Self::send_tree_deletion_message(blocks.clone()) {
				log::warn!(
					target: LOG_TARGET,
					"Tree deletion XCM failed: {e:?}, retrying next offchain worker cycle",
				);
				Self::deposit_event(Event::TreeDeletionSendFailed);
				return Some(T::WeightInfo::send_tree_deletions(taken as u32)).into();
			}

			PendingTreeDeletions::<T>::mutate(|queued| {
				queued.drain(..taken);
			});
			Self::deposit_event(Event::TreeDeletionsSent { blocks });

			Some(T::WeightInfo::send_tree_deletions(taken as u32)).into()
		}

		/// Validates a [`Pallet::send_tree_deletions`] transaction.
		///
		/// This accepts local and in-block sources only, as
		/// [`Pallet::authorize_sweep_expired_trees`] does. `front` must equal the queue's first
		/// block, which a successful send replaces, so a retry of a send that landed is `Stale`.
		/// The queue holds the blocks in removal order, so a block that is still queued does not
		/// compare as later than the front and no mismatch is `Future`.
		pub fn authorize_send_tree_deletions(
			source: TransactionSource,
			front: &AwardBlock,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			if !matches!(source, TransactionSource::InBlock | TransactionSource::Local) {
				return Err(AuthorizeInvalidity::TransactionNotLocal.into());
			}

			let Some(queued_front) = PendingTreeDeletions::<T>::get().first().copied() else {
				return Err(AuthorizeInvalidity::NoQueuedTreeDeletions.into());
			};
			if *front != queued_front {
				return Err(InvalidTransaction::Stale.into());
			}

			// The tag is the front, so the sends of two fronts do not share one. Only the current
			// front authorizes, so one block holds at most one send.
			let validity = ValidTransaction::with_tag_prefix("nft-claims:send-tree-deletions")
				.and_provides(queued_front)
				// The block number rises with every retry window, so a retry outranks the attempt
				// holding the same tag. The pool replaces that attempt only for a strictly higher
				// priority.
				.priority(tx_priority::BACKGROUND_PROGRESS.saturating_add(
					frame_system::Pallet::<T>::block_number().saturated_into::<u64>(),
				))
				.longevity(TX_LONGEVITY)
				.propagate(false)
				.build()
				.expect("tag prefix is not empty; qed");

			Ok((validity, Weight::zero()))
		}

		/// Hands the game chain's deletion call to the router, reporting the router's own reason
		/// for a refusal so a stalled channel can be told from an oversized message.
		fn send_tree_deletion_message(
			blocks: BoundedVec<AwardBlock, T::MaxTreeDeletionsPerMessage>,
		) -> Result<(), SendError> {
			let call = (
				T::GameChainPalletIndex::get(),
				NftCreditsCall::<T>::ReceiveTreeDeletions { blocks },
			)
				.encode();

			send_xcm::<T::XcmRouter>(T::GameChainLocation::get(), Self::tree_deletion_xcm(call))
				.map(|_| ())
		}

		fn tree_deletion_xcm(encoded_call: Vec<u8>) -> Xcm<()> {
			Xcm(vec![
				UnpaidExecution { weight_limit: WeightLimit::Unlimited, check_origin: None },
				Transact {
					origin_kind: OriginKind::Native,
					call: encoded_call.into(),
					fallback_max_weight: None,
				},
			])
		}

		/// The encoded size of the message that deletes `blocks` trees, which a router compares
		/// against the channel's `max_message_size`.
		///
		/// This encodes a full message instead of adding up its parts. Only the `integrity_test`
		/// calls it, so the cost does not matter.
		#[cfg(feature = "std")]
		fn tree_deletion_message_size(blocks: u32) -> usize {
			let blocks = BoundedVec::<AwardBlock, T::MaxTreeDeletionsPerMessage>::truncate_from(
				vec![AwardBlock::MAX; blocks as usize],
			);
			let call = (u8::MAX, NftCreditsCall::<T>::ReceiveTreeDeletions { blocks }).encode();

			xcm::VersionedXcm::<()>::from(Self::tree_deletion_xcm(call)).encoded_size()
		}

		/// Submits a [`Pallet::sweep_expired_trees`] for the oldest filed timestamp, if its
		/// deadline has passed.
		///
		/// This repeats the deadline check that `authorize` makes. Without it a chain with nothing
		/// expired submits a transaction every block that the pool holds as `Future`.
		pub(crate) fn submit_expiry_sweep(block_number: BlockNumberFor<T>) {
			let Some(oldest) = oldest_expiry::<TreeExpiries<T>, AwardBlock>() else {
				return;
			};
			if T::UnixTime::now().as_secs() < expiry_deadline(oldest, T::TreeTtl::get()) {
				return;
			}

			let call = Call::<T>::sweep_expired_trees {
				oldest,
				// The submitting block, not the retry window `indiv_support::offchain` paces other
				// calls by. Trees at one timestamp can outnumber one sweep's limit, which keeps
				// `oldest` the same, so a window would allow one sweep per window: the pool bans
				// the hash of the sweep it included, and the next attempt of that window repeats
				// it. The `provides` tag keeps one attempt in the pool.
				discriminator: block_number,
			};
			submit_authorized::<T, _>(call, "sweep_expired_trees", LOG_TARGET);
		}

		/// Submits a [`Pallet::send_tree_deletions`] for the queued deletions, if any.
		pub(crate) fn submit_tree_deletions(block_number: BlockNumberFor<T>) {
			let Some(front) = PendingTreeDeletions::<T>::get().first().copied() else {
				return;
			};

			let call = Call::<T>::send_tree_deletions {
				// A send replaces the front, so the next batch's send carries a hash of its own
				// and reaches the pool in the following block instead of the next window. The
				// sweep refills the queue as fast as the send drains it, so the send keeps that
				// pace.
				front,
				discriminator: block_number / RETRY_WINDOW.into(),
			};
			submit_authorized::<T, _>(call, "send_tree_deletions", LOG_TARGET);
		}

		/// The [`PrivateRingCloses`] key a window closing in `closes_at` is filed under.
		///
		/// The block is widened to `u64`, so the key covers every block number a runtime may use
		/// and orders them as it orders the smaller ones.
		fn close_key(closes_at: BlockNumberFor<T>) -> BigEndianU64 {
			BigEndianU64(closes_at.saturated_into::<u64>())
		}

		/// Submits a [`Pallet::close_private_ring`] for the game whose claim window closed first.
		///
		/// This repeats the window check that `authorize` makes. Without it a chain whose windows
		/// are all open submits a transaction every block that the pool holds as `Future`. One
		/// game goes per block: a close takes as many steps as its state needs, and a step of the
		/// next game waits for those.
		///
		/// [`PrivateRingCloses`] iterates in closing order, so this reads the one entry it needs
		/// and decodes no ring at all. Reading the rings instead would decode a ring commitment
		/// per open window to find the same game.
		pub(crate) fn submit_private_ring_close(block_number: BlockNumberFor<T>) {
			let Some((closes_at, game_index)) = PrivateRingCloses::<T>::iter_keys().next() else {
				return;
			};
			if closes_at.0 > block_number.saturated_into::<u64>() {
				return;
			}

			let call = Call::<T>::close_private_ring {
				game_index,
				// The submitting block, not the retry window `indiv_support::offchain` paces
				// other calls by. A game's close takes several steps, which keeps `game_index`
				// the same: the pool bans the hash of the step it included, and the next attempt
				// of that window would repeat it. The `provides` tag keeps one attempt in the
				// pool.
				discriminator: block_number,
			};
			submit_authorized::<T, _>(call, "close_private_ring", LOG_TARGET);
		}
	}
}

impl<T: Config> Pallet<T> {
	/// The commitment held for `block`, which a claim for a credit awarded in that block is
	/// verified against.
	pub fn credit_tree(block: AwardBlock) -> Option<NftClaimCreditTree> {
		CreditTrees::<T>::get(block)
	}
}

/// Clears a collection's minter registration when Scarcity deletes the collection, so no
/// registration outlives the collection it names. The runtime wires this into
/// [`indiv_pallet_scarcity::Config::OnCollectionDeleted`].
pub struct ClearCollectionMinter<T>(core::marker::PhantomData<T>);

impl<T: Config> indiv_pallet_scarcity::OnCollectionDeleted for ClearCollectionMinter<T> {
	fn on_collection_deleted(collection: CollectionId) {
		CollectionMinters::<T>::remove(collection);
	}

	fn on_delete_weight() -> Weight {
		T::DbWeight::get().writes(1)
	}
}
