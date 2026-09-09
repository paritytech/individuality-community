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

import "../../nft-claims/sol/INftClaimsMinter.sol";

/// @title ClaimsMinterCaller
/// @notice Reaches the claims-minter precompile as the owner of a collection: a read and a
/// registration from inside a read-only frame, and the same registration from an ordinary one. Each
/// function hands back what came out, so a test can inspect it.
/// @dev `staticcall` puts the precompile in a read-only frame while the enclosing call stays
/// writable. A refusal that reverts carries a reason and one that traps comes back empty, and a test
/// tells the two apart by that difference.
/// @custom:security-contact admin@parity.io
contract ClaimsMinterCaller {
    /// @notice Read the registration of `collection` from inside a read-only frame.
    /// @param claims The address of the claims-minter precompile.
    /// @param collection The Scarcity collection to read.
    /// @return ok True, because a read is served in a read-only frame.
    /// @return returnData The encoded registration the precompile reported.
    function readMinterInStaticFrame(address claims, uint32 collection)
        external
        view
        returns (bool ok, bytes memory returnData)
    {
        (ok, returnData) =
            claims.staticcall(abi.encodeWithSelector(INftClaimsMinter.collectionMinter.selector, collection));
    }

    /// @notice Register `collection` for random selection from inside a read-only frame, which the
    /// precompile denies.
    /// @param claims The address of the claims-minter precompile.
    /// @param collection The Scarcity collection to register.
    /// @return ok False, because a read-only frame permits no state change.
    /// @return returnData What came back, which is empty when the denial traps the frame.
    function registerInStaticFrame(address claims, uint32 collection)
        external
        view
        returns (bool ok, bytes memory returnData)
    {
        (ok, returnData) =
            claims.staticcall(abi.encodeWithSelector(INftClaimsMinter.setRandomMinter.selector, collection));
    }

    /// @notice Register `collection` for random selection through an ordinary call.
    /// @param claims The address of the claims-minter precompile.
    /// @param collection The Scarcity collection to register.
    /// @return ok True when this contract owns `collection`, which leaves the frame's read-only flag
    /// as the only difference from `registerInStaticFrame`.
    /// @return returnData What came back, which is empty on success.
    function register(address claims, uint32 collection) external returns (bool ok, bytes memory returnData) {
        (ok, returnData) = claims.call(abi.encodeWithSelector(INftClaimsMinter.setRandomMinter.selector, collection));
    }
}
