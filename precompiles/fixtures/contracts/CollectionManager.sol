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
import "../../scarcity/sol/IScarcityFactory.sol";

/// @title CollectionManager
/// @notice Creates a collection through the factory, takes ownership of it, and mints only behind
/// its own access control.
/// @dev This contract is the collection owner, so later owner-only calls answer to its access
/// control. It works out the collection's address from the id the runtime assigns, and uses the
/// factory and the collection in the same transaction.
/// @custom:security-contact admin@parity.io
contract CollectionManager {
    /// @notice The account that deployed and controls this manager.
    address public owner;
    /// @notice The factory used to create the managed collection.
    address public factory;
    /// @notice The prefix built into the managed collection's address.
    uint16 public collectionPrefix;
    /// @notice The id of the managed collection, set once bootstrap runs.
    uint32 public collectionId;

    /// @notice Thrown when a caller other than the owner invokes an owner-only function.
    error NotOwner();

    constructor() {
        owner = msg.sender;
    }

    modifier onlyOwner() {
        require(msg.sender == owner, NotOwner());
        _;
    }

    /// @notice Create a collection owned by this contract and define one item on it.
    /// @dev Ownership passes to this contract, so later owner-only calls answer to its access
    /// control.
    /// @param factory_ The address of the factory.
    /// @param prefix_ The prefix built into the collection's address.
    /// @return collection The id of the created collection.
    function bootstrap(address factory_, uint16 prefix_) external onlyOwner returns (uint32 collection) {
        factory = factory_;
        collectionPrefix = prefix_;
        collectionId = IScarcityFactory(factory_).createCollection();
        bytes[] memory none = new bytes[](0);
        IScarcityCollection(collectionAddress()).defineItem(false, none, none);
        return collectionId;
    }

    /// @notice Mint the first item of the managed collection to a key, behind this contract's access
    /// control.
    /// @param to The empty key to mint into.
    /// @return tokenId The identifier of the minted token.
    function mintTo(address to) external onlyOwner returns (uint256 tokenId) {
        bytes[] memory none = new bytes[](0);
        return IScarcityCollection(collectionAddress()).mint(0, to, none, none);
    }

    /// @notice The address of the managed collection.
    /// @dev The id sits in the top four bytes and the prefix in the next-to-last two bytes, both in
    /// big-endian order.
    /// @return collection The address of the managed collection.
    function collectionAddress() public view returns (address collection) {
        return address((uint160(collectionId) << 128) | (uint160(collectionPrefix) << 16));
    }
}
