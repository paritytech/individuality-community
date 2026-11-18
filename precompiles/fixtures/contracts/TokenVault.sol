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

/// @title TokenVault
/// @notice Holds any number of Scarcity tokens and lets its deployer move them out.
/// @dev A test fixture, not a model for custody: it keeps no record of who sent which token.
/// @custom:security-contact admin@parity.io
contract TokenVault {
    /// @notice The account that deployed the vault and may release its tokens.
    address public owner;

    /// @notice Thrown when a caller other than the owner releases a token.
    error NotOwner();

    constructor() {
        owner = msg.sender;
    }

    /// @notice Move a token this vault holds to `to`.
    /// @param collection The address of the token's collection.
    /// @param tokenId The token to move.
    /// @param to The empty purse key or the contract to move the token to.
    function release(address collection, uint256 tokenId, address to) external {
        require(msg.sender == owner, NotOwner());
        IScarcityCollection(collection).transferFrom(address(this), to, tokenId);
    }

    /// @notice The number of tokens of `collection` this vault holds.
    /// @param collection The address of the collection.
    /// @return count The balance the collection reports for this vault.
    function held(address collection) external view returns (uint256 count) {
        return IScarcityCollection(collection).balanceOf(address(this));
    }
}
