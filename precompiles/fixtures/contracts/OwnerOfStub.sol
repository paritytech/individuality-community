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

/// @title OwnerOfStub
/// @notice A test-only stand-in for a collection whose `ownerOf` answers, or fails, as a test says.
/// @dev `SlotVault` accepts any address as a collection, and `void` decides whether a stake is gone
/// from how `ownerOf` fails. This stub produces each kind of failure on demand, so a test can show
/// which ones `void` accepts without driving the real precompile into them.
/// @custom:security-contact admin@parity.io
contract OwnerOfStub {
    /// @notice How `ownerOf` answers.
    enum Mode {
        /// @dev Return `holder`.
        Answer,
        /// @dev Revert with no return data, as a call that could not run looks to its caller.
        RevertEmpty,
        /// @dev Revert with `Error("unknown token")`, as the precompile does for a burned token.
        RevertUnknownToken,
        /// @dev Revert with another `Error(string)`.
        RevertOther
    }

    /// @notice The address `ownerOf` returns while the mode is `Answer`.
    address public holder;
    /// @notice The current mode.
    Mode public mode;

    /// @notice Set what `ownerOf` returns.
    /// @param newHolder The address to answer with.
    function setHolder(address newHolder) external {
        holder = newHolder;
    }

    /// @notice Set how `ownerOf` answers.
    /// @param newMode The mode.
    function setMode(Mode newMode) external {
        mode = newMode;
    }

    /// @notice Answer or fail according to the mode, for any token.
    function ownerOf(uint256) external view returns (address) {
        if (mode == Mode.RevertEmpty) {
            revert();
        }
        if (mode == Mode.RevertUnknownToken) {
            revert("unknown token");
        }
        if (mode == Mode.RevertOther) {
            revert("token is not in this slot");
        }
        return holder;
    }
}
