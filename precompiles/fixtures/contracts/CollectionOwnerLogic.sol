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

/// @title CollectionOwnerLogic
/// @notice Implementation code meant to run behind a proxy. When a proxy delegate-calls it, this
/// code runs under the proxy's address and in the proxy's storage, so the collection it creates is
/// owned by the proxy, and the owner-only call it makes to define an item succeeds.
/// @dev The collection id sits in the first storage slot, which is the proxy's slot when this runs
/// through a delegate call.
/// @custom:security-contact admin@parity.io
contract CollectionOwnerLogic {
    /// @notice The id of the collection this owner created.
    uint32 public collectionId;

    /// @notice Create a collection through the factory and define one item on it.
    /// @dev Run through a proxy, the caller the factory sees is the proxy, so the proxy owns the
    /// collection and the owner-only call to define an item is allowed.
    /// @param factory The address of the factory.
    /// @param prefix The prefix built into the collection's address.
    /// @return collection The id of the created collection.
    function bootstrap(address factory, uint16 prefix) external returns (uint32 collection) {
        collectionId = IScarcityFactory(factory).createCollection();
        address collectionAddr = address((uint160(collectionId) << 128) | (uint160(prefix) << 16));
        bytes[] memory none = new bytes[](0);
        IScarcityCollection(collectionAddr).defineItem(false, none, none);
        return collectionId;
    }
}
