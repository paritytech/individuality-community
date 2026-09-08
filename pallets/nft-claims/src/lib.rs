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
//! [`ClaimedLeaves`] spends it once. Scarcity purse keys hold one NFT each and take no
//! destination consent, so the call names the key to mint to rather than minting to the
//! claimant's own account.
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
//! Two paths remove a tree. Both tell the game chain to drop its own copy, so the trees each chain
//! holds will be able to process all open claims.
//!
//! - **Fully claimed.** The set bits of [`ClaimedLeaves`] reach the tree's `leaf_count`. Every
//!   credit the tree commits to has been minted, so no proof can be built against it again, and the
//!   claim that completes it removes it.
//! - **Expiry.** A tree that is not fully claimed outlives [`Config::TreeTtl`]. The TTL runs from
//!   the award block's own wall-clock time, which the game chain records in the tree, not from the
//!   time the tree arrived here. [`Pallet::claim`] does not check the TTL, so the sweep that
//!   removes the tree is what ends claimability, and [`Event::CreditTreesExpired`] reports that
//!   those unclaimed credits are unmintable from then on.
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
//! The deletions owed to the game chain queue in [`PendingTreeDeletions`] and travel in a
//! [`Pallet::send_tree_deletions`] message, which the offchain worker submits as well. A deletion
//! is idempotent and carries no sequence number. The game chain's own TTL covers a deletion that is
//! lost, or that the queue had no room for, so no repair call exists.

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
use indiv_support::{
	credit_trees::{
		authorize_expiry_sweep, credit_leaf, drain_due_expiries, expiry_deadline, oldest_expiry,
		AwardBlock, CreditProofNode, ExpirySweepTx, ExpiryTimestamp, NftClaimCredit,
		NftClaimCreditLeaf, NftClaimCreditTree, TreeSequence,
	},
	identity::AccountOrPerson,
	offchain::{submit_authorized, RETRY_WINDOW, TX_LONGEVITY},
	tx_priority,
	weight_budget::OcwWeightBudget,
};
use pallet_scarcity::{CollectionId, InspectCollection, InstanceId, ItemIndex, MintWithoutDeposit};
use sp_core::{H160, H256};
use sp_runtime::{traits::BlakeTwo256, DispatchError, SaturatedConversion};
use xcm::{
	latest::{
		Instruction::{Transact, UnpaidExecution},
		Location, OriginKind, SendError, SendXcm, WeightLimit, Xcm,
	},
	prelude::send_xcm,
};

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
pub trait BenchmarkHelper<AccountId> {
	/// Make `collection` exist owned by `owner`, with `item` defined in it, as the owner would
	/// have done before the first claim.
	fn prepare_collection(owner: &AccountId, collection: CollectionId, item: ItemIndex);

	/// Deploy a contract that the collection registration benchmark can validate.
	fn prepare_contract(owner: &AccountId) -> H160;

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

		/// The most credits one award block commits to, which is the game chain's
		/// `MaxCreditsPerBlock`.
		///
		/// [`ClaimedLeaves`] holds one bit per leaf, so this sizes that bitmap. A tree committing
		/// to more leaves is not stored, because the leaves past the bitmap could be claimed
		/// twice. Set it to the game chain's own bound, which a lower value makes large trees
		/// undeliverable.
		#[pallet::constant]
		type MaxCreditsPerAwardBlock: Get<u32>;

		/// Setup the claim benchmark needs from the NFT backend.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: BenchmarkHelper<Self::AccountId>;
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
	}

	/// Why an offchain worker's submission was rejected, reported to the caller as
	/// `InvalidTransaction::Custom`.
	pub enum AuthorizeInvalidity {
		/// Transaction source is not local or in block.
		TransactionNotLocal = 210,
		/// No tree is filed for expiry, so there is nothing to sweep.
		NothingToSweep = 211,
		/// No tree deletion is waiting to be sent to the game chain.
		NoQueuedTreeDeletions = 212,
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
			Self::remove_tree(block);
			Self::deposit_event(Event::TreeFullyClaimed { block });

			Ok(Some(
				base_last
					.saturating_add(selection_weight)
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
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
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
		}

		fn offchain_worker(block_number: BlockNumberFor<T>) {
			Self::submit_expiry_sweep(block_number);
			Self::submit_tree_deletions(block_number);
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
		/// and every claimed-leaf bitmap is filed for expiry, and no registration outlives the
		/// collection it names.
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

		/// Removes the tree of `block` and queues its deletion for the game chain.
		///
		/// The expiry entry stays, and so does [`ClaimedLeaves`]. The game chain holds its root
		/// for longer than this chain holds the tree, and anyone can replay the tree from there,
		/// so only the spent leaves stop it from minting its credits twice. The sweep of that
		/// entry removes the bitmap once the deadline has passed.
		fn remove_tree(block: AwardBlock) {
			CreditTrees::<T>::remove(block);
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
				if CreditTrees::<T>::take(block).is_some() {
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
