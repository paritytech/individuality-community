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

import "./vendor/openzeppelin/Proxy.sol";

/// @title OwnerProxy
/// @notice Forwards every call to a fixed implementation with a delegate call, on top of
/// OpenZeppelin's Proxy. The implementation runs under this proxy's address and storage, so a
/// collection the implementation creates is owned by this proxy.
/// @dev The implementation address is immutable, so it lives in code rather than storage and never
/// collides with the implementation's own storage.
/// @custom:security-contact admin@parity.io
contract OwnerProxy is Proxy {
    /// @notice The implementation this proxy forwards to.
    address private immutable _implementationAddress;

    /// @param implementation The implementation to forward calls to.
    constructor(address implementation) {
        _implementationAddress = implementation;
    }

    /// @inheritdoc Proxy
    function _implementation() internal view override returns (address) {
        return _implementationAddress;
    }
}
