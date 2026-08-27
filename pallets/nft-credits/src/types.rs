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

//! The types the NFT claim credits are held and served in.
//!
//! The credit, its leaf and the tree committing to a block's leaves live in `indiv-support`,
//! because the claims chain hashes and stores the very same values. What is here is what only the
//! awarding side needs, and is re-exported from the crate root.

use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use indiv_pallet_game::GameIdx;
use indiv_support::{
	credit_trees::{CreditProofNode, NftClaimCredit, NftClaimCreditLeaf, PrivateClaimSlot},
	identity::AccountOrPerson,
};
use scale_info::TypeInfo;

/// One credit of one claimant in one game, as a position in [`AwardedCredits`], derived from
/// the round and the [`indiv_pallet_game::AttesterPosition`] by `Pallet::credit_slot`.
pub type CreditSlot = u32;

/// The set of credits one claimant has been awarded in one game, held as one bit per
/// [`CreditSlot`].
///
/// [`Self::CAPACITY`] caps how many slots a game can use per claimant, which the pallet's
/// `integrity_test` holds the game's `MaxRounds * MaxGroupSize` to. A slot
/// beyond it has nowhere to be recorded, so the set reports it absent and refuses to insert
/// it, leaving [`Self::within_capacity`] as the check a caller makes once before relying on
/// either.
#[derive(
	Encode,
	Decode,
	DecodeWithMemTracking,
	MaxEncodedLen,
	TypeInfo,
	Debug,
	Clone,
	Copy,
	Default,
	PartialEq,
	Eq,
)]
pub struct AwardedCredits(u128);

impl AwardedCredits {
	/// The number of credit slots the set holds.
	pub const CAPACITY: u32 = u128::BITS;

	/// Every slot awarded, for benchmarks that need the worst case.
	#[cfg(feature = "runtime-benchmarks")]
	pub const FULL: Self = Self(u128::MAX);

	/// Whether `slot` is within [`Self::CAPACITY`], and so representable at all.
	pub const fn within_capacity(slot: CreditSlot) -> bool {
		slot < Self::CAPACITY
	}

	/// Whether `slot`'s credit is awarded. A slot the set cannot hold never is.
	pub fn contains(&self, slot: CreditSlot) -> bool {
		Self::bit(slot).is_some_and(|bit| self.0 & bit != 0)
	}

	/// Record `slot`'s credit as awarded. A slot the set cannot hold is not recorded.
	pub fn insert(&mut self, slot: CreditSlot) {
		if let Some(bit) = Self::bit(slot) {
			self.0 |= bit;
		}
	}

	/// How many of the claimant's credits this game has awarded.
	pub fn count(&self) -> u32 {
		self.0.count_ones()
	}

	/// The bit standing for `slot`, `None` beyond [`Self::CAPACITY`].
	const fn bit(slot: CreditSlot) -> Option<u128> {
		1u128.checked_shl(slot)
	}
}

/// One NFT claim credit as its block awarded it, which is the preimage of one
/// [`NftClaimCreditLeaf`].
///
/// Kept per award block in [`crate::NftClaimCreditAwards`] for as long as the block's awards are
/// retained, so a claim can be proven from state alone. Distinct from
/// [`crate::AwardedNftClaimCredits`], which only marks which of a game's credit slots a claimant
/// has had awarded.
#[derive(
	Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Debug, Clone, PartialEq, Eq,
)]
pub struct NftClaimCreditAward<AccountId> {
	/// Who the credit was awarded to, and who alone may mint against its leaf.
	pub claimant: AccountOrPerson<AccountId>,
	/// The credit awarded.
	pub credit: NftClaimCredit,
}

/// The fields a block's `NftClaimCreditTree` carries besides the root, recorded when the
/// block's first credit is awarded and read back when the root is computed.
///
/// They are kept alongside the leaves rather than derived when the root is computed: the game can
/// be over by then, so its index is no longer readable.
#[derive(
	Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Debug, Clone, PartialEq, Eq,
)]
pub struct NftClaimCreditRootInfo {
	/// The game the block's credits were awarded in.
	pub game_index: GameIdx,
	/// The block's wall-clock time in seconds since the UNIX epoch.
	pub timestamp: u32,
	/// The private claim slots the game grants, `0` for a public game.
	pub private_slots: PrivateClaimSlot,
}

