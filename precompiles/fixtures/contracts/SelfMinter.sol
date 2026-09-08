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
import "../../nft-claims/sol/INftClaimsMinter.sol";

/// @title SelfMinter
/// @notice Creates a collection through the factory, defines its items, and registers itself as the
/// collection's claims minter. It also answers the callback a claim makes to choose an item.
/// @dev The factory, the collection, and the minter are used in one transaction, with this contract
/// standing as both the collection owner and the registered minter.
/// @custom:security-contact admin@parity.io
contract SelfMinter {
    /// @notice The number of items this contract defines on its collection.
    uint32 private constant ITEM_COUNT = 2;

    /// @notice The account that deployed and controls this contract.
    address public owner;
    /// @notice The id of the managed collection, set once bootstrap runs.
    uint32 public collectionId;

    /// @notice The addresses bootstrap works with.
    /// @param factory The address of the factory.
    /// @param prefix The prefix built into the collection's address.
    /// @param claims The address of the claims minter.
    struct BootstrapConfig {
        address factory;
        uint16 prefix;
        address claims;
    }

    /// @notice Thrown when a caller other than the owner invokes an owner-only function.
    error NotOwner();

    constructor() {
        owner = msg.sender;
    }

    modifier onlyOwner() {
        require(msg.sender == owner, NotOwner());
        _;
    }

    /// @notice Create a collection, define its items, and register this contract as its claims
    /// minter.
    /// @param config The factory, prefix, and claims-minter addresses to work with.
    /// @return collection The id of the created collection.
    function bootstrap(BootstrapConfig calldata config) external onlyOwner returns (uint32 collection) {
        collectionId = IScarcityFactory(config.factory).createCollection();
        address collectionAddress = address((uint160(collectionId) << 128) | (uint160(config.prefix) << 16));
        bytes[] memory none = new bytes[](0);
        for (uint32 index = 0; index < ITEM_COUNT; index++) {
            IScarcityCollection(collectionAddress).defineItem(false, none, none);
        }
        INftClaimsMinter(config.claims).setContractMinter(collectionId, address(this));
        return collectionId;
    }

    /// @notice Pick the item to mint for a claim, ignoring which collection it targets.
    /// @param entropy The randomness the claim carries.
    /// @return item The chosen item, one of those this contract defined.
    function mint(uint32, bytes32 entropy) external pure returns (uint32 item) {
        return uint32(uint256(entropy) % ITEM_COUNT);
    }
}
