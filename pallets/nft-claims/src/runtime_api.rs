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

//! Runtime API definition for previewing NFT claim selections and reading the status of credits.

use alloc::vec::Vec;
use codec::{Decode, DecodeWithMemTracking, Encode};
use indiv_pallet_scarcity::{CollectionId, ItemIndex};
use indiv_support::credit_trees::{CreditTreeBlock, NftClaimCredit};
use scale_info::TypeInfo;
use sp_core::{ConstU32, H160};
use sp_runtime::{BoundedVec, DispatchError};

/// Maximum number of mint selections in one runtime API request.
pub const MAX_PREVIEW_QUERIES: u32 = 32;

/// A preview batch, bounded so an oversized request fails at decode.
pub type PreviewQueries = BoundedVec<PreviewQuery, ConstU32<MAX_PREVIEW_QUERIES>>;

/// One credit and collection whose mint selection is previewed.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub struct PreviewQuery {
	/// The credit whose claim is previewed, read by the Random draw or the minter contract.
	pub credit: NftClaimCredit,
	/// The registered collection the claim would mint into.
	pub collection: CollectionId,
}

/// The registered mechanism that selected a previewed item.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub enum SelectionKind {
	/// The credit was reduced modulo the collection's next item index.
	Random,
	/// The registered minter contract selected the item.
	Contract(H160),
}

/// A failure encountered by the real claim selection path.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub enum PreviewFailure {
	/// The collection has no claim registration.
	CollectionNotRegistered,
	/// The registered collection no longer exists.
	UnknownCollection,
	/// The collection owner differs from the owner who registered claims. Only a runtime that
	/// keeps registrations across a handover reports this.
	CollectionOwnerChanged,
	/// Random selection has no allocated item index to draw from.
	NoItems,
	/// The selected item definition has been deleted or does not exist.
	UnknownItem { item: ItemIndex },
	/// The registered contract rejected, trapped or returned an invalid result.
	ContractSelectionFailed { error: DispatchError },
}

/// The item a claim would mint or the selection failure it would return.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub enum PreviewOutcome {
	/// Selection succeeded and the named item definition exists.
	Mints { item: ItemIndex, via: SelectionKind },
	/// Selection failed before minting.
	Fails { reason: PreviewFailure },
}

/// Maximum number of credits in one status request.
pub const MAX_STATUS_QUERIES: u32 = 256;

/// A status batch, bounded so an oversized request fails at decode.
pub type CreditStatusQueries = BoundedVec<CreditStatusQuery, ConstU32<MAX_STATUS_QUERIES>>;

/// A credit to read the status of, specified as a claim specifies it.
///
/// The game chain's `NftCreditsApi::nft_claim_credit_proofs` returns the leaf index of each
/// credit of a claimant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub struct CreditStatusQuery {
	/// The game chain block whose tree commits to the credit.
	pub block: CreditTreeBlock,
	/// The position of the credit's leaf in that tree.
	pub leaf_index: u32,
}

/// The status of a credit on this chain.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode, DecodeWithMemTracking, TypeInfo)]
pub enum CreditStatus {
	/// This chain holds neither the block's tree nor a claim of the leaf.
	///
	/// The tree has not arrived yet, or the expiry sweep removed it. The game chain's
	/// `NftCreditsApi::nft_claim_credit_blocks` tells the two apart.
	NotStored,
	/// The tree is stored and the leaf is not claimed.
	///
	/// `expires_at` is the deadline in seconds since the UNIX epoch. A claim stays possible after
	/// it until the expiry sweep removes the tree.
	Claimable { expires_at: u64 },
	/// The leaf is claimed.
	///
	/// The status stays after the last claim of a tree removes the tree, until the expiry sweep.
	Claimed,
	/// The block's tree has no leaf at `leaf_index`.
	LeafIndexOutOfBounds,
}

sp_api::decl_runtime_apis! {
	/// Read-only previews of the item each NFT claim credit selects, and the status of credits.
	#[api_version(2)]
	pub trait NftClaimsApi {
		/// Executes the claim selector once per query and returns positionally aligned outcomes.
		/// Every query runs in its own discarded storage layer, so each one is previewed
		/// independently against the block's state and none observes another's contract writes.
		/// A preview cannot bind a later claim, which runs against whatever state its block holds.
		fn preview_mints(queries: PreviewQueries) -> Vec<PreviewOutcome>;

		/// Returns the status of each queried credit, positionally aligned with the queries.
		#[api_version(2)]
		fn credit_statuses(queries: CreditStatusQueries) -> Vec<CreditStatus>;
	}
}
