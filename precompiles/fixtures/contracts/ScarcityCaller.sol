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

/// @title ScarcityCaller
/// @notice Reads a collection's owner three ways: an ordinary read that succeeds, a delegate call
/// the collection refuses, and a call that carries value and is also refused. The two refused calls
/// capture the failure and hand it back to their caller, so a test can inspect what came back.
/// @dev A refusal comes back with a readable reason, while a hard failure comes back empty, and a
/// test tells the two apart by that difference.
/// @custom:security-contact admin@parity.io
contract ScarcityCaller {
    /// @notice Read the collection owner through an ordinary read-only call, which succeeds.
    /// @param collection The address of the collection to read.
    /// @return owner The owner the collection reports.
    function readOwner(address collection) external view returns (address owner) {
        return IScarcityCollection(collection).collectionOwner();
    }

    /// @notice Read the collection owner through a delegate call, which the collection refuses, so
    /// the call comes back as a failure.
    /// @param collection The address of the collection to read.
    /// @return ok False, because the collection refuses a delegate call.
    /// @return returnData The reason the collection gave for refusing.
    function delegateReadOwner(address collection) external returns (bool ok, bytes memory returnData) {
        (ok, returnData) = collection.delegatecall(abi.encodeWithSignature("collectionOwner()"));
    }

    /// @notice Read the collection owner while sending value, which the collection refuses, so the
    /// call comes back as a failure.
    /// @param collection The address of the collection to read.
    /// @return ok False, because the collection refuses a call that carries value.
    /// @return returnData The reason the collection gave for refusing.
    function valueReadOwner(address collection) external payable returns (bool ok, bytes memory returnData) {
        (ok, returnData) = collection.call{value: msg.value}(abi.encodeWithSignature("collectionOwner()"));
    }
}
