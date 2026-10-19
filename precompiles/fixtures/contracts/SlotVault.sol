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
/// @notice A staking round that holds the players' Scarcity tokens in slots of its own, pays them
/// all to one winner, and refunds them if the round is called off.
/// @dev A slot is a purse key derived from this contract, so a contract holds many tokens by holding
/// one in each of several slots. This is the reference for a contract author, and it follows the
/// rules in the `slotAddress` documentation:
///
/// - It keeps its own record of who staked what, written by its own functions, and credits a stake
///   to the account that reserved the slot, never to whoever pushed a token into it.
/// - It checks each token it acts on in constant time with `ownerOf(tokenId) == slotOf(index)`, and
///   never reads `balanceOf` or `tokenOfOwnerByIndex`.
/// - Its only loop is `settle`, bounded by `MAX_STAKES`.
/// - It counts only tokens of collections the owner accepted, and ignores anything else sitting in
///   a slot.
/// - It does not eject what someone else put in a slot it reserved: a player whose push was
///   blocked reserves another slot.
///
/// A winner claims one token per call into an empty key, because a purse key holds one token. A
/// failing destination only blocks that one claim or refund. The contract also derives slots on its
/// own, which makes it the independent check on the precompile's derivation.
/// @custom:security-contact admin@parity.io
contract SlotVault {
    /// @notice The most tokens a round holds. Bounds the loop in `settle`.
    uint256 public constant MAX_STAKES = 2;

    /// @notice The account that deployed and controls this vault.
    address public owner;
    /// @notice Whether the owner has called `settle` for the current round.
    bool public settled;
    /// @notice The number of slots handed out so far; the next reservation takes this index.
    uint256 public reservations;

    /// @dev Everything this vault knows about one slot. `player` is who may confirm the slot, and
    /// after `settle` who may claim it.
    struct Stake {
        address player;
        address refundTo;
        address collection;
        uint256 tokenId;
        bool staked;
    }

    mapping(address => bool) private accepted;
    mapping(uint256 => Stake) private stakes;
    uint256[] private staked;

    /// @notice Thrown when a caller other than the owner invokes an owner-only function.
    error NotOwner();
    /// @notice Thrown when a stake names a collection the owner has not accepted.
    error NotAccepted();
    /// @notice Thrown when the caller did not reserve the slot, or the slot already holds a stake.
    error NotReserved();
    /// @notice Thrown when the round already holds `MAX_STAKES` tokens or has been settled.
    error RoundClosed();
    /// @notice Thrown when the named token is not in the slot.
    error NotInSlot();
    /// @notice Thrown when a slot cannot be claimed or refunded by this caller now.
    error NotReleasable();

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

    /// @notice Accept the tokens of a collection as stakes.
    /// @param collection The address of the collection.
    function accept(address collection) external onlyOwner {
        accepted[collection] = true;
    }

    /// @notice Reserve the next slot for the caller.
    /// @param refundTo The empty key the token goes back to if the round is called off.
    /// @return index The slot to push a token into.
    function reserve(address refundTo) external returns (uint256 index) {
        index = reservations++;
        stakes[index] = Stake({
            player: msg.sender,
            refundTo: refundTo,
            collection: address(0),
            tokenId: 0,
            staked: false
        });
    }

    /// @notice Count the token in a reserved slot as the caller's stake.
    /// @dev Credits the account that reserved the slot. It does not look at who pushed the token.
    /// @param collection The address of the token's collection.
    /// @param index The slot the caller reserved.
    /// @param tokenId The token the caller pushed into it.
    function confirm(address collection, uint256 index, uint256 tokenId) external {
        require(accepted[collection], NotAccepted());
        Stake storage stake = stakes[index];
        require(stake.player == msg.sender && !stake.staked, NotReserved());
        require(!settled && staked.length < MAX_STAKES, RoundClosed());
        require(IScarcityCollection(collection).ownerOf(tokenId) == slotOf(index), NotInSlot());
        stake.collection = collection;
        stake.tokenId = tokenId;
        stake.staked = true;
        staked.push(index);
    }

    /// @notice Give every stake of the round to the winner.
    /// @dev A mapping write per stake. No token moves, so no destination key is needed yet.
    /// @param winner The account that may claim the stakes.
    function settle(address winner) external onlyOwner {
        require(!settled, RoundClosed());
        settled = true;
        for (uint256 i = 0; i < staked.length; i++) {
            stakes[staked[i]].player = winner;
        }
    }

    /// @notice Move one won token out of its slot to an empty key.
    /// @param index The slot to claim.
    /// @param to The empty key to move the token to.
    function claim(uint256 index, address to) external {
        Stake storage stake = stakes[index];
        require(settled && stake.staked && stake.player == msg.sender, NotReleasable());
        stake.staked = false;
        IScarcityCollection(stake.collection).transferFromSlot(index, stake.tokenId, to);
    }

    /// @notice Send one token back to the key its player named, while the round is unsettled.
    /// @param index The slot to refund.
    function refund(uint256 index) external onlyOwner {
        Stake storage stake = stakes[index];
        require(!settled && stake.staked, NotReleasable());
        stake.staked = false;
        IScarcityCollection(stake.collection).transferFromSlot(index, stake.tokenId, stake.refundTo);
    }

    /// @notice The account that may confirm, or after settlement claim, slot `index`.
    /// @param index The slot number.
    /// @return player The account, or the zero address for a slot never reserved.
    function playerOf(uint256 index) external view returns (address player) {
        return stakes[index].player;
    }

    /// @notice Move any token out of one of this vault's slots, whatever the round says.
    /// @dev For tests that need the precompile called from a contract without the round's rules.
    /// @param collection The address of the token's collection.
    /// @param index The slot number the token sits in.
    /// @param tokenId The token to move.
    /// @param to The empty key to move it to.
    function release(address collection, uint256 index, uint256 tokenId, address to) external onlyOwner {
        IScarcityCollection(collection).transferFromSlot(index, tokenId, to);
    }
}
