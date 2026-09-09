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

/// @title IScarcityCollection - Scarcity collection ERC-721 precompile
/// @notice ERC-721 view of one Scarcity collection, plus the collection owner's admin surface
/// and the Scarcity-specific reads.
/// @dev Each collection is its own virtual contract: the collection id is encoded big-endian in
/// the first four bytes of the precompile address. The ERC-721 subset keeps the exact standard
/// signatures (and therefore selectors); the admin and Scarcity-specific functions are this
/// precompile's own surface. Due to ABI-generation constraints the ERC-165, ERC-721,
/// ERC-721 Metadata, ERC-5192, ERC-2981, ERC-4906 and ERC-7572 surfaces are merged into this
/// single interface.
///
/// The per-function documentation describes live collections only. Three conditions apply to
/// every function and are not repeated below: an address whose collection was never created or
/// has been deleted reverts, native value attached to a call reverts, and a delegate call
/// reverts.
///
/// A holder moves a token on its own authority: the caller must be the current holder.
/// Approvals are not supported, so @custom:function approve and @custom:function setApprovalForAll revert, @custom:function getApproved
/// returns the zero address and @custom:function isApprovedForAll returns false.
/// @custom:reverts "unknown collection"
/// @custom:reverts "this precompile does not accept value"
/// @custom:reverts "illegal to call this pre-compile via delegate call"
/// @custom:security-contact admin@parity.io
interface IScarcityCollection {
    /// @notice ERC-721 standard event. Emitted by @custom:function mint, @custom:function transferFrom, @custom:function safeTransferFrom,
    /// @custom:function forceTransfer and @custom:function forceBurn when `tokenId` moves between purse keys, including mints
    /// (from the zero address) and burns (to the zero address).
    /// @param from The purse key the instance left, or the zero address on a mint.
    /// @param to The purse key the instance arrived at, or the zero address on a burn.
    /// @param tokenId The permanent instance identifier that moved.
    event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);

    /// @notice ERC-721 standard event, declared for ABI completeness and never emitted because
    /// @custom:function approve always reverts, so no approval can come into being.
    /// @param owner The token owner that would have granted the approval.
    /// @param approved The account that would have been approved.
    /// @param tokenId The instance the approval would have covered.
    event Approval(address indexed owner, address indexed approved, uint256 indexed tokenId);

    /// @notice ERC-721 standard event, declared for ABI completeness and never emitted because
    /// @custom:function setApprovalForAll always reverts, so no operator approval can come into being.
    /// @param owner The token owner that would have granted the operator approval.
    /// @param operator The account that would have been approved as operator.
    /// @param approved The approval flag that would have been set.
    event ApprovalForAll(address indexed owner, address indexed operator, bool approved);

    /// @notice ERC-4906 metadata-update event. Emitted by @custom:function setInstanceMetadata and
    /// @custom:function removeInstanceMetadata when a write changes what @custom:function tokenURI returns for one instance.
    /// @param tokenId The instance whose token URI changed.
    event MetadataUpdate(uint256 tokenId);

    /// @notice ERC-4906 metadata-update event. Emitted by @custom:function setCollectionMetadata,
    /// @custom:function removeCollectionMetadata, @custom:function setItemMetadata and @custom:function removeItemMetadata when a write changes
    /// what @custom:function tokenURI returns for more than one instance, over the inclusive token-id range
    /// `fromTokenId` to `toTokenId`.
    /// @dev A collection- or item-scope write resolves into instances that cannot be enumerated on
    /// chain, so the range is the whole `uint256` space and the log's source address names the
    /// collection.
    /// @param fromTokenId The first token id in the affected range.
    /// @param toTokenId The last token id in the affected range.
    event BatchMetadataUpdate(uint256 fromTokenId, uint256 toTokenId);

    /// @notice ERC-5192 lock event. Emitted by @custom:function mint when it creates a token whose item is
    /// soulbound.
    /// @dev Transferability is fixed when the item is defined, so a token's status never changes
    /// after this.
    /// @param tokenId The soulbound instance.
    event Locked(uint256 tokenId);

    /// @notice ERC-5192 lock event. Emitted by @custom:function mint when it creates a token whose item is
    /// transferable.
    /// @dev Final, like @custom:emits Locked.
    /// @param tokenId The transferable instance.
    event Unlocked(uint256 tokenId);

    /// @notice ERC-7572 contract-metadata event. Emitted by @custom:function setCollectionMetadata and
    /// @custom:function removeCollectionMetadata when a write to the reserved collection key `contractURI` changes
    /// what @custom:function contractURI returns.
    /// @dev Carries no arguments, per ERC-7572: a consumer refetches the document rather than
    /// reading the new value from the log.
    event ContractURIUpdated();

    /// @notice This precompile's own event, not part of any ERC standard. Emitted by
    /// @custom:function claimCollectionOwnership when it moves ownership.
    /// @dev Nomination emits nothing, because it moves no authority.
    /// @param previousOwner The account that owned the collection before the claim.
    /// @param newOwner The account that owns the collection after the claim.
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    /// @notice This precompile's own event, not part of any ERC standard. Emitted by @custom:function deleteItem
    /// when it removes an item definition.
    /// @dev The index is never reused.
    /// @param item The removed item index.
    event ItemDeleted(uint32 indexed item);

    /// @notice This precompile's own event, not part of any ERC standard. Emitted by
    /// @custom:function deleteCollection when it removes this collection.
    /// @dev Every function on this address reverts afterwards, and the id is never reused.
    event CollectionDeleted();

    /// @notice Reports whether this contract implements the interface identified by `interfaceId`.
    /// @dev ERC-165 standard function. True for the ERC-165 (0x01ffc9a7), ERC-721
    /// (0x80ac58cd), ERC-721 Metadata (0x5b5e139f), ERC-5192 (0xb45a3c0e), ERC-2981
    /// (0x2a55205a) and ERC-4906 (0x49064906) identifiers. ERC-721 Enumerable (0x780e9d63)
    /// is not claimed: @custom:function tokenOfOwnerByIndex is served but `totalSupply` and
    /// `tokenByIndex` are not.
    /// @param interfaceId The 4-byte ERC-165 interface identifier to query.
    /// @return supported True when `interfaceId` is one of the supported interfaces.
    function supportsInterface(bytes4 interfaceId) external view returns (bool supported);

    /// @notice Number of tokens of this collection held by `owner`, always 0 or 1.
    /// @dev ERC-721 standard function. A purse key holds at most one instance. Minting registers
    /// its destination, so a holder that was minted to answers correctly whatever its balance. A
    /// key that received its instance by transfer instead, or whose account has since been reaped,
    /// answers 0 unless it is registered for some other reason, because its address cannot
    /// otherwise be resolved back; prefer @custom:function ownerOf.
    /// @param owner The address to query.
    /// @return balance 1 when `owner` holds an instance of this collection, otherwise 0.
    /// @custom:reverts "balance query for the zero address"
    function balanceOf(address owner) external view returns (uint256 balance);

    /// @notice The purse key holding `tokenId`.
    /// @dev ERC-721 standard function. The address is stable and correct for every holder, unlike
    /// @custom:function balanceOf, so prefer this to establish ownership.
    /// @param tokenId The instance to look up.
    /// @return owner The purse key currently holding `tokenId`.
    /// @custom:reverts "unknown token"
    function ownerOf(uint256 tokenId) external view returns (address owner);

    /// @notice Move `tokenId` to the empty purse key `to` on the caller's holder authority, then
    /// require a code-carrying `to` to acknowledge the token.
    /// @dev ERC-721 standard function. The caller must be the current holder. `data` is forwarded
    /// to the receiver callback, which this precompile cannot make yet, so a code-carrying `to`
    /// always reverts. The acknowledgement runs after the move and unwinds it on failure.
    /// @param from The current holder.
    /// @param to The empty purse key to move the instance to.
    /// @param tokenId The instance to move.
    /// @param data Forwarded to the receiver callback unread.
    /// @custom:reverts "destination is the zero address"
    /// @custom:reverts "unknown token"
    /// @custom:reverts "transfer from the wrong holder"
    /// @custom:reverts "caller does not hold this token: transfers on another holder's authority need approvals, which are not supported yet"
    /// @custom:reverts "destination already holds this instance"
    /// @custom:reverts "destination purse already holds an instance"
    /// @custom:reverts "token is soulbound to its purse key"
    /// @custom:reverts "instance state nonce exhausted"
    /// @custom:reverts "safe transfer to a contract is not supported yet: the receiver callback is unavailable"
    /// @custom:emits Transfer
    function safeTransferFrom(address from, address to, uint256 tokenId, bytes calldata data) external;

    /// @notice As the four-argument @custom:function safeTransferFrom, with empty `data`.
    /// @dev ERC-721 standard function.
    /// @param from The current holder.
    /// @param to The empty purse key to move the instance to.
    /// @param tokenId The instance to move.
    /// @custom:reverts "destination is the zero address"
    /// @custom:reverts "unknown token"
    /// @custom:reverts "transfer from the wrong holder"
    /// @custom:reverts "caller does not hold this token: transfers on another holder's authority need approvals, which are not supported yet"
    /// @custom:reverts "destination already holds this instance"
    /// @custom:reverts "destination purse already holds an instance"
    /// @custom:reverts "token is soulbound to its purse key"
    /// @custom:reverts "instance state nonce exhausted"
    /// @custom:reverts "safe transfer to a contract is not supported yet: the receiver callback is unavailable"
    /// @custom:emits Transfer
    function safeTransferFrom(address from, address to, uint256 tokenId) external;

    /// @notice Move `tokenId` to the empty purse key `to` on the caller's holder authority.
    /// @dev ERC-721 standard function. The caller must be the current holder. Unlike
    /// @custom:function safeTransferFrom, a code-carrying `to` is accepted without an
    /// acknowledgement.
    /// @param from The current holder.
    /// @param to The empty purse key to move the instance to.
    /// @param tokenId The instance to move.
    /// @custom:reverts "destination is the zero address"
    /// @custom:reverts "unknown token"
    /// @custom:reverts "transfer from the wrong holder"
    /// @custom:reverts "caller does not hold this token: transfers on another holder's authority need approvals, which are not supported yet"
    /// @custom:reverts "destination already holds this instance"
    /// @custom:reverts "destination purse already holds an instance"
    /// @custom:reverts "token is soulbound to its purse key"
    /// @custom:reverts "instance state nonce exhausted"
    /// @custom:emits Transfer
    function transferFrom(address from, address to, uint256 tokenId) external;

    /// @notice Always reverts: the purse model has no per-token approval mechanism.
    /// @dev ERC-721 standard function. See @custom:function getApproved.
    /// @param to Unused; the call reverts before reading it.
    /// @param tokenId Unused; the call reverts before reading it.
    /// @custom:reverts "approvals are not supported yet: the purse model has no approval mechanism"
    function approve(address to, uint256 tokenId) external;

    /// @notice Always reverts: the purse model has no operator approval mechanism.
    /// @dev ERC-721 standard function. See @custom:function isApprovedForAll.
    /// @param operator Unused; the call reverts before reading it.
    /// @param approved Unused; the call reverts before reading it.
    /// @custom:reverts "approvals are not supported yet: the purse model has no approval mechanism"
    function setApprovalForAll(address operator, bool approved) external;

    /// @notice Always returns the zero address, because @custom:function approve always reverts.
    /// @dev ERC-721 standard function.
    /// @param tokenId The instance to query.
    /// @return operator Always the zero address.
    /// @custom:reverts "unknown token"
    function getApproved(uint256 tokenId) external view returns (address operator);

    /// @notice Always returns false, because @custom:function setApprovalForAll always reverts.
    /// @dev ERC-721 standard function. Never reverts on an unknown token, unlike
    /// @custom:function getApproved.
    /// @param owner Unused; retained for ERC-721 signature compatibility.
    /// @param operator Unused; retained for ERC-721 signature compatibility.
    /// @return approved Always false.
    function isApprovedForAll(address owner, address operator) external view returns (bool approved);

    /// @notice Collection name, from the reserved metadata key `name`, or the empty string.
    /// @dev ERC-721 Metadata standard function. The pallet stores bytes, so a value that is not
    /// UTF-8 decodes with replacement characters rather than failing the call.
    /// @return collectionName The decoded collection name, or the empty string when unset.
    function name() external view returns (string memory collectionName);

    /// @notice Collection symbol, from the reserved metadata key `symbol`, or the empty string.
    /// @dev ERC-721 Metadata standard function. Decodes lossily like @custom:function name.
    /// @return collectionSymbol The decoded collection symbol, or the empty string when unset.
    function symbol() external view returns (string memory collectionSymbol);

    /// @notice Token URI for `tokenId`, from the reserved key `tokenURI` resolved instance, then
    /// item, then collection scope, or the empty string.
    /// @dev ERC-721 Metadata standard function. Decodes lossily like @custom:function name.
    /// @param tokenId The instance to resolve the URI for.
    /// @return uri The decoded token URI, or the empty string when unset.
    /// @custom:reverts "unknown token"
    function tokenURI(uint256 tokenId) external view returns (string memory uri);

    /// @notice The token `owner` holds, for `index` 0 only.
    /// @dev The only ERC-721 Enumerable function served, so the interface is not claimed. A purse
    /// key holds at most one instance, so every index above 0 is out of range. Resolving `owner`
    /// has the same limitation as @custom:function balanceOf. Served for wallets.
    /// @param owner The holder to query.
    /// @param index The index into the holder's tokens; only 0 is valid.
    /// @return tokenId The token held at index 0.
    /// @custom:reverts "balance query for the zero address"
    /// @custom:reverts "token index out of range for this owner"
    function tokenOfOwnerByIndex(address owner, uint256 index) external view returns (uint256 tokenId);

    /// @notice True when `tokenId` cannot be moved on its holder's authority.
    /// @dev ERC-5192 standard function. The collection owner's @custom:function forceTransfer and
    /// @custom:function forceBurn ignore this, so a locked token can still be moved or destroyed by
    /// the owner.
    /// @param tokenId The instance to query.
    /// @return isLocked True when the instance is soulbound.
    /// @custom:reverts "unknown token"
    function locked(uint256 tokenId) external view returns (bool isLocked);

    /// @notice Royalty recipient and amount owed on a sale of `tokenId` at `salePrice`.
    /// @dev ERC-2981 standard function. Read from the reserved metadata keys `royaltyReceiver` and
    /// `royaltyBasisPoints`, resolved item, then collection scope. Answers the zero address and
    /// zero amount whenever those keys do not describe a usable royalty, so a settling marketplace
    /// is never blocked.
    /// @param tokenId The instance being sold.
    /// @param salePrice The sale price to compute the royalty from.
    /// @return receiver The royalty recipient, or the zero address when none applies.
    /// @return royaltyAmount The amount owed, or zero when none applies.
    /// @custom:reverts "unknown token"
    /// @custom:reverts "royalty exceeds the representable range"
    function royaltyInfo(uint256 tokenId, uint256 salePrice)
        external
        view
        returns (address receiver, uint256 royaltyAmount);

    /// @notice Collection-level metadata URI, from the reserved key `contractURI`, or the empty
    /// string.
    /// @dev ERC-7572 standard function. @custom:function setCollectionMetadata and
    /// @custom:function removeCollectionMetadata emit @custom:emits ContractURIUpdated when they
    /// touch that key.
    /// @return uri The decoded contract URI, or the empty string when unset.
    function contractURI() external view returns (string memory uri);

    /// @notice The collection owner, identical to @custom:function collectionOwner.
    /// @dev ERC-173 standard function, served because tooling calls it. ERC-173 is not claimed:
    /// its id also covers `transferOwnership`, which cannot exist here, because a handover carries
    /// the collection's storage deposit and the successor must accept and fund it. That is what
    /// @custom:function nominateCollectionOwner and @custom:function claimCollectionOwnership are
    /// for.
    /// @return owner The account that owns this collection.
    function owner() external view returns (address owner);

    /// @notice Define a new immutable item in this collection with shared metadata defaults.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. `keys` and `values` are parallel arrays. `soulbound` binds every instance
    /// minted from this item to its purse key, and is fixed here.
    /// @param soulbound Whether instances minted from this item are bound to their purse key.
    /// @param keys Metadata keys shared by every instance minted from the item.
    /// @param values Metadata values positionally paired with `keys`.
    /// @return item The index assigned to the new item definition.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "metadata keys and values differ in length"
    /// @custom:reverts "metadata key too long"
    /// @custom:reverts "metadata value too long"
    /// @custom:reverts "reserved metadata value is not valid UTF-8"
    /// @custom:reverts "item index space exhausted"
    /// @custom:reverts "collection owner cannot pay the storage deposit"
    function defineItem(bool soulbound, bytes[] calldata keys, bytes[] calldata values) external returns (uint32 item);

    /// @notice Mint one instance of `item` into the empty purse key `to` with instance-level
    /// metadata overrides.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. `keys` and `values` are parallel overrides validated as in
    /// @custom:function defineItem.
    /// @param item The item definition to mint an instance of.
    /// @param to The empty purse key to mint into.
    /// @param keys Instance-level metadata override keys.
    /// @param values Metadata values positionally paired with `keys`.
    /// @return tokenId The permanent instance identifier, matching the emitted @custom:emits Transfer.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "destination is the zero address"
    /// @custom:reverts "unknown item"
    /// @custom:reverts "destination purse already holds an instance"
    /// @custom:reverts "too many instance metadata entries"
    /// @custom:reverts "item supply exhausted"
    /// @custom:reverts "instance id space exhausted"
    /// @custom:reverts "metadata keys and values differ in length"
    /// @custom:reverts "metadata key too long"
    /// @custom:reverts "metadata value too long"
    /// @custom:reverts "reserved metadata value is not valid UTF-8"
    /// @custom:reverts "collection owner cannot pay the storage deposit"
    /// @custom:emits Transfer
    /// @custom:emits Locked
    /// @custom:emits Unlocked
    function mint(uint32 item, address to, bytes[] calldata keys, bytes[] calldata values)
        external
        returns (uint256 tokenId);

    /// @notice Move a live instance of this collection to the empty purse key `to`.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. It ignores whether the instance is soulbound.
    /// @param tokenId The instance to move.
    /// @param to The empty purse key to move the instance to.
    /// @custom:reverts "destination is the zero address"
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown token"
    /// @custom:reverts "destination already holds this instance"
    /// @custom:reverts "destination purse already holds an instance"
    /// @custom:reverts "instance state nonce exhausted"
    /// @custom:emits Transfer
    function forceTransfer(uint256 tokenId, address to) external;

    /// @notice Permanently remove a live instance of this collection.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this.
    /// @param tokenId The instance to burn.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown token"
    /// @custom:emits Transfer
    function forceBurn(uint256 tokenId) external;

    /// @notice Set or overwrite the collection-scope metadata default under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. On runtimes wiring the ERC-721 metadata policy, when `key` is `name`,
    /// `symbol` or `tokenURI`, `value` must be valid UTF-8, so a mistake fails here rather than
    /// reading back as replacement characters.
    /// @param key The metadata key to set.
    /// @param value The metadata value to store under `key`.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "metadata key too long"
    /// @custom:reverts "metadata value too long"
    /// @custom:reverts "reserved metadata value is not valid UTF-8"
    /// @custom:reverts "collection owner cannot pay the storage deposit"
    /// @custom:emits BatchMetadataUpdate
    /// @custom:emits ContractURIUpdated
    function setCollectionMetadata(bytes calldata key, bytes calldata value) external;

    /// @notice Remove the collection-scope metadata default under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. Removing an absent key is a successful no-op that still emits.
    /// @param key The metadata key to remove.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "metadata key too long"
    /// @custom:emits BatchMetadataUpdate
    /// @custom:emits ContractURIUpdated
    function removeCollectionMetadata(bytes calldata key) external;

    /// @notice Set or overwrite the item-scope metadata default of `item` under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. The value is shared by every instance minted from `item`; reserved keys are
    /// validated as in @custom:function setCollectionMetadata.
    /// @param item The item definition to set the default on.
    /// @param key The metadata key to set.
    /// @param value The metadata value to store under `key`.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown item"
    /// @custom:reverts "metadata key too long"
    /// @custom:reverts "metadata value too long"
    /// @custom:reverts "reserved metadata value is not valid UTF-8"
    /// @custom:reverts "collection owner cannot pay the storage deposit"
    /// @custom:emits BatchMetadataUpdate
    function setItemMetadata(uint32 item, bytes calldata key, bytes calldata value) external;

    /// @notice Remove the item-scope metadata default of `item` under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. Removing an absent key is a successful no-op that still emits.
    /// @param item The item definition to remove the default from.
    /// @param key The metadata key to remove.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown item"
    /// @custom:reverts "metadata key too long"
    /// @custom:emits BatchMetadataUpdate
    function removeItemMetadata(uint32 item, bytes calldata key) external;

    /// @notice Set or overwrite the instance-scope metadata override of `tokenId` under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this; reserved keys are validated as in @custom:function setCollectionMetadata.
    /// @param tokenId The instance to override metadata on.
    /// @param key The metadata key to set.
    /// @param value The metadata value to store under `key`.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown token"
    /// @custom:reverts "too many instance metadata entries"
    /// @custom:reverts "metadata key too long"
    /// @custom:reverts "metadata value too long"
    /// @custom:reverts "reserved metadata value is not valid UTF-8"
    /// @custom:reverts "collection owner cannot pay the storage deposit"
    /// @custom:emits MetadataUpdate
    function setInstanceMetadata(uint256 tokenId, bytes calldata key, bytes calldata value) external;

    /// @notice Remove the instance-scope metadata override of `tokenId` under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. Removing an absent key is a successful no-op that still emits.
    /// @param tokenId The instance to remove the override from.
    /// @param key The metadata key to remove.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown token"
    /// @custom:reverts "metadata key too long"
    /// @custom:emits MetadataUpdate
    function removeInstanceMetadata(uint256 tokenId, bytes calldata key) external;

    /// @notice Nominate `successor` to claim ownership of this collection.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. Nomination alone changes no authority; the successor must call
    /// @custom:function claimCollectionOwnership. Use @custom:function clearCollectionOwnerNomination to withdraw.
    /// @param successor The account nominated to claim the collection.
    /// @custom:reverts "successor is the zero address"
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "successor is already the collection owner"
    function nominateCollectionOwner(address successor) external;

    /// @notice Clear the pending ownership nomination.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this.
    /// @custom:reverts "caller is not the collection owner"
    function clearCollectionOwnerNomination() external;

    /// @notice Claim ownership of this collection after being nominated.
    /// @dev This precompile's own function, not part of any ERC standard. Only the nominated
    /// successor may call this, so it is the one admin function not gated on the current owner. The
    /// caller assumes the collection's aggregate storage deposit and the former owner's deposit is
    /// released; the whole operation is atomic. The "collection owner cannot pay the storage
    /// deposit" revert here refers to the claimant, who must be able to fund the equivalent deposit
    /// before the former owner's ticket is dropped.
    /// @custom:reverts "caller is not the nominated successor"
    /// @custom:reverts "collection owner cannot pay the storage deposit"
    /// @custom:emits OwnershipTransferred
    function claimCollectionOwnership() external;

    /// @notice Delete an item definition.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. Every live instance of `item` must be burnt and every item-scope metadata
    /// entry removed first; deleted item indices are never reused.
    /// @param item The item definition to delete.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "unknown item"
    /// @custom:reverts "item still has live instances"
    /// @custom:reverts "item metadata must be removed first"
    /// @custom:emits ItemDeleted
    function deleteItem(uint32 item) external;

    /// @notice Delete this collection and release its remaining deposit.
    /// @dev This precompile's own function, not part of any ERC standard. Only the collection owner
    /// may call this. Every item definition must be deleted and every collection-scope metadata
    /// entry removed first; the collection id is never reused.
    /// @custom:reverts "caller is not the collection owner"
    /// @custom:reverts "item definitions must be deleted first"
    /// @custom:reverts "collection metadata must be removed first"
    /// @custom:emits CollectionDeleted
    function deleteCollection() external;

    /// @notice The collection owner's address.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @return owner The account that owns this collection.
    function collectionOwner() external view returns (address owner);

    /// @notice The account nominated to claim this collection, or the zero address when none.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @return pendingOwner The nominated successor, or the zero address.
    function pendingCollectionOwner() external view returns (address pendingOwner);

    /// @notice The collection's aggregate storage deposit.
    /// @dev This precompile's own function, not part of any ERC standard. Charged to the current
    /// owner and assumed by a successor on @custom:function claimCollectionOwnership.
    /// @return deposit The aggregate storage deposit backing this collection.
    function collectionOwnerDeposit() external view returns (uint256 deposit);

    /// @notice Whether a collection-scope entry exists under `key`, even with an empty value.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @param key The metadata key to probe.
    /// @return exists True when a collection-scope entry is stored under `key`.
    /// @custom:reverts "metadata key too long"
    function hasCollectionMetadata(bytes calldata key) external view returns (bool exists);

    /// @notice Whether an item-scope entry of `item` exists under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Unlike
    /// @custom:function itemMetadata, this does not fall back to the collection scope.
    /// @param item The item definition to probe.
    /// @param key The metadata key to probe.
    /// @return exists True when an item-scope entry is stored under `key`.
    /// @custom:reverts "metadata key too long"
    function hasItemMetadata(uint32 item, bytes calldata key) external view returns (bool exists);

    /// @notice Whether an instance-scope entry of `tokenId` exists under `key`.
    /// @dev This precompile's own function, not part of any ERC standard. Unlike
    /// @custom:function instanceMetadata, this does not fall back to the item or collection scope.
    /// @param tokenId The instance to probe.
    /// @param key The metadata key to probe.
    /// @return exists True when an instance-scope entry is stored under `key`.
    /// @custom:reverts "unknown token"
    /// @custom:reverts "metadata key too long"
    function hasInstanceMetadata(uint256 tokenId, bytes calldata key) external view returns (bool exists);

    /// @notice Minted-ever and currently-live instance counts of `item`.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @param item The item definition to query.
    /// @return supply The number of instances ever minted from `item`.
    /// @return liveSupply The number of currently live instances of `item`.
    /// @custom:reverts "unknown item"
    function itemSupply(uint32 item) external view returns (uint32 supply, uint32 liveSupply);

    /// @notice Item index, mint time, last move time and state nonce of a live instance.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @param tokenId The instance to query.
    /// @return item The item definition the instance was minted from.
    /// @return mintedAt Unix seconds at mint.
    /// @return lastMoved Unix seconds of the last move, equal to `mintedAt` until the first move.
    /// @return stateNonce The monotonic ownership-state revision.
    /// @custom:reverts "unknown token"
    function instanceInfo(uint256 tokenId)
        external
        view
        returns (uint32 item, uint64 mintedAt, uint64 lastMoved, uint64 stateNonce);

    /// @notice Collection-scope metadata value for `key`, or empty bytes when unset.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @param key The metadata key to read.
    /// @return value The raw stored bytes, or empty bytes when unset.
    /// @custom:reverts "metadata key too long"
    function collectionMetadata(bytes calldata key) external view returns (bytes memory value);

    /// @notice Metadata value for `key` resolved item, then collection scope, or empty bytes.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @param item The item definition to resolve from.
    /// @param key The metadata key to read.
    /// @return value The raw stored bytes, or empty bytes when unset.
    /// @custom:reverts "metadata key too long"
    function itemMetadata(uint32 item, bytes calldata key) external view returns (bytes memory value);

    /// @notice Metadata value for `key` resolved instance, then item, then collection scope, or
    /// empty bytes.
    /// @dev This precompile's own function, not part of any ERC standard.
    /// @param tokenId The instance to resolve from.
    /// @param key The metadata key to read.
    /// @return value The raw stored bytes, or empty bytes when unset.
    /// @custom:reverts "unknown token"
    /// @custom:reverts "metadata key too long"
    function instanceMetadata(uint256 tokenId, bytes calldata key) external view returns (bytes memory value);
}
