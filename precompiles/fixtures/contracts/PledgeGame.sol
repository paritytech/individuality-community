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

/// @title PledgeGame
/// @notice A winner-takes-all round staked with pledges. Each player pledges a token to this
/// contract, anyone reports the pledge with `join`, and the operator settles: the winner's stake
/// is released and every other stake moves to a key the winner named.
/// @dev The tokens never leave their holders' keys before settlement, so the contract keeps no
/// custody and strands nothing. A round that is never settled ends when the pledges expire, and the
/// players keep their stakes. The contract follows the rules `IScarcityCollection` sets for
/// contracts that take pledges:
/// - `join` counts a token only when the pledge names this contract and outlives `deadline`.
/// - `join` credits `ownerOf`, never `msg.sender`, so a front-run `join` credits the holder.
/// - `settle` collects one empty key per prize, because a key holds one token.
/// - `settle` runs while every pledge still has effect, or it reverts as a whole.
/// A fixture: it accepts one collection, fixed at deployment, and keeps no other record.
/// @custom:security-contact admin@parity.io
contract PledgeGame {
    /// @notice The most stakes one round holds, which bounds `settle`.
    uint256 public constant MAX_STAKES = 8;

    /// @notice The collection whose tokens this round takes.
    IScarcityCollection public immutable collection;
    /// @notice The Unix second by which the round settles. A stake's pledge must outlive it.
    uint64 public immutable deadline;
    /// @notice The account that settles the round.
    address public immutable operator;

    /// @notice The stakes, in the order they joined.
    uint256[] public stakes;
    /// @notice The holder each stake was credited to when it joined.
    mapping(uint256 => address) public stakerOf;
    /// @notice True once the round has settled.
    bool public settled;

    /// @notice The token's pledge does not name this contract.
    error NotPledgedHere();
    /// @notice The token's pledge ends before the round can settle.
    error PledgeTooShort();
    /// @notice The token already counts as a stake.
    error AlreadyJoined();
    /// @notice The round holds `MAX_STAKES` stakes.
    error Full();
    /// @notice The round has settled.
    error Settled();
    /// @notice Only the operator may settle.
    error NotOperator();
    /// @notice The winning token is not a stake of this round.
    error UnknownStake();
    /// @notice `settle` needs one prize key per stake other than the winner's.
    error WrongPrizeKeyCount();

    /// @param collection_ The collection whose tokens this round takes.
    /// @param deadline_ The Unix second by which the round settles.
    constructor(address collection_, uint64 deadline_) {
        collection = IScarcityCollection(collection_);
        deadline = deadline_;
        operator = msg.sender;
    }

    /// @notice Count a token pledged to this contract as a stake of its holder.
    /// @dev Anyone may call, because the pledge is the holder's consent and the credit goes to
    /// `ownerOf`.
    /// @param tokenId The pledged token.
    function join(uint256 tokenId) external {
        if (settled) revert Settled();
        if (stakerOf[tokenId] != address(0)) revert AlreadyJoined();
        if (stakes.length == MAX_STAKES) revert Full();
        (address pledgee, uint64 until) = collection.pledgeOf(tokenId);
        if (pledgee != address(this)) revert NotPledgedHere();
        if (until <= deadline) revert PledgeTooShort();
        stakerOf[tokenId] = collection.ownerOf(tokenId);
        stakes.push(tokenId);
    }

    /// @notice Release the winner's stake and move every other stake to the winner's prize keys.
    /// @dev Each loser's stake moves through `transferFrom`, which a pledgee may call as a seize.
    /// @param winningToken The stake whose holder wins.
    /// @param prizeKeys Empty keys of the winner, one per other stake, in join order.
    function settle(uint256 winningToken, address[] calldata prizeKeys) external {
        if (msg.sender != operator) revert NotOperator();
        if (settled) revert Settled();
        if (stakerOf[winningToken] == address(0)) revert UnknownStake();
        if (prizeKeys.length != stakes.length - 1) revert WrongPrizeKeyCount();
        settled = true;
        uint256 next;
        for (uint256 i; i < stakes.length; ++i) {
            uint256 token = stakes[i];
            if (token == winningToken) {
                collection.release(token);
            } else {
                collection.transferFrom(collection.ownerOf(token), prizeKeys[next], token);
                ++next;
            }
        }
    }

    /// @notice How many stakes the round holds.
    /// @return count The number of stakes.
    function stakeCount() external view returns (uint256 count) {
        return stakes.length;
    }
}
