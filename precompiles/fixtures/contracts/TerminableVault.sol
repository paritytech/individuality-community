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
    /// @notice Pay the calling contract's balance to `beneficiary`. Remove its code at the end of
    /// the transaction only when that transaction created it.
    function terminate(address beneficiary) external;
}

/// @title TerminableVault
/// @notice A `TokenVault` whose owner can call `ISystem.terminate`.
/// @dev A test fixture for the contract lifecycle. A call after the creating transaction leaves
/// the code and the tokens in place.
/// @custom:security-contact admin@parity.io
contract TerminableVault is TokenVault {
    /// @notice The `pallet-revive` system precompile.
    ISystem private constant SYSTEM = ISystem(address(0x900));

    /// @notice Pay this contract's balance to `beneficiary` through `ISystem.terminate`.
    /// @param beneficiary The account that receives the balance.
    function terminate(address beneficiary) external {
        require(msg.sender == owner, NotOwner());
        SYSTEM.terminate(beneficiary);
    }
}
