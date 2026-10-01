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

import "../../personhood/sol/IPersonhood.sol";

/// @title PersonhoodCaller
/// @notice Queries the personhood precompile four ways: a read and a proof verification inside a
/// read-only frame that both succeed, a delegate call the precompile refuses and a call that
/// carries value and is also refused. Both selectors have a read-only path, because a guard added
/// for one does not cover the other. Each function captures what came back and hands it to its
/// caller, so a test can inspect it.
/// @dev `staticcall` puts the precompile in a read-only frame while the enclosing call stays
/// writable. A refusal that reverts carries a reason, while one that traps comes back empty, and a
/// test tells the two apart by that difference.
/// @custom:security-contact admin@parity.io
contract PersonhoodCaller {
    /// @notice Read `account`'s status from inside a read-only frame.
    /// @param personhood The address of the personhood precompile.
    /// @param account The account to report on.
    /// @param context The context alias to resolve against.
    /// @return ok True, because a read-only frame serves a view.
    /// @return returnData The encoded `PersonhoodInfo` the precompile answered with.
    function readInStaticFrame(address personhood, address account, bytes32 context)
        external
        view
        returns (bool ok, bytes memory returnData)
    {
        (ok, returnData) =
            personhood.staticcall(abi.encodeWithSelector(IPersonhood.personhoodStatus.selector, account, context));
    }

    /// @notice Verify `request` from inside a read-only frame.
    /// @param personhood The address of the personhood precompile.
    /// @param request The bundled verification inputs, forwarded unchanged.
    /// @return ok True, because a read-only frame serves a view.
    /// @return returnData The encoded verification outcome the precompile answered with.
    function verifyProofInStaticFrame(address personhood, IPersonhood.ProofVerificationRequest calldata request)
        external
        view
        returns (bool ok, bytes memory returnData)
    {
        (ok, returnData) =
            personhood.staticcall(abi.encodeWithSelector(IPersonhood.personhoodInfoByProof.selector, request));
    }

    /// @notice Read `account`'s status through a delegate call, which the precompile refuses.
    /// @param personhood The address of the personhood precompile.
    /// @param account The account to report on.
    /// @param context The context alias to resolve against.
    /// @return ok False, because the precompile refuses a delegate call.
    /// @return returnData The reason the precompile gave for refusing.
    function readViaDelegateCall(address personhood, address account, bytes32 context)
        external
        returns (bool ok, bytes memory returnData)
    {
        (ok, returnData) =
            personhood.delegatecall(abi.encodeWithSelector(IPersonhood.personhoodStatus.selector, account, context));
    }

    /// @notice Read `account`'s status while sending value, which the precompile refuses.
    /// @param personhood The address of the personhood precompile.
    /// @param account The account to report on.
    /// @param context The context alias to resolve against.
    /// @return ok False, because the precompile refuses a call that carries value.
    /// @return returnData The reason the precompile gave for refusing.
    function readWithValue(address personhood, address account, bytes32 context)
        external
        payable
        returns (bool ok, bytes memory returnData)
    {
        (ok, returnData) = personhood.call{value: msg.value}(
            abi.encodeWithSelector(IPersonhood.personhoodStatus.selector, account, context)
        );
    }
}
