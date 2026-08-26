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

/// @title ClaimsMinter
/// @notice Answers the callback a claim makes to choose which item to mint. A registration points at
/// this contract, and each claim calls it to pick an item.
/// @dev Registration needs an address that carries deployed code, and each claim calls back for a
/// single item choice.
/// @custom:security-contact admin@parity.io
contract ClaimsMinter {
    /// @notice The number of items the managed collection defines.
    uint32 private constant ITEM_COUNT = 4;

    /// @notice Pick the item to mint for a claim, ignoring which collection it targets.
    /// @param entropy The randomness the claim carries.
    /// @return item The chosen item, one of those the collection defines.
    function mint(uint32, bytes32 entropy) external pure returns (uint32 item) {
        return uint32(uint256(entropy) % ITEM_COUNT);
    }
}
