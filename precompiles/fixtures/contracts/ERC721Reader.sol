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

/// @title ERC721Reader
/// @notice Reads token ownership and collection metadata from a collection. One read checks, through
/// interface detection, that the collection is an ERC-721 before it returns an owner; the others
/// read directly. It also reads an instance's owner and that owner's balance together, so a test can
/// see whether the two agree.
/// @dev The collection's reads are reached through a static call from this contract.
/// @custom:security-contact admin@parity.io
contract ERC721Reader {
    /// @notice The interface identifier of the ERC-721 core interface.
    bytes4 private constant ERC721_INTERFACE_ID = 0x80ac58cd;

    /// @notice Thrown when the collection does not report ERC-721 support.
    error NotErc721();

    /// @notice Read the owner of a token, but only once interface detection confirms the collection
    /// is an ERC-721.
    /// @dev Reverts with {NotErc721} when detection fails, so the read runs only for an ERC-721
    /// collection.
    /// @param collection The address of the collection to read.
    /// @param tokenId The token to look up.
    /// @return owner The key holding the token.
    function ownerIfErc721(address collection, uint256 tokenId) external view returns (address owner) {
        require(IScarcityCollection(collection).supportsInterface(ERC721_INTERFACE_ID), NotErc721());
        return IScarcityCollection(collection).ownerOf(tokenId);
    }

    /// @notice Read a token's owner and that owner's balance in a single call.
    /// @dev Lets a test see whether a live token's owner holds exactly one, which is what ERC-721
    /// promises. It reports the two figures and leaves the judgement to the caller.
    /// @param collection The address of the collection to read.
    /// @param tokenId The token to inspect.
    /// @return owner The owner the collection reports for the token.
    /// @return ownerBalance The balance the collection reports for that owner.
    function ownerAndBalance(address collection, uint256 tokenId)
        external
        view
        returns (address owner, uint256 ownerBalance)
    {
        IScarcityCollection scarcity = IScarcityCollection(collection);
        owner = scarcity.ownerOf(tokenId);
        ownerBalance = scarcity.balanceOf(owner);
    }

    /// @notice Read the collection's name.
    /// @param collection The address of the collection to read.
    /// @return name The collection name.
    function collectionName(address collection) external view returns (string memory name) {
        return IScarcityCollection(collection).name();
    }
}
