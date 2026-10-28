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

import "../../scarcity/sol/IScarcityCollection.sol";

/// @title StakeVault
/// @notice Holds staked Scarcity tokens and credits each one to the player who staked it.
/// @dev A push into a contract does not specify its sender, so a stake takes three steps:
///
/// 1. The player calls `reserve` while it holds the token.
/// 2. The player pushes the token into the vault, with `transferFrom` or the native transfer.
/// 3. Anyone calls `confirm`, which credits each token that arrived and has a reservation.
///
/// Each check costs the same whatever the vault holds, so the vault never reads `balanceOf` or
/// `tokenOfOwnerByIndex`. A copy of this contract adds its own game rules, for example when a
/// player may withdraw.
/// @custom:security-contact admin@parity.io
contract StakeVault {
    /// @notice The most tokens one `confirm` call takes, which bounds its loop.
    uint256 public constant MAX_BATCH = 16;

    /// @notice Where a token is in the stake flow.
    enum Status {
        None,
        Reserved,
        Staked
    }

    /// @notice The player a token is credited to and the token's status.
    struct Entry {
        address player;
        Status status;
    }

    /// @notice The collection whose tokens this vault takes.
    IScarcityCollection public immutable collection;

    /// @notice The entry of each token.
    mapping(uint256 => Entry) public entries;

    /// @notice The caller does not hold the token.
    error NotHolder();
    /// @notice The token has no reservation.
    error NotReserved();
    /// @notice The vault does not hold the token.
    error NotReceived();
    /// @notice The caller is not the player of the stake.
    error NotStaker();
    /// @notice The batch holds more than `MAX_BATCH` tokens.
    error BatchTooLarge();

    /// @param collection_ The collection whose tokens this vault takes.
    constructor(address collection_) {
        collection = IScarcityCollection(collection_);
    }

    /// @notice Record the caller as the player of `tokenId`, before it pushes the token.
    /// @dev Only the holder passes the check, so a front-run gains nothing. The vault does not hold
    /// the token, so any earlier entry is stale and the new record replaces it.
    /// @param tokenId The token the caller holds and stakes next.
    function reserve(uint256 tokenId) external {
        require(collection.ownerOf(tokenId) == msg.sender, NotHolder());
        entries[tokenId] = Entry(msg.sender, Status.Reserved);
    }

    /// @notice Mark each token in `tokenIds` as staked by the player that reserved it.
    /// @dev Anyone may call, because the reservation names the player. One failing token reverts
    /// the batch.
    /// @param tokenIds The tokens to confirm, at most `MAX_BATCH`.
    function confirm(uint256[] calldata tokenIds) external {
        require(tokenIds.length <= MAX_BATCH, BatchTooLarge());
        for (uint256 i = 0; i < tokenIds.length; i++) {
            uint256 tokenId = tokenIds[i];
            Entry storage entry = entries[tokenId];
            require(entry.status == Status.Reserved, NotReserved());
            require(collection.ownerOf(tokenId) == address(this), NotReceived());
            entry.status = Status.Staked;
        }
    }

    /// @notice Move a staked token back out to `to`, as the player that staked it.
    /// @param tokenId The staked token.
    /// @param to The empty purse key or the contract to move the token to.
    function withdraw(uint256 tokenId, address to) external {
        Entry memory entry = entries[tokenId];
        require(entry.status == Status.Staked && entry.player == msg.sender, NotStaker());
        delete entries[tokenId];
        collection.transferFrom(address(this), to, tokenId);
    }
}
