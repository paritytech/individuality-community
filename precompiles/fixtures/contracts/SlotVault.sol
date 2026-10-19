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

/// @title SlotVault
/// @notice A contract that holds Scarcity tokens in slots of its own and lets its owner release them.
/// @dev A slot is a purse key derived from this contract, so a contract holds many tokens by holding
/// one in each of several slots. This fixture derives slots on its own, as the interface tells
/// contract authors to, which makes it the independent check on the precompile's derivation. It
/// keeps no custody record, so it is not a model of a game: it only gets calls to the precompile
/// from a contract.
/// @custom:security-contact admin@parity.io
contract SlotVault {
    /// @notice The account that deployed and controls this vault.
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

    /// @notice The address of slot `index` of this vault, derived without the precompile.
    /// @dev The low 20 bytes of `keccak256(abi.encodePacked("scarcity/slot", vault, index))`.
    /// @param index The slot number.
    /// @return slot The address of the slot.
    function slotOf(uint256 index) public view returns (address slot) {
        return address(uint160(uint256(keccak256(abi.encodePacked("scarcity/slot", address(this), index)))));
    }

    /// @notice Move a token out of one of this vault's slots to an empty key.
    /// @param collection The address of the token's collection.
    /// @param index The slot number the token sits in.
    /// @param tokenId The token to move.
    /// @param to The empty key to move it to.
    function release(address collection, uint256 index, uint256 tokenId, address to) external onlyOwner {
        IScarcityCollection(collection).transferFromSlot(index, tokenId, to);
    }
}
