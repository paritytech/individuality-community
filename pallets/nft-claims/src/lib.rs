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
//! The NFT itself is a `pallet-scarcity` instance, minted with no storage deposit: the credit is
//! what bounds the state a claim creates, since the game chain awards a credit once and
//! [`ClaimedCredits`] spends it once. Scarcity purse keys hold one NFT each and take no
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
//! Once the window is closed, [`Pallet::close_private_ring`] drops the ring and the aliases spent
//! against it. No claim can be made by then, so nothing is kept to stop one. It is a signed,
//! permissionless call: anyone may pay to reclaim the space, and until somebody does the state
//! sits there unused. The game is recorded in [`ClosedPrivateGames`], which refuses a later
//! outcome for it: without the aliases the dropped ring's slots would mint again.
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

use frame_support::{
	dispatch::WithPostDispatchInfo,
	traits::{EnsureOrigin, EnsureOriginWithArg, Get},
	weights::Weight,
};
use indiv_support::{
	context::{build_product_context, private_nft_claims, ProductContextNetworkSuffix},
	credit_trees::{
		credit_leaf, AwardBlock, CreditProofNode, NftClaimCredit, NftClaimCreditLeaf,
		NftClaimCreditTree, PrivateClaimSlot, PrivateGameOutcome, PrivateRingBatch,
		PrivateRingDelivery, TreeSequence,
	},
	identity::AccountOrPerson,
	traits::{Alias, RingExponent},
	weight_budget::OcwWeightBudget,
};
use pallet_scarcity::{CollectionId, InspectCollection, InstanceId, ItemIndex, MintWithoutDeposit};
use sp_core::{H160, H256};
use sp_runtime::{traits::BlakeTwo256, DispatchError, Saturating};
use verifiable::GenerateVerifiable;

pub use indiv_support::credit_trees::GameIdx;

const LOG_TARGET: &str = "runtime::indiv-pallet-nft-claims";

