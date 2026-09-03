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

/// @title IScarcityFactory - Scarcity collection factory precompile
/// @notice Creates Scarcity collections, at its own fixed precompile address.
/// @dev Collection creation cannot live on the per-collection addresses because the collection id
/// does not exist until creation allocates it. Native value attached to the call reverts, and a
/// delegate call reverts.
/// @custom:reverts "this precompile does not accept value"
/// @custom:reverts "illegal to call this pre-compile via delegate call"
/// @custom:security-contact admin@parity.io
interface IScarcityFactory {
    /// @notice Emitted when a new collection is allocated.
    /// @dev This precompile's own event, not part of any ERC standard. Emitted by
    /// @custom:function createCollection.
    /// @param collection The id encoded into the collection's own precompile address.
    /// @param owner The account that created and now owns the collection.
    event CollectionCreated(uint32 indexed collection, address indexed owner);

    /// @notice Create a new collection owned by the caller.
    /// @return collection The id of the new collection, naming its per-collection precompile
    /// address.
    /// @custom:reverts "collection id space exhausted"
    /// @custom:reverts "collection owner cannot pay the storage deposit"
    /// @custom:emits CollectionCreated
    function createCollection() external returns (uint32 collection);
}
