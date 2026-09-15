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
use frame_support::{
	pallet_prelude::{BoundedVec, Get},
	CloneNoBound, DebugNoBound, EqNoBound, PartialEqNoBound,
};
use indiv_pallet_game::GameIdx;
use indiv_support::{
	credit_trees::{ClaimPath, CreditProofNode, NftClaimCredit, PrivateClaimTier},
	identity::AccountOrPerson,
};
use scale_info::TypeInfo;
use sp_runtime::Saturating;

/// One credit of one claimant in one game, as a position in [`AwardedCredits`], derived from
/// the round and the [`indiv_pallet_game::AttesterPosition`] by `Pallet::credit_slot`.
pub type CreditSlot = u32;

/// One chunk of a block's awards in [`crate::NftClaimCreditAwards`], as the second key of that map.
/// Chunk `c` holds the awards at leaf indices `c * AWARDS_PER_CHUNK` upwards, so it is
/// `leaf_index / AWARDS_PER_CHUNK` for the award being written or read.
pub type ChunkIndex = u32;

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

/// One NFT claim credit as its tree block committed it, which is the preimage of one
/// [`indiv_support::credit_trees::NftClaimCreditLeaf`].
///
/// Kept per tree block in [`crate::NftClaimCreditAwards`] for as long as the block's awards are
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

/// One buffer of awards waiting for its tree, held in [`crate::CreditBuffers`]: the fields the
/// `NftClaimCreditTree` built over it carries, and its running award count.
///
/// Written when the buffer's first credit is awarded, updated by every award after it and read back
/// when the root is computed. The game index is kept here rather than derived at that point,
/// because the game can be over by then.
#[derive(
	Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo, Debug, Clone, PartialEq, Eq,
)]
pub struct CreditBuffer {
	/// The game every credit in the buffer was awarded in. A tree carries one game index, so a
	/// credit of another game starts a buffer of its own.
	pub game_index: GameIdx,
	/// The wall-clock time of the buffer's first award, in seconds since the UNIX epoch.
	pub timestamp: u32,
	/// How many awards the buffer holds, which is the leaf index the next one takes. Counted here
	/// so that awarding reads no chunk to find the buffer's end.
	pub awards: u32,
	/// Which path the buffer's game mints on, which its tree carries.
	pub claim_path: ClaimPath,
}

/// The inclusion proof of one NFT claim credit against the `NftClaimCreditTree` of the block that
/// committed it, as returned by [`crate::Pallet::nft_claim_credit_proofs`].
///
/// Carries only what the claims chain accepts from the claimant.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Debug, Clone, PartialEq, Eq)]
pub struct NftClaimCreditProof {
	/// The credit being claimed. The verifier hashes it with the claimant it authenticated to get
	/// the leaf, so somebody else's credit builds a different leaf and does not rehash to the
	/// stored root.
	pub credit: NftClaimCredit,
	/// The position of the credit's leaf in the block's leaves, in award order.
	pub leaf_index: u32,
	/// The sibling hashes that rehash the leaf up to the block's root, bottom layer first.
	pub proof: Vec<CreditProofNode>,
}

/// Why no [`NftClaimCreditProof`] could be built for a claim.
#[derive(Encode, Decode, DecodeWithMemTracking, TypeInfo, Debug, Clone, PartialEq, Eq)]
pub enum NftClaimCreditProofError {
	/// The block has no `NftClaimCreditTree`, so it committed no credit.
	UnknownCreditTree,
	/// The block's awards are no longer on chain, its root having dropped out of the retained
	/// window. The awards have to be supplied from the `NftClaimCreditAwarded` events naming the
	/// block instead.
	AwardsPruned,
	/// The given awards are not as many as the block's root was computed over.
	LeafCountMismatch {
		/// The number of leaves the root was computed over.
		expected: u32,
	},
	/// `leaf_index` is not a leaf of the block's tree.
	LeafIndexOutOfBounds,
	/// The given awards rehash to a different root than the one recorded for the block, so they
	/// are not the block's awards, or not in leaf order.
	RootMismatch,
}

/// What one tier of a private game's ring ladder is measured against.
///
/// Tier `t` is the ring of everyone that earned at least `t` credits. `eligible` is the population
/// that could register for it and `registered` is how many of them did, so the two decide whether
/// the tier hides anyone. Both are counted as they land, which is before the ladder is built.
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
pub struct PrivateTierCounts {
	/// Claimants that earned exactly this tier's credit count. The population of tier `t` is the
	/// sum of this from `t` upwards.
	pub eligible: u32,
	/// Registrants whose credit count is exactly this tier's, which is the length of the tier's
	/// key bucket. The ring of tier `t` is the sum of this from `t` upwards.
	pub registered: u32,
}