/// How many spent aliases one [`Pallet::close_private_ring`] call removes.
///
/// A closed window's ring holds one alias per claim that was made, so the removal runs in bounded
/// steps. It is a pallet constant because only the weight of one call depends on it.
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
}

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use alloc::vec::Vec;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	#[pallet::pallet]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Origin check for the XCM messages carrying credit trees, which authenticates the
		/// chain the game pallet runs on.
		type EnsureGameChainOrigin: EnsureOrigin<Self::RuntimeOrigin>;

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

		/// The NFTs a claim mints, which is `pallet-scarcity`.
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

	/// The leaves of an award block's tree that have been claimed.
	/// A leaf commits to one claimant holding one credit, so it identifies the claim on its own.
	/// Entries are kept for good: dropping one would let that credit mint a second NFT.
	#[pallet::storage]
	pub type ClaimedCredits<T: Config> = StorageDoubleMap<
		_,
		Twox64Concat,
		AwardBlock,
		Identity,
		NftClaimCreditLeaf,
		(),
		OptionQuery,
	>;

	/// How many of an award block's leaves have been claimed.
	/// Against the tree's `leaf_count`, this tells whether anything is left to claim.
	#[pallet::storage]
	pub type ClaimedCounts<T: Config> = StorageMap<_, Twox64Concat, AwardBlock, u32, ValueQuery>;

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
		/// Every credit committed to by `block`'s tree has now been claimed.
		TreeFullyClaimed { block: AwardBlock },
		/// `collection`'s owner registered it for claims with `selection`, or withdrew it with
		/// `None`.
		CollectionMinterSet { collection: CollectionId, selection: Option<ItemSelection> },
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

	/// Why a `claim_private` submission is not valid.
	///
	/// Reported as [`InvalidTransaction::Custom`], so a claimant can tell the causes apart. The
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
		#[pallet::call_index(1)]
		// Resolving a person claimant reads the signer's alias binding, which an account
		// claimant does not, so the kind the call names picks the weight.
		#[pallet::weight(
			match claimant {
				ClaimantKind::Account => T::WeightInfo::claim_account(proof.len() as u32),
				ClaimantKind::Person => T::WeightInfo::claim_person(proof.len() as u32),
			}
			.saturating_add(T::CollectionSelector::max_weight())
			.saturating_add(T::Nfts::mint_hook_weight())
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
			// Every failure carries `actual_weight` so the selector ceiling is refunded on the
			// error path too: a failed claim charges the claim's own weight plus what a failed
			// contract selection really consumed, not the whole reservation.
			let base = match claimant {
				ClaimantKind::Account => T::WeightInfo::claim_account(proof.len() as u32),
				ClaimantKind::Person => T::WeightInfo::claim_person(proof.len() as u32),
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
				!ClaimedCredits::<T>::contains_key(block, leaf),
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
			// dispatch, the entry included.
			ClaimedCredits::<T>::insert(block, leaf, ());

			let selection = Self::select_item(collection, credit).map_err(|error| {
				let error = error.into_claim_error::<T>();
				error.error.with_weight(base.saturating_add(error.weight_consumed))
			})?;
			let SelectedItem { item, weight_consumed: selection_weight, .. } = selection;
			let instance =
				T::Nfts::mint_without_deposit(collection, item, mint_to.clone(), Vec::new())
					.map_err(|e| e.with_weight(base.saturating_add(selection_weight)))?;

			// Counted after the selection: a contract may reenter with another credit of the
			// same block, and counting around its execution from a stale snapshot would drop
			// that claim's increment.
			let claimed = ClaimedCounts::<T>::mutate(block, |claimed| {
				*claimed = claimed.saturating_add(1);
				*claimed
			});

			Self::deposit_event(Event::CreditClaimed {
				block,
				leaf,
				collection,
				item,
				owner: mint_to,
				instance,
			});
			if claimed == tree.leaf_count {
				Self::deposit_event(Event::TreeFullyClaimed { block });
			}

			// The mint ran, so its runtime hooks did too. Only this path pays for them: every
			// failure above returns before the mint.
			Ok(Some(
				base.saturating_add(selection_weight)
					.saturating_add(T::Nfts::mint_hook_weight()),
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
		#[pallet::call_index(3)]
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
		#[pallet::call_index(4)]
		#[pallet::weight(
			T::WeightInfo::claim_private()
				.saturating_add(T::CollectionSelector::max_weight())
				.saturating_add(T::Nfts::mint_hook_weight())
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
					.saturating_add(T::Nfts::mint_hook_weight()),
			)
			.into())
		}

		/// Registers `collection` for claims with `selection` deciding the minted item, or
		/// withdraws it with `None`.
		///
		/// Registration is the owner's opt-in to deposit-free supply growth: without it no claim
		/// can mint into the collection. Withdrawing stops further claims and spends nothing
		/// already claimed. Deleting the collection clears its registration through
		/// [`pallet_scarcity::OnCollectionDeleted`], so an unknown collection can be neither
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

		/// Drops a private game's ring and the aliases spent against it, once its claim window is
		/// closed.
		///
		/// One call removes at most [`PRIVATE_CLOSE_ITEMS`] aliases and refunds the rest, so a
		/// game is dropped over as many calls as it takes. The ring goes last, because it holds
		/// the window that says the removal is allowed. Nothing removed here can gate a claim: a
		/// closed window takes none.
		///
		/// ## Origin
		/// Any signed account. The state belongs to nobody, so anyone may pay to reclaim it.
		///
		/// ## Parameters
		/// - `game_index`: The private game whose ring is dropped.
		#[pallet::call_index(5)]
		#[pallet::weight(T::WeightInfo::close_private_ring(PRIVATE_CLOSE_ITEMS))]
		pub fn close_private_ring(
			origin: OriginFor<T>,
			game_index: GameIdx,
		) -> DispatchResultWithPostInfo {
			ensure_signed(origin)?;

			let removed = Self::do_close_private_ring(game_index)?;

			Ok(Some(T::WeightInfo::close_private_ring(removed)).into())
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

			// A full batch arrives in an XCM `Transact` and is dispatched as one extrinsic, so a
			// worst case above the block's per-extrinsic limit can never execute: the message
			// fails and every tree in it is lost until it is replayed. The budget is the same
			// half of `Normal.max_extrinsic` the game pallet holds its sending side to, which
			// keeps the two ends of the delivery on one yardstick.
			OcwWeightBudget::from_normal_max::<T>().assert_fits(
				"receive_credit_trees",
				T::WeightInfo::receive_credit_trees(T::MaxTreesPerMessage::get()),
			);

			// A claim reserves the selector's ceiling on top of its own worst case whether the
			// collection uses a contract or not, so an unsubmittable worst case would make every
			// claim unsubmittable, not just contract-selected ones.
			OcwWeightBudget::from_normal_max::<T>().assert_fits(
				"claim",
				T::WeightInfo::claim_account(T::MaxProofNodes::get())
					.max(T::WeightInfo::claim_person(T::MaxProofNodes::get()))
					.saturating_add(T::CollectionSelector::max_weight())
					.saturating_add(T::Nfts::mint_hook_weight()),
			);

			// A private claim carries no signer and pays no fee, so the block budget is the only
			// bound on it. Both halves count, and the ring verification in `authorize` is the
			// heavier one.
			OcwWeightBudget::from_normal_max::<T>().assert_fits(
				"claim_private",
				T::WeightInfo::claim_private()
					.saturating_add(T::WeightInfo::authorize_claim_private())
					.saturating_add(T::CollectionSelector::max_weight())
					.saturating_add(T::Nfts::mint_hook_weight()),
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

			// The call is signed and its worst case is charged before the refund, so a worst
			// case above the limit leaves the ring undroppable.
			OcwWeightBudget::from_normal_max::<T>().assert_fits(
				"close_private_ring",
				T::WeightInfo::close_private_ring(PRIVATE_CLOSE_ITEMS),
			);
		}

		#[cfg(feature = "try-runtime")]
		fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
			Self::do_try_state()
		}
	}

	#[cfg(any(test, feature = "try-runtime"))]
	impl<T: Config> Pallet<T> {
		/// Check that the pallet's records agree with each other and with Scarcity: every
		/// claimed leaf belongs to a held tree, each block's claimed count matches its claimed
		/// leaves and never exceeds its tree's leaf count, and no registration outlives the
		/// collection it names.
		pub(crate) fn do_try_state() -> Result<(), sp_runtime::TryRuntimeError> {
			use alloc::collections::BTreeMap;
			use sp_runtime::TryRuntimeError;

			let mut counted = BTreeMap::<AwardBlock, u32>::new();
			for (block, _leaf, ()) in ClaimedCredits::<T>::iter() {
				if !CreditTrees::<T>::contains_key(block) {
					return Err(TryRuntimeError::Other("claimed credit has no tree"));
				}
				let count = counted.entry(block).or_default();
				*count = count
					.checked_add(1)
					.ok_or(TryRuntimeError::Other("claimed leaf count overflowed"))?;
			}

			let stored = ClaimedCounts::<T>::iter().collect::<BTreeMap<_, _>>();
			if stored != counted {
				return Err(TryRuntimeError::Other(
					"claimed counts do not match the claimed leaves",
				));
			}
			for (block, count) in &stored {
				let tree = CreditTrees::<T>::get(block)
					.ok_or(TryRuntimeError::Other("claimed count has no tree"))?;
				if *count > tree.leaf_count {
					return Err(TryRuntimeError::Other(
						"a block has more claims than its tree has leaves",
					));
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
			}

			// Registration requires a live collection and deletion clears it through
			// `pallet_scarcity::OnCollectionDeleted`, so an entry naming a collection that no
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

		/// The body of [`Pallet::close_private_ring`], returning the aliases it removed.
		fn do_close_private_ring(game_index: GameIdx) -> Result<u32, DispatchError> {
			let ring = PrivateRings::<T>::get(game_index).ok_or(Error::<T>::UnknownPrivateRing)?;
			ensure!(
				frame_system::Pallet::<T>::block_number() >= ring.closes_at,
				Error::<T>::PrivateClaimWindowOpen
			);

			// The aliases are read before they are removed, rather than cleared by prefix, so
			// that the count the refund is measured in is exact.
			let aliases = SpentPrivateClaims::<T>::iter_key_prefix(game_index)
				.take(PRIVATE_CLOSE_ITEMS as usize)
				.collect::<Vec<_>>();
			let removed = aliases.len() as u32;
			for alias in &aliases {
				SpentPrivateClaims::<T>::remove(game_index, alias);
			}

			// A step that spent its whole budget leaves the rest to the next one. The ring is
			// what says the removal is still owed, so it goes with the last of them.
			if removed < PRIVATE_CLOSE_ITEMS {
				PrivateRings::<T>::remove(game_index);
				ClosedPrivateGames::<T>::insert(game_index, ());
				Self::deposit_event(Event::PrivateRingClosed { game_index });
			}

			Ok(removed)
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
/// [`pallet_scarcity::Config::OnCollectionDeleted`].
pub struct ClearCollectionMinter<T>(core::marker::PhantomData<T>);

impl<T: Config> pallet_scarcity::OnCollectionDeleted for ClearCollectionMinter<T> {
	fn on_collection_deleted(collection: CollectionId) {
		CollectionMinters::<T>::remove(collection);
	}

	fn on_delete_weight() -> Weight {
		T::DbWeight::get().writes(1)
	}
}
