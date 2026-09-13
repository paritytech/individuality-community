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

import {IERC165} from "./vendor/openzeppelin/IERC165.sol";
import {IERC721} from "./vendor/openzeppelin/IERC721.sol";
import {IERC721Metadata} from "./vendor/openzeppelin/IERC721Metadata.sol";

/// @title InterfaceIds
/// @notice Reports the ERC-165 interface ids of ERC-165, ERC-721 and the ERC-721 metadata extension,
/// each computed by the compiler from the standard interface. A test deploys this and compares the
/// results against the precompile crate's constants, so the constants are checked against the
/// compiler's own computation rather than trusted by hand.
/// @custom:security-contact admin@parity.io
contract InterfaceIds {
    /// @notice The ERC-165 interface id.
    function erc165InterfaceId() external pure returns (bytes4) {
        return type(IERC165).interfaceId;
    }

    /// @notice The ERC-721 core interface id.
    function erc721InterfaceId() external pure returns (bytes4) {
        return type(IERC721).interfaceId;
    }

    /// @notice The ERC-721 metadata extension interface id.
    function erc721MetadataInterfaceId() external pure returns (bytes4) {
        return type(IERC721Metadata).interfaceId;
    }
}