/// The inclusion proof of one NFT claim credit against the `NftClaimCreditTree` of the block it
/// was awarded in, as returned by [`crate::Pallet::nft_claim_credit_proofs`].
///
/// Everything Asset Hub needs to verify one claim, so a wallet forwards it as it is.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Debug, Clone, PartialEq, Eq)]
pub struct NftClaimCreditProof {
	/// The root the proof verifies against, as recorded for the award block.
	pub root: CreditProofNode,
	/// The credit being claimed, which the claimant sends along so that Asset Hub can recompute
	/// [`Self::leaf`] and see who may mint.
	pub credit: NftClaimCredit,
	/// The leaf being proven, `blake2_256(claimant ++ credit)`.
	pub leaf: NftClaimCreditLeaf,
	/// The position of `leaf` in the block's leaves, in award order.
	pub leaf_index: u32,
	/// The number of leaves the tree was built over, which the verifier needs to rehash an odd
	/// layer the same way the root was computed over.
	pub leaf_count: u32,
	/// The sibling hashes that rehash `leaf` up to `root`, bottom layer first.
	pub proof: Vec<CreditProofNode>,
}

/// Why no [`NftClaimCreditProof`] could be built for a claim.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Debug, Clone, PartialEq, Eq)]
pub enum NftClaimCreditProofError {
	/// The block has no `NftClaimCreditTree`, so it awarded no credit.
	UnknownAwardBlock,
	/// The block's awards are no longer on chain, its root having dropped out of the retained
	/// window. The awards have to be supplied from the block's `NftClaimCreditAwarded` events
	/// instead.
	AwardsPruned,
	/// The given awards are not as many as the block's root was computed over.
	LeafCountMismatch {
		/// The number of leaves the root was computed over.
		expected: u32,
	},
	/// `leaf_index` is not a leaf of the block's tree.
	LeafIndexOutOfBounds,
	/// The given awards rehash to a different root than the one recorded for the block, so they
	/// are not the block's awards, or not in award order.
	RootMismatch,
}

/// One game's private claim path, from its first awarded credit until its ring is delivered and
/// its registration state is dropped.
///
/// It is copied from the game rather than read back from it, because a game is killed once its
/// player process ends, which is before registration opens.
#[derive(
	Encode,
	Decode,
	DecodeWithMemTracking,
	MaxEncodedLen,
	TypeInfo,
	Debug,
	Clone,
	Copy,
	PartialEq,
	Eq,
)]
pub struct PrivateGameInfo {
	/// The slots every registrant holds, from the game's schedule. Each slot is one mint hidden
	/// by the game's ring.
	pub slots: PrivateClaimSlot,
	/// When registration opens, in seconds since the UNIX epoch. It is the end of the game's
	/// player process, when the credits are final. Before that, a claimant's balance need not
	/// cover the entry price.
	pub registration_starts: u32,
	/// When registration closes, in seconds since the UNIX epoch. Building starts after it, so a
	/// ring never grows under a claimant who already proved against it.
	pub registration_ends: u32,
	/// The number of keys registered, which is the anonymity set of every claim of the game.
	pub key_count: u32,
	/// How many claimants earned the credits one registration costs. It is the population the
	/// game's registration is measured against, and it is final once the player process ends.
	/// A claimant below the price cannot register, so counting every credited player instead
	/// would put the floor out of reach.
	pub eligible_players: u32,
	/// What the game still owes: its ring to build, or its registration state to drop.
	pub phase: PrivateGamePhase,
}

/// The work a private game has left.
#[derive(
	Encode,
	Decode,
	DecodeWithMemTracking,
	MaxEncodedLen,
	TypeInfo,
	Debug,
	Clone,
	Copy,
	PartialEq,
	Eq,
)]
pub enum PrivateGamePhase {
	/// The game's ring is being built. It is finished once every registered key is pushed.
	Building {
		/// How many of the registered keys are already pushed.
		included: u32,
		/// How many build steps failed in a row. A push fails on a chunk the ring cannot be
		/// built from, which no retry repairs, so the game is abandoned once the count reaches
		/// `PRIVATE_RING_BUILD_RETRIES`.
		failures: u8,
	},
	/// The game reached its outcome, so the keys, the registrations and the unspent credits are
	/// left to drop.
	CleaningUp,
}

impl PrivateGameInfo {
	/// Whether registration is still open at `now`.
	pub fn accepts_keys(&self, now: u32) -> bool {
		now >= self.registration_starts && now < self.registration_ends
	}
}
