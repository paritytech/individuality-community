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

//! Types for the nft-claims pallet.

use crate::Config;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use indiv_support::credit_trees::PrivateClaimSlot;
use scale_info::TypeInfo;
use sp_core::H160;

/// Batch of credit trees received from the game pallet, bounded by this pallet's per-message
/// limit.
pub type CreditTreeBatch<T> =
	indiv_support::credit_trees::CreditTreeBatch<<T as Config>::MaxTreesPerMessage>;

/// Which of the signer's two identities a claim is made under.
///
/// A credit's leaf commits to one of them, so a claim under the other rehashes to a leaf that is
/// in no tree. One account can hold credits of both kinds, which is why the call names the kind
/// rather than the chain inferring it.
#[derive(
	Clone,
	Copy,
	PartialEq,
	Eq,
	Debug,
	Encode,
	Decode,
	DecodeWithMemTracking,
	MaxEncodedLen,
	TypeInfo,
)]
pub enum ClaimantKind {
	/// The signing account itself, as the game chain recorded an account player.
	Account,
	/// The person alias the signing account is bound to.
	Person,
}

/// How a registered collection picks the item a claim mints, chosen by the collection's owner.
#[derive(
	Clone,
	Copy,
	PartialEq,
	Eq,
	Debug,
	Encode,
	Decode,
	DecodeWithMemTracking,
	TypeInfo,
	MaxEncodedLen,
)]
pub enum ItemSelection {
	/// A pseudo-random item index: the credit modulo the collection's next item index. The
	/// claimant chooses the collection, not the item within it, as long as the item set is fixed
	/// before claims open. The next item index is the modulus, so define every item up front and
	/// delete none: additions shift which item a credit maps to and a deleted index a credit
	/// lands on fails.
	Random,
	/// The collection's minter contract picks the item, called with the credit as its only
	/// entropy.
	Contract(H160),
}

/// A collection's claim registration, bound to the owner who authorized it.
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
)]
pub struct CollectionMinter<AccountId> {
	/// The collection owner who authorized deposit-free claim minting.
	pub owner: AccountId,
	/// How claims choose the item to mint.
	pub selection: ItemSelection,
}

/// One game's private claim ring, as the game chain delivered it, with the window its claims are
/// made in.
///
/// A claim's proof is checked against the root. The root never changes, a ring being built once
/// after registration closed, and it holds every key registered for the game. The window is fixed
/// when the ring arrives and a redelivery does not move it.
#[derive(
	Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Debug, Clone, PartialEq, Eq,
)]
pub struct PrivateRing<Members, BlockNumber> {
	/// The ring root a claim proves membership in.
	pub root: Members,
	/// The slots every member of the ring holds, which bounds the slot a claim may name.
	pub slots: PrivateClaimSlot,
	/// The number of keys in the ring, which is the anonymity set of every claim against it.
	pub key_count: u32,
	/// The first block a claim of the game is taken in. Every member's claims open together, so
	/// the wallets that watch the chain closest are not the ones that claim first. A claim before
	/// it waits in the pool.
	pub opens_at: BlockNumber,
	/// The first block a claim of the game is refused in. Claims spread over one window cover
	/// each other, whereas a claim in an open-ended tail stands alone in time. A member who lets
	/// it pass mints nothing.
	pub closes_at: BlockNumber,
}
