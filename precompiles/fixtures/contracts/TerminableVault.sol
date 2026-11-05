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

import "./TokenVault.sol";

/// @notice The `pallet-revive` system precompile, reduced to the call this fixture makes.
interface ISystem {
    /// @notice Remove the calling contract's code at the end of the transaction and pay its
    /// balance to `beneficiary`.
    function terminate(address beneficiary) external;
}

/// @title TerminableVault
/// @notice A `TokenVault` whose owner can remove its code.
/// @dev A test fixture for the contract lifecycle. A custody contract must not expose this path,
/// because a redeploy at the address takes over every token it holds.
/// @custom:security-contact admin@parity.io
contract TerminableVault is TokenVault {
    /// @notice The `pallet-revive` system precompile.
    ISystem private constant SYSTEM = ISystem(address(0x900));

    /// @notice Remove this contract's code and pay its balance to `beneficiary`.
    /// @param beneficiary The account that receives the balance.
    function terminate(address beneficiary) external {
        require(msg.sender == owner, NotOwner());
        SYSTEM.terminate(beneficiary);
    }
}
