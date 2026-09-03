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

/// @title IERC721Receiver - ERC-721 safe-transfer acknowledgement
/// @notice The callback a contract destination of {IScarcityCollection-safeTransferFrom} answers.
/// @dev Declared for the call that variant makes once `pallet-revive` exports the reentrancy
/// argument of its message-call primitive. Contract destinations are refused until then, so
/// nothing calls this yet.
/// @custom:security-contact admin@parity.io
interface IERC721Receiver {
    /// @notice Handle receipt of `tokenId`, already held by this contract when called.
    /// @dev Return this function's own selector to keep the token; any other return value or a
    /// revert refuses it and undoes the transfer.
    /// @param operator The address that initiated the transfer.
    /// @param from The previous holder of the token.
    /// @param tokenId The token transferred to this contract.
    /// @param data Additional data passed with the transfer.
    /// @return selector The `onERC721Received` selector to accept the token.
    function onERC721Received(address operator, address from, uint256 tokenId, bytes calldata data)
        external
        returns (bytes4 selector);
}
