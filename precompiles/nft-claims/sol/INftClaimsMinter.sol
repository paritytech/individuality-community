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

pragma solidity ^0.8.30;

/// @title NFT Claims Minter
/// @notice Collection-minter registration for NFT claims, at one fixed precompile address.
/// @dev A Scarcity collection accepts deposit-free claim minting only after its owner registers
/// it with an item selection. The mutators dispatch as the caller, so only the collection's
/// current Scarcity owner can register or withdraw it. This surface is separate from the Scarcity
/// precompile because `indiv-pallet-scarcity` is a standalone base layer and not every runtime
/// that has it also has `pallet-nft-claims`.
///
/// Two conditions apply to every function and are not repeated below: native value attached to a
/// call reverts, and a delegate call reverts.
/// @custom:reverts "this precompile does not accept value"
/// @custom:reverts "illegal to call this pre-compile via delegate call"
/// @custom:security-contact admin@parity.io
interface INftClaimsMinter {
    /// @notice Register the caller's `collection` with pseudo-random item selection.
    /// @dev Each claim mints the item the credit maps to modulo the collection's next item index.
    /// Define every item before claims open, because a later one raises that index and re-maps
    /// every unclaimed credit. Delete none, because the index never shrinks, so the credits
    /// drawing a deleted item fail while the rest keep working.
    /// @param collection The Scarcity collection to register.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown collection"
    function setRandomMinter(uint32 collection) external;

    /// @notice Register the caller's `collection` with `minter` picking the item per claim.
    /// @dev `minter` must have contract code deployed, otherwise the runtime's collection
    /// selector rejects it and its reason is forwarded as the revert. Nothing guarantees the code
    /// implements `mint(uint32,bytes32) returns (uint32)`, so a wrong contract fails claims
    /// instead of failing here.
    /// @param collection The Scarcity collection to register.
    /// @param minter The contract that picks the item for each claim.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown collection"
    function setContractMinter(uint32 collection, address minter) external;

    /// @notice Withdraw the caller's `collection` from claims.
    /// @dev Nothing already claimed is undone; withdrawing an unregistered collection owned by the
    /// caller is a successful no-op.
    /// @param collection The Scarcity collection to withdraw.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown collection"
    function clearMinter(uint32 collection) external;

    /// @notice The stored claim registration of `collection`.
    /// @dev Reports what is stored, not whether a claim will mint: a claim also needs `owner` to
    /// still be the collection's current Scarcity owner, kind 2's `minter` to still carry code,
    /// and the drawn item to still exist. An unknown collection answers as unregistered rather
    /// than reverting.
    /// @param collection The Scarcity collection to read.
    /// @return kind 0 = unregistered, 1 = pseudo-random, 2 = contract-selected.
    /// @return minter The picking contract for kind 2, the zero address otherwise.
    /// @return owner The account that registered the collection, or the zero address when
    /// unregistered.
    function collectionMinter(uint32 collection) external view returns (uint8 kind, address minter, address owner);
}
