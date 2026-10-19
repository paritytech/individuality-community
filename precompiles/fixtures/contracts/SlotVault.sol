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
/// - Its only loops, in `settle` and `refund`, are bounded by `MAX_STAKES`.
/// - It counts only tokens of collections the owner accepted, and ignores anything else sitting in
///   a slot.
/// - The vault never ejects junk itself. A player whose push was blocked reserves another slot, and
///   the reserver of a slot may take whatever sits in it while it holds no live stake.
/// - A player never loses custody of a token they pushed into their own reserved slot. If the stake
///   cannot be confirmed, because the round is full or has closed or the collection was not
///   accepted, `withdraw` gives the token back.
///
/// A winner claims one token per call into an empty key, because a purse key holds one token. A
/// failing destination only blocks that one claim, refund or withdrawal. The contract also derives
/// slots on its own, which makes it the independent check on the precompile's derivation.
///
/// A round runs once per deployment: `settled` is never reset.
/// @custom:security-contact admin@parity.io
contract SlotVault {
    /// @notice The most live stakes a round holds. Bounds the loops in `settle` and `refund`.
    uint256 public constant MAX_STAKES = 2;

    /// @notice The account that deployed and controls this vault.
    address public owner;
    /// @notice Whether the owner has called `settle`.
    bool public settled;
    /// @notice The number of slots handed out so far; the next reservation takes this index.
    uint256 public reservations;

    /// @dev Everything this vault knows about one slot. `player` is who may confirm or withdraw
    /// from the slot, and after `settle` who may claim a stake in it.
    struct Stake {
        address player;
        address refundTo;
        address collection;
        uint256 tokenId;
        bool staked;
    }

    mapping(address => bool) private accepted;
    mapping(uint256 => Stake) private stakes;
    /// @dev The slots holding a live stake: confirmed, and not refunded. At most `MAX_STAKES`.
    uint256[] private live;

    /// @notice Thrown when a caller other than the owner invokes an owner-only function.
    error NotOwner();
    /// @notice Thrown when a stake names a collection the owner has not accepted.
    error NotAccepted();
    /// @notice Thrown when the caller did not reserve the slot, or the slot holds a live stake.
    error NotReserved();
    /// @notice Thrown when the round already holds `MAX_STAKES` stakes or has been settled.
    error RoundClosed();
    /// @notice Thrown when the named token is not in the slot.
    error NotInSlot();
    /// @notice Thrown when a slot cannot be claimed or refunded by this caller now.
    error NotReleasable();
    /// @notice Thrown when a reservation names the zero address to send a token back to, which the
    /// precompile refuses as a destination and which would strand the token.
    error NoRefundKey();

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
    /// @dev Accept only collections whose owner this vault trusts. A collection's owner keeps
    /// `forceTransfer` and `forceBurn` over its tokens wherever they sit, slots included, so an
    /// owner who is also a player can pull a losing stake back out after `settle`, and the winner's
    /// `claim` then reverts with "token is not in this slot".
    /// @param collection The address of the collection.
    function accept(address collection) external onlyOwner {
        accepted[collection] = true;
    }

    /// @notice Reserve the next slot for the caller, while the round is open.
    /// @param refundTo The empty key a token goes to if it is refunded or withdrawn.
    /// @return index The slot to push a token into.
    function reserve(address refundTo) external returns (uint256 index) {
        require(!settled, RoundClosed());
        require(refundTo != address(0), NoRefundKey());
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
        require(!settled && live.length < MAX_STAKES, RoundClosed());
        require(IScarcityCollection(collection).ownerOf(tokenId) == slotOf(index), NotInSlot());
        stake.collection = collection;
        stake.tokenId = tokenId;
        stake.staked = true;
        live.push(index);
    }

    /// @notice Take back a token from a reserved slot that holds no live stake.
    /// @dev This is how a player gets a token back that `confirm` turned away, or one pushed into the
    /// slot by someone else. It goes to the key the player named, whoever pushed it. It does not ask
    /// whether the collection is accepted, because custody of what a player pushed must not depend
    /// on the owner's list. That is safe here: only the slot's reserver can call it, it writes no
    /// state, and the only call it makes goes to the address the caller names, asking it to move a
    /// token out of the caller's own slot, which gives that address no authority over the vault.
    /// @param collection The address of the token's collection.
    /// @param index The slot the caller reserved.
    /// @param tokenId The token to take back.
    function withdraw(address collection, uint256 index, uint256 tokenId) external {
        Stake storage stake = stakes[index];
        require(stake.player == msg.sender && !stake.staked, NotReserved());
        IScarcityCollection(collection).transferFromSlot(index, tokenId, stake.refundTo);
    }

    /// @notice Give every live stake of the round to the winner.
    /// @dev A mapping write per stake. No token moves, so no destination key is needed yet.
    /// @param winner The account that may claim the stakes.
    function settle(address winner) external onlyOwner {
        require(!settled, RoundClosed());
        settled = true;
        for (uint256 i = 0; i < live.length; i++) {
            stakes[live[i]].player = winner;
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

    /// @notice Send one live stake back to the key its player named, while the round is unsettled.
    /// @dev Frees the capacity the stake used.
    /// @param index The slot to refund.
    function refund(uint256 index) external onlyOwner {
        Stake storage stake = stakes[index];
        require(!settled && stake.staked, NotReleasable());
        stake.staked = false;
        for (uint256 i = 0; i < live.length; i++) {
            if (live[i] == index) {
                live[i] = live[live.length - 1];
                live.pop();
                break;
            }
        }
        IScarcityCollection(stake.collection).transferFromSlot(index, stake.tokenId, stake.refundTo);
    }

    /// @notice The account that may confirm or withdraw from, and after settlement claim, slot `index`.
    /// @param index The slot number.
    /// @return player The account, or the zero address for a slot never reserved.
    function playerOf(uint256 index) external view returns (address player) {
        return stakes[index].player;
    }
}
