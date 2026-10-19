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

/// @title SlotReleaser
/// @notice A test-only contract that moves any token out of any of its slots for its owner.
/// @dev This exists so tests get calls to `transferFromSlot` from a contract without a staking
/// round's rules in the way. It keeps no custody record and lets the owner take any token whatever
/// anyone else was promised, so it is not a model for a contract that holds other people's tokens:
/// for that, copy `SlotVault`.
/// @custom:security-contact admin@parity.io
contract SlotReleaser {
    /// @notice The account that deployed and controls this contract.
    address public owner;

    /// @notice Thrown when a caller other than the owner invokes an owner-only function.
    error NotOwner();

    constructor() {
        owner = msg.sender;
    }

    modifier onlyOwner() {
        require(msg.sender == owner, NotOwner());
        _;
    }

    /// @notice The address of slot `index` of this contract, derived without the precompile.
    /// @dev The low 20 bytes of `keccak256(abi.encodePacked("scarcity/slot", vault, index))`.
    /// @param index The slot number.
    /// @return slot The address of the slot.
    function slotOf(uint256 index) public view returns (address slot) {
        return address(uint160(uint256(keccak256(abi.encodePacked("scarcity/slot", address(this), index)))));
    }

    /// @notice Move a token out of one of this contract's slots to an empty key.
    /// @param collection The address of the token's collection.
    /// @param index The slot number the token sits in.
    /// @param tokenId The token to move.
    /// @param to The empty key to move it to.
    function release(address collection, uint256 index, uint256 tokenId, address to) external onlyOwner {
        IScarcityCollection(collection).transferFromSlot(index, tokenId, to);
    }
}