/// One game's private claim path, from its first awarded credit until its ladder is delivered and
/// its registration state is dropped.
///
/// It is copied from the game rather than read back from it, because a game is killed once its
/// player process ends, which is before registration opens.
#[derive(
	CloneNoBound,
	PartialEqNoBound,
	EqNoBound,
	Encode,
	Decode,
	DecodeWithMemTracking,
	MaxEncodedLen,
	DebugNoBound,
	TypeInfo,
)]
#[scale_info(skip_type_params(MaxTiers))]
pub struct PrivateGameInfo<MaxTiers: Get<u32>> {
	/// When key registration opens, in seconds since the UNIX epoch. It is the end of the game's
	/// player process, when the credits are final. Before then a claimant's credit count does not
	/// name the tier they would register into.
	pub key_registration_starts: u32,
	/// When key registration closes, in seconds since the UNIX epoch. Building starts after it, so
	/// no ring grows under a claimant who already proved against it.
	pub key_registration_ends: u32,
	/// The number of keys registered, which is the ring of tier 1 and the widest anonymity set the
	/// game offers.
	pub key_count: u32,
	/// One entry per credit count, tier 1 first. A claimant that earns more credits than the
	/// vector holds is counted in its last tier.
	pub tiers: BoundedVec<PrivateTierCounts, MaxTiers>,
	/// The stage the game has reached, from `Registering` to `CleaningUp`. It only moves forward,
	/// and the entry is removed once `CleaningUp` finishes.
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
	/// The game is taking registrations or waiting for the window to close. The build opens the
	/// ladder from here, which is where its height is decided.
	Registering,
	/// The game's ladder is being built, from its top tier down. Each tier is the tier above it
	/// plus its own bucket of keys, so one pass over the keys builds every ring.
	Building {
		/// The tier being pushed. The ladder is finished when tier 1 is closed.
		tier: PrivateClaimTier,
		/// How many of the tier's own keys are already pushed.
		included: u32,
		/// How many build steps failed in a row. A push fails on a chunk the ring cannot be built
		/// from, which no retry repairs, so the game is abandoned once the count reaches
		/// `PRIVATE_RING_BUILD_RETRIES`.
		failures: u8,
	},
	/// The game reached its outcome and the claims chain does not hold it yet.
	/// [`crate::PrivateOutcomes`] holds what is owed.
	Delivering,
	/// The claims chain holds the game's outcome, so the keys, the registrations and the unspent
	/// credits are left to drop.
	CleaningUp,
}

/// What a claimant of a private game has done with the credits they earned.
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
pub enum PrivateClaimantState {
	/// The claimant holds credits of the game and may register a key. The count names the tier
	/// their key goes into and is final once the player process ends. It stops at the ladder's
	/// top, the credits above it being forfeit.
	Eligible { credits: u32 },
	/// The claimant registered a key into the tier their credits named, so a second registration
	/// of theirs is refused.
	Registered { credits: u32 },
}

impl PrivateClaimantState {
	/// The credits the claimant earned, whether or not they have registered.
	pub fn credits(&self) -> u32 {
		match self {
			Self::Eligible { credits } | Self::Registered { credits } => *credits,
		}
	}
}

impl<MaxTiers: Get<u32>> PrivateGameInfo<MaxTiers> {
	/// Whether key registration is still open at `now`.
	pub fn accepts_keys(&self, now: u32) -> bool {
		now >= self.key_registration_starts && now < self.key_registration_ends
	}

	/// The ring of tier `tier`, which is every registrant that earned at least that many credits.
	///
	/// Tiers are one-based, so tier 1 is every registrant. A tier above what the vector holds has
	/// no ring and reports zero.
	pub fn ring_size(&self, tier: PrivateClaimTier) -> u32 {
		self.tier_suffix(tier, |counts| counts.registered)
	}

	/// The claimants that earned at least `tier` credits, which is the population tier `tier`'s
	/// ring is measured against.
	pub fn eligible_at(&self, tier: PrivateClaimTier) -> u32 {
		self.tier_suffix(tier, |counts| counts.eligible)
	}

	/// The sum of `count` over every tier at or above `tier`.
	fn tier_suffix(
		&self,
		tier: PrivateClaimTier,
		count: impl Fn(&PrivateTierCounts) -> u32,
	) -> u32 {
		self.tiers
			.iter()
			.skip(usize::from(tier).saturating_sub(1))
			.map(count)
			.fold(0u32, |sum, held| sum.saturating_add(held))
	}

	/// Record one more claimant that earned exactly `credits` and one fewer at `credits - 1`.
	///
	/// A credit count above what the vector holds is counted in its last tier. Returns whether the
	/// counts changed.
	pub fn note_tier_credit(&mut self, credits: u32) -> bool {
		let Some(reached) = self.tier_index(credits) else {
			return false;
		};
		// A claimant that earned more credits than the vector holds stays in its last tier, so the
		// award moves nobody.
		if self.tier_index(credits.saturating_sub(1)) == Some(reached) {
			return false;
		}

		self.tiers[reached].eligible.saturating_inc();
		if let Some(left) = self.tier_index(credits.saturating_sub(1)) {
			self.tiers[left].eligible = self.tiers[left].eligible.saturating_sub(1);
		}
		true
	}

	/// Record one more registrant that earned exactly `credits`.
	pub fn note_tier_registration(&mut self, credits: u32) {
		if let Some(tier) = self.tier_index(credits) {
			self.tiers[tier].registered.saturating_inc();
		}
	}

	/// The position in [`Self::tiers`] that a claimant of `credits` is counted at, `None` for a
	/// claimant that earned nothing.
	fn tier_index(&self, credits: u32) -> Option<usize> {
		let last = self.tiers.len().checked_sub(1)?;
		(credits > 0).then(|| usize::try_from(credits - 1).unwrap_or(last).min(last))
	}
}
