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

//! Runtime API definition for the NFT claim credits.
//!
//! What a wallet needs to mint: which roots a claimant holds credits under, and the inclusion proof
//! of each of those credits. Both are reads, and a whole block's awards inside an extrinsic's proof
//! would be paid for by every other extrinsic in the block, so neither is a call.

use crate::{NftClaimCreditAward, NftClaimCreditProof, NftClaimCreditProofError};
use alloc::vec::Vec;
use codec::Codec;
use indiv_support::{credit_trees::NftClaimCreditTree, identity::AccountOrPerson};

sp_api::decl_runtime_apis! {
	/// The API a wallet mints an NFT claim credit through.
	pub trait NftCreditsApi<AccountId, BlockNumber>
	where
		AccountId: Codec,
		BlockNumber: Codec,
	{
		/// Returns the NFT claim credit roots `claimant` has at least one credit under, keyed by
		/// the block that committed those credits, in ascending block order.
		///
		/// A wallet starts here and asks `nft_claim_credit_proofs` about each block returned. A
		/// block gets its root only in the block after it, so the newest one a claimant's credit
		/// landed in is left out until then, and a credit that spilled into a later buffer waits
		/// for that buffer's tree.
		fn nft_claim_credit_roots(
			claimant: AccountOrPerson<AccountId>,
		) -> Vec<(BlockNumber, NftClaimCreditTree)>;

		/// Returns the inclusion proof of each credit of `claimant` that `tree_block` committed,
		/// which is what Asset Hub verifies a mint against.
		///
		/// Everything comes from chain state, so a caller needs no event history: the proofs carry
		/// the credit, the leaf, its index, the leaf count, the root and the sibling hashes.
		/// Empty if the block committed nothing of `claimant`'s.
		///
		/// Only blocks whose awards are still retained can be served. An older one gives
		/// `NftClaimCreditProofError::AwardsPruned`, and has to go through
		/// `nft_claim_credit_proof_from_awards`.
		///
		/// One call rebuilds the block's tree for each proof it returns, and a node serves this
		/// unmetered, so an operator exposing it publicly must rate-limit RPC.
		fn nft_claim_credit_proofs(
			tree_block: BlockNumber,
			claimant: AccountOrPerson<AccountId>,
		) -> Result<Vec<NftClaimCreditProof>, NftClaimCreditProofError>;

		/// Returns the inclusion proof of the credit at `leaf_index` of `tree_block`, from
		/// `awards` supplied by the caller.
		///
		/// The fallback for a block whose awards are no longer retained: `awards` are every credit
		/// the block committed, in leaf order, rebuilt from the `NftClaimCreditAwarded` events
		/// naming that block, which are emitted in it or in an earlier one. Leaf hashing and the
		/// tree layout stay in the runtime, and the recomputed root
		/// is checked against the recorded one, so awards that are incomplete or out of order
		/// fail with `NftClaimCreditProofError::RootMismatch` rather than yielding a proof Asset
		/// Hub rejects.
		fn nft_claim_credit_proof_from_awards(
			tree_block: BlockNumber,
			awards: Vec<NftClaimCreditAward<AccountId>>,
			leaf_index: u32,
		) -> Result<NftClaimCreditProof, NftClaimCreditProofError>;
	}
}
