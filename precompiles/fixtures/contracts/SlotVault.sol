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
/// @notice A staking round that holds the players' Scarcity tokens in slots of its own and pays
/// them to one winner, who claims them one by one. Before settlement the owner can cancel a stake,
/// and a player can take back a token that was never counted.
/// @dev A slot is a purse key derived from this contract, so a contract holds many tokens by holding
/// one in each of several slots. This is the reference for a contract author, and it follows the
/// rules in the `slotAddress` documentation:
///
/// - It keeps its own record of who staked what, written by its own functions, and credits a stake
///   to the account that reserved the slot, never to whoever pushed a token into it.
/// - It checks each token it acts on in constant time with `ownerOf(tokenId) == slotOf(index)`, and
///   never reads `balanceOf` or `tokenOfOwnerByIndex`.
/// - Its only loops, in `settle` and `_removeLive`, are bounded by `MAX_STAKES`.
/// - It counts only tokens of collections the owner accepted, and ignores anything else sitting in
///   a slot.
/// - The vault never ejects junk itself. A player whose push was blocked reserves another slot, and
///   the reserver of a slot may take whatever sits in it while it holds no live stake, except a
///   soulbound token, which only its collection's owner can move.
/// - Every exit names its destination when it is made: `claim` and `withdraw` take the key as an
///   argument. A key named in advance, at reservation, would be public from then on, and anyone
///   holding one token could push it there and block the exit, because a purse key holds one token.
///   Naming it at exit time leaves only a destination the caller can change and retry.
/// - A player never loses custody of a token they pushed into their own reserved slot. If the stake
///   cannot be confirmed, because the round is full or has closed or the collection was not
///   accepted, `withdraw` gives the token back, to a key the player names then.
///
/// What a copy of this contract still has to decide for itself:
///
/// - The owner can hold live stakes for as long as it likes, since only `settle` and `cancel` end
///   them and both are owner-only. A real round should add a deadline after which a player may
///   take a stake back.
/// - An accepted collection's owner can pull a stake out of a slot with `forceTransfer` or
///   `forceBurn`. `void` frees the slot and its capacity afterwards.
/// - Authority over the slots follows this contract's address, and its records live in its storage.
///   A contract redeployed at the same address starts with empty records and the old authority.
///
/// A winner claims one token per call into an empty key. The contract also derives slots on its
/// own, which makes it the independent check on the precompile's derivation.
///
/// A round runs once per deployment: `settled` is never reset.
/// @custom:security-contact admin@parity.io
contract SlotVault {
    /// @notice The most live stakes a round holds. Bounds the loops in `settle` and `_removeLive`.
    /// @dev Small so that the tests reach it; a real round picks its own.
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
        address collection;
        uint256 tokenId;
        bool staked;
    }

    mapping(address => bool) private accepted;
    mapping(uint256 => Stake) private stakes;
    /// @dev The slots holding a live stake: confirmed, and neither cancelled nor voided. At most
    /// `MAX_STAKES`.
    uint256[] private live;

    /// @notice Emitted when the owner accepts a collection.
    event Accepted(address indexed collection);
    /// @notice Emitted when a player reserves a slot. From here the slot's address is public.
    event Reserved(uint256 indexed index, address indexed player);
    /// @notice Emitted when a token in a reserved slot becomes a live stake.
    event Confirmed(uint256 indexed index, address indexed player, address collection, uint256 tokenId);
    /// @notice Emitted when the owner gives every live stake to the winner.
    event Settled(address indexed winner);
    /// @notice Emitted when a winner claims a stake.
    event Claimed(uint256 indexed index, address indexed to);
    /// @notice Emitted when the owner cancels a live stake before settlement.
    event Cancelled(uint256 indexed index);
    /// @notice Emitted when a live stake is found to have left its slot and is dropped.
    event Voided(uint256 indexed index);
    /// @notice Emitted when a player takes a token back from a slot that holds no live stake.
    event Withdrawn(uint256 indexed index, address indexed to);

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
    /// @notice Thrown when a stake cannot be claimed or cancelled by this caller now.
    error NotReleasable();
    /// @notice Thrown when settlement names the zero address, which no claim could ever come from.
    error NoWinner();
    /// @notice Thrown when a stake is still in its slot, or the collection could not say that it
    /// has left it, so there is nothing to void.
    error StillInSlot();

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
    /// `claim` then reverts with "token is not in this slot" until someone calls `void`.
    /// @param collection The address of the collection.
    function accept(address collection) external onlyOwner {
        accepted[collection] = true;
        emit Accepted(collection);
    }

    /// @notice Reserve the next slot for the caller, while the round is open.
    /// @return index The slot to push a token into.
    function reserve() external returns (uint256 index) {
        require(!settled, RoundClosed());
        index = reservations++;
        stakes[index] = Stake({player: msg.sender, collection: address(0), tokenId: 0, staked: false});
        emit Reserved(index, msg.sender);
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
        emit Confirmed(index, msg.sender, collection, tokenId);
    }

    /// @notice Take back a token from a reserved slot that holds no live stake, to a key named now.
    /// @dev This is how a player gets a token back that `confirm` turned away, that the owner
    /// cancelled, or that someone else pushed into the slot. A soulbound token is the exception:
    /// the precompile refuses to move it, so only its collection's owner can clear the slot, and
    /// the player reserves another. The destination is an argument rather than something fixed at
    /// reservation, so an occupied key only fails this call, not every later one. It does not ask
    /// whether the collection is accepted, because custody of what a player pushed must not depend
    /// on the owner's list. That is safe here: only the slot's reserver can call it, it writes no
    /// state before the call, and the only call it makes goes to the address the caller names,
    /// asking it to move a token out of the caller's own slot, which gives that address no
    /// authority over the vault. A contract that holds other roles should not copy this as it
    /// stands: restrict `collection` to the precompile address shape or to a known list.
    /// @param collection The address of the token's collection.
    /// @param index The slot the caller reserved.
    /// @param tokenId The token to take back.
    /// @param to The empty key to send it to.
    function withdraw(address collection, uint256 index, uint256 tokenId, address to) external {
        Stake storage stake = stakes[index];
        require(stake.player == msg.sender && !stake.staked, NotReserved());
        emit Withdrawn(index, to);
        IScarcityCollection(collection).transferFromSlot(index, tokenId, to);
    }

    /// @notice Give every live stake of the round to the winner.
    /// @dev A mapping write per stake. No token moves, so no destination key is needed yet.
    /// @param winner The account that may claim the stakes.
    function settle(address winner) external onlyOwner {
        require(!settled, RoundClosed());
        require(winner != address(0), NoWinner());
        settled = true;
        for (uint256 i = 0; i < live.length; i++) {
            stakes[live[i]].player = winner;
        }
        emit Settled(winner);
    }

    /// @notice Move one won token out of its slot to an empty key.
    /// @param index The slot to claim.
    /// @param to The empty key to move the token to.
    function claim(uint256 index, address to) external {
        Stake storage stake = stakes[index];
        require(settled && stake.staked && stake.player == msg.sender, NotReleasable());
        stake.staked = false;
        emit Claimed(index, to);
        IScarcityCollection(stake.collection).transferFromSlot(index, stake.tokenId, to);
    }

    /// @notice Stop counting one live stake, while the round is unsettled.
    /// @dev Frees the capacity the stake used. No token moves: the player takes it back with
    /// `withdraw`, to a key they name then.
    /// @param index The slot to cancel.
    function cancel(uint256 index) external onlyOwner {
        Stake storage stake = stakes[index];
        require(!settled && stake.staked, NotReleasable());
        stake.staked = false;
        _removeLive(index);
        emit Cancelled(index);
    }

    /// @notice Drop a live stake whose token is no longer in its slot, so the slot and its capacity
    /// are not lost.
    /// @dev A collection's owner can move a token out of a slot with `forceTransfer` or `forceBurn`.
    /// Until this runs, `claim` and `cancel` on that slot fail, `withdraw` refuses a live stake, and
    /// the stake still counts toward `MAX_STAKES`. Anyone can call it, because it only records what
    /// the collection already says: it does nothing while the token is still in the slot. A burned
    /// token makes `ownerOf` revert with "unknown token", which counts as gone. No other failure of
    /// that call does: a call that could not run, for example because the call stack was full,
    /// fails with no return data on the EVM backend, and taking that as gone would let anyone drop
    /// a live stake while its token is still in the slot.
    /// @param index The slot to check.
    function void(uint256 index) external {
        Stake storage stake = stakes[index];
        require(stake.staked, NotReleasable());
        try IScarcityCollection(stake.collection).ownerOf(stake.tokenId) returns (address holder) {
            require(holder != slotOf(index), StillInSlot());
        } catch (bytes memory reason) {
            // The precompile's refusal for a token that no longer exists, as `Error(string)`.
            bytes memory gone = abi.encodeWithSignature("Error(string)", "unknown token");
            require(keccak256(reason) == keccak256(gone), StillInSlot());
        }
        stake.staked = false;
        _removeLive(index);
        emit Voided(index);
    }

    /// @notice The account that may confirm or withdraw from, and after settlement claim, slot `index`.
    /// @param index The slot number.
    /// @return player The account, or the zero address for a slot never reserved.
    function playerOf(uint256 index) external view returns (address player) {
        return stakes[index].player;
    }

    /// @dev Remove `index` from the live list. The loop is bounded by `MAX_STAKES`.
    function _removeLive(uint256 index) private {
        for (uint256 i = 0; i < live.length; i++) {
            if (live[i] == index) {
                live[i] = live[live.length - 1];
                live.pop();
                break;
            }
        }
    }
}
