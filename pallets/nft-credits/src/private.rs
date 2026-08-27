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

//! The private claim path: the ring one game's claimants register into, and its delivery.
//!
//! A game that opts in mints privately only. Its credits still form the per-block Merkle trees,
//! which record who earned what, but the claims chain refuses a public claim against such a tree
//! and takes the ring instead.
//!
//! A game builds one ring over the keys of every claimant that registered. Registration costs one
//! flat price and grants the game's slots to everyone who pays it, so no claim is provable against
//! a set narrower than the whole registration. Registration is first-come, first-served up to
//! [`Config::MaxPrivateRingKeys`], which is the room one ring of
//! [`Config::PrivateRingExponent`] holds.
//!
//! A claim reveals the alias of one context and nothing else. The anonymity set of every claim is
//! the game's whole ring, whichever slot it names.
//!
//! A game whose registration stays below its anonymity floor gets no ring, because a ring that
//! small hides nobody, and neither does one whose build keeps failing. Either way the game is
//! abandoned and the claims chain is told, which reopens the public claim path for the game's
//! credit trees. Nobody can have claimed privately by then, there being no ring to prove against,
//! so no credit is claimed twice.
//!
//! The floor is [`Config::MinPrivateRingKeys`], raised to
//! [`Config::MinPrivateRingParticipation`] of the claimants that earned the entry price. An
//! absolute floor alone is a fixed number of keys to buy: a group that registers with keys it
//! never claims with fills the set of one target, and a floor of sixteen in a game of hundreds is
//! sixteen keys. The share ties the cost of that to the size of the game.

use alloc::vec::Vec;
use codec::{Compact, MaxEncodedLen};
use cumulus_primitives_core::GetChannelInfo;
use frame_support::{ensure, pallet_prelude::*, traits::UnixTime, weights::Weight};
use frame_system::pallet_prelude::BlockNumberFor;
use indiv_pallet_chunks_manager::ChunksApi;
use indiv_pallet_game::{GameIdx, GameTimes};
use indiv_support::{
	credit_trees::{PrivateClaimSlot, PrivateGameOutcome, PrivateRingDelivery},
	identity::AccountOrPerson,
};
use sp_runtime::{traits::Zero, DispatchError, DispatchResult, SaturatedConversion};
use verifiable::GenerateVerifiable;
use xcm::{latest::prelude::*, VersionedXcm};

use crate::{
	pallet::*, AuthorizeInvalidity, Config, Error, Event, NftClaimsCall, Pallet, PrivateGameInfo,
	PrivateGamePhase, PrivateRingBatchOf, WeightInfo as _, LOG_TARGET,
};

/// How many registration entries one [`Pallet::clean_up_private_game`] call removes.
///
/// A game holds one registration and one credit balance per credited player, so the cleanup runs
/// in bounded steps. It is a pallet constant because only the weight of one call depends on it.
pub const PRIVATE_CLEAN_UP_ITEMS: u32 = 32;

/// How many failed build steps a private game takes before it is abandoned.
///
/// A push fails on a trusted-setup chunk the ring cannot be built from, which is a chain
/// configuration the next block does not repair. Without a limit the game keeps its keys, its
/// registrations and its credits forever, and none of them can be claimed. It is a pallet
/// constant because no runtime has a reason to pick a different number.
pub const PRIVATE_RING_BUILD_RETRIES: u8 = 8;

impl<T: Config> Pallet<T> {
	/// Open the private claim path of `game_index` if its game opted into one, and report how many
	/// slots it grants. Zero is a public game.
	///
	/// Call it when the game's first credit is awarded. That is the last moment the game is
	/// readable, because it is killed once its player process ends.
	pub(crate) fn note_private_game(game_index: GameIdx) -> PrivateClaimSlot {
		if let Some(info) = PrivateGames::<T>::get(game_index) {
			return info.slots;
		}

		// The running game is the only one whose schedule is still readable, so a credit awarded
		// for another game opens no private path. Play awards reach the running game only, while
		// the testnet grant can name another.
		let Some(game) =
			indiv_pallet_game::Game::<T>::get().filter(|game| game.index == game_index)
		else {
			return 0;
		};
		let Some(setting) = game.private_claims else {
			return 0;
		};

		// Registration opens when the credits are final, at the end of the player process, and
		// runs for the configured window.
		let player_process_end = GameTimes::<T>::player_process_end(&game);
		PrivateGames::<T>::insert(
			game_index,
			PrivateGameInfo {
				slots: setting.slots,
				registration_starts: player_process_end,
				registration_ends: player_process_end
					.saturating_add(T::PrivateRegistrationSeconds::get()),
				key_count: 0,
				eligible_players: 0,
				phase: PrivateGamePhase::Building { included: 0, failures: 0 },
			},
		);

		setting.slots
	}

	/// Credit `claimant` with one spendable credit of a private game.
	///
	/// The caller has read the game's slots from [`Pallet::note_private_game`], so this does not
	/// read the game again.
	pub(crate) fn note_private_credit(
		game_index: GameIdx,
		claimant: &AccountOrPerson<T::AccountId>,
	) {
		let balance = PrivateCreditBalances::<T>::mutate(game_index, claimant, |balance| {
			*balance = balance.saturating_add(1);
			*balance
		});

		// A claimant below the entry price cannot register, so the anonymity floor counts each
		// one as their credits reach it. Every award lands before registration opens, which is
		// what makes the count final by then.
		if balance != T::PrivateClaimEntryCredits::get() {
			return;
		}
		PrivateGames::<T>::mutate(game_index, |info| match info {
			Some(info) => info.eligible_players = info.eligible_players.saturating_add(1),
			None => log::error!(
				target: LOG_TARGET,
				"Private credit noted for game {game_index}, which has no private path",
			),
		});
	}

	/// The keys `info`'s ring has to hold, which is the anonymity set each of its claims gets.
	///
	/// It is the greater of [`Config::MinPrivateRingKeys`] and
	/// [`Config::MinPrivateRingParticipation`] of the claimants that can afford to register,
	/// capped at the room a registration has. The share is what keeps the floor meaningful in a
	/// game far larger than the absolute one: a group registering to fill one target's set has
	/// to grow with the game.
	fn private_ring_floor(info: &PrivateGameInfo) -> u32 {
		let share = T::MinPrivateRingParticipation::get().mul_ceil(info.eligible_players);

		T::MinPrivateRingKeys::get().max(share).min(T::MaxPrivateRingKeys::get())
	}

	/// Whether registration for `game_index` is open right now.
	///
	/// Both ends matter. Before the player process is over the credits are not final, so a
	/// claimant's balance need not yet cover the entry price.
	fn private_registration_open(info: &PrivateGameInfo) -> bool {
		let now = T::UnixTime::now().as_secs().saturated_into::<u32>();
		info.accepts_keys(now)
	}

	/// Whether registration for `game_index` is over, which is when its ring can be built.
	///
	/// This is not the opposite of [`Pallet::private_registration_open`]. A game whose player
	/// process still runs has opened no registration, and its ring must not be built either.
	fn private_registration_closed(info: &PrivateGameInfo) -> bool {
		let now = T::UnixTime::now().as_secs().saturated_into::<u32>();
		now >= info.registration_ends
	}

	/// The body of [`Pallet::register_private_claim_key`].
	pub(crate) fn do_register_private_claim_key(
		game_index: GameIdx,
		claimant: AccountOrPerson<T::AccountId>,
		key: PrivateRingKey<T>,
	) -> DispatchResult {
		let mut info = PrivateGames::<T>::get(game_index).ok_or(Error::<T>::NotAPrivateGame)?;

		ensure!(Self::private_registration_open(&info), Error::<T>::PrivateRegistrationClosed);
		ensure!(T::RingVrf::is_member_valid(&key), Error::<T>::InvalidRingKey);

		ensure!(
			!PrivateRegistrations::<T>::contains_key(game_index, &claimant),
			Error::<T>::AlreadyRegistered
		);

		let price = T::PrivateClaimEntryCredits::get();
		let balance = PrivateCreditBalances::<T>::get(game_index, &claimant);
		ensure!(balance >= price, Error::<T>::InsufficientCredits);

		PrivateRingKeys::<T>::try_mutate(game_index, |keys| {
			// Registrations are public, so a claimant can read another's key and enrol it. The
			// ring would then count a member it does not have, and `MinPrivateRingKeys` would
			// pass on a set smaller than it names. The list is decoded here anyway.
			ensure!(!keys.contains(&key), Error::<T>::DuplicateRingKey);
			keys.try_push(key).map_err(|_| Error::<T>::PrivateRingFull)?;
			Ok::<(), Error<T>>(())
		})?;

		let remaining = balance.saturating_sub(price);
		if remaining.is_zero() {
			PrivateCreditBalances::<T>::remove(game_index, &claimant);
		} else {
			PrivateCreditBalances::<T>::insert(game_index, &claimant, remaining);
		}
		PrivateRegistrations::<T>::insert(game_index, &claimant, ());

		info.key_count = info.key_count.saturating_add(1);
		PrivateGames::<T>::insert(game_index, info);

		Self::deposit_event(Event::<T>::PrivateClaimKeyRegistered {
			game_index,
			claimant,
			credits: price,
		});

		Ok(())
	}

	/// How many keys the build step due for `game_index` pushes, or `None` when the game owes no
	/// build work.
	///
	/// Zero closes the ring: it finishes the root, or abandons a ring that is too small or that
	/// failed to build, and moves the game on to its cleanup.
	///
	/// Registration must be closed first. A root built while keys still arrive changes under the
	/// claimants that already proved against it.
	pub(crate) fn private_ring_build_step(game_index: GameIdx) -> Option<u32> {
		let info = PrivateGames::<T>::get(game_index)?;
		let PrivateGamePhase::Building { included, failures } = info.phase else {
			return None;
		};
		if !Self::private_registration_closed(&info) {
			return None;
		}

		// The same zero-key step abandons a ring below the floor, and one whose retries are
		// spent.
		if info.key_count < Self::private_ring_floor(&info) ||
			failures >= PRIVATE_RING_BUILD_RETRIES
		{
			return Some(0);
		}

		let outstanding = info.key_count.saturating_sub(included);

		Some(outstanding.min(T::PrivateKeysPerBuild::get()))
	}

	/// Validate a [`Pallet::build_private_ring`] submission.
	pub(crate) fn authorize_build_private_ring(
		source: TransactionSource,
		game_index: &GameIdx,
		to_include: &u32,
	) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
		if !matches!(source, TransactionSource::InBlock | TransactionSource::Local) {
			return Err(AuthorizeInvalidity::TransactionNotLocal.into());
		}

		let expected = Self::private_ring_build_step(*game_index)
			.ok_or(AuthorizeInvalidity::NoPrivateRingToBuild)?;
		if expected != *to_include {
			return Err(AuthorizeInvalidity::NoPrivateRingToBuild.into());
		}

		Ok((
			ValidTransaction {
				priority: indiv_support::tx_priority::BACKGROUND_PROGRESS,
				requires: Vec::new(),
				provides: Vec::from([(b"nft-credits/build-private-ring", game_index).encode()]),
				longevity: crate::CREDIT_TREE_TX_LONGEVITY,
				propagate: false,
			},
			Weight::zero(),
		))
	}

	/// The body of [`Pallet::build_private_ring`].
	pub(crate) fn do_build_private_ring(game_index: GameIdx, to_include: u32) -> DispatchResult {
		let mut info = PrivateGames::<T>::get(game_index).ok_or(Error::<T>::NotAPrivateGame)?;
		let PrivateGamePhase::Building { included, failures } = info.phase else {
			return Err(Error::<T>::NoPrivateRingToBuild.into());
		};

		let key_count = info.key_count;

		// A ring too few claimants registered for is not built, because counting its members
		// names them, and one whose retries are spent is not built either. The game falls back
		// to the public path instead.
		if key_count < Self::private_ring_floor(&info) || failures >= PRIVATE_RING_BUILD_RETRIES {
			Self::abandon_private_game(game_index, info);
			return Ok(());
		}

		if to_include.is_zero() {
			ensure!(included >= key_count, Error::<T>::NoPrivateRingToBuild);

			let intermediate = PrivateRingIntermediates::<T>::take(game_index)
				.ok_or(Error::<T>::NoPrivateRingToBuild)?;
			let root = T::RingVrf::finish_members(intermediate);
			PrivateOutcomes::<T>::insert(game_index, PrivateGameOutcome::Ring { root, key_count });
			info.phase = PrivateGamePhase::CleaningUp;
			PrivateGames::<T>::insert(game_index, info);
			Self::queue_private_ring_delivery(game_index);
			Self::deposit_event(Event::<T>::PrivateRingBuilt { game_index, key_count });

			return Ok(());
		}

		let Ok(capacity) = T::PrivateRingExponent::get().try_into() else {
			// The runtime's exponent is one the crypto does not take, so no retry builds this
			// ring or any other.
			Self::note_private_build_failure(game_index, info, "the ring exponent is invalid");
			return Ok(());
		};
		let mut intermediate = PrivateRingIntermediates::<T>::get(game_index)
			.unwrap_or_else(|| T::RingVrf::start_members(capacity));

		let pushed = PrivateRingKeys::<T>::get(game_index)
			.into_iter()
			.skip(included as usize)
			.take(to_include as usize)
			.collect::<Vec<_>>();
		let pushed_count = pushed.len() as u32;

		if T::RingVrf::push_members(&mut intermediate, pushed.into_iter(), |range| {
			T::ChunksManager::get_chunks(
				T::PrivateRingExponent::get(),
				range.start.saturated_into(),
				range.end.saturated_into(),
			)
			.map_err(|_| ())
		})
		.is_err()
		{
			// The step keeps the failure rather than reverting it, so the retries are counted.
			// A push fails on the trusted-setup chunks the ring is built from, which the next
			// block serves just as it served this one.
			Self::note_private_build_failure(game_index, info, "pushing the keys failed");
			return Ok(());
		}

		PrivateRingIntermediates::<T>::insert(game_index, intermediate);
		info.phase = PrivateGamePhase::Building {
			included: included.saturating_add(pushed_count),
			failures: 0,
		};
		PrivateGames::<T>::insert(game_index, info);

		Ok(())
	}

	/// Count one failed build step of `game_index`.
	///
	/// The step that follows [`PRIVATE_RING_BUILD_RETRIES`] failures is the closing one, which
	/// abandons the game. Counting the failures here and giving up there keeps every abandonment
	/// on the branch its weight is measured on.
	///
	/// `info` is the game's record as the failed step read it, so the caller must not have
	/// written it back.
	fn note_private_build_failure(game_index: GameIdx, mut info: PrivateGameInfo, reason: &str) {
		let PrivateGamePhase::Building { included, failures } = info.phase else {
			log::error!(
				target: LOG_TARGET,
				"Build failure noted for game {game_index}, which owes no build step",
			);
			return;
		};

		let failures = failures.saturating_add(1);
		log::warn!(
			target: LOG_TARGET,
			"Private ring build step {failures} for game {game_index} failed: {reason}",
		);

		info.phase = PrivateGamePhase::Building { included, failures };
		PrivateGames::<T>::insert(game_index, info);
		Self::deposit_event(Event::<T>::PrivateRingBuildFailed { game_index, failures });
	}

	/// Give up on `game_index`'s ring and queue the abandonment for the claims chain, which
	/// reopens the public claim path for the game's credit trees.
	///
	/// `info` is the game's record with its phase not yet advanced.
	fn abandon_private_game(game_index: GameIdx, mut info: PrivateGameInfo) {
		let key_count = info.key_count;
		let required = Self::private_ring_floor(&info);

		// A ring half-pushed before the retries ran out commits to nothing anyone can prove
		// against.
		PrivateRingIntermediates::<T>::remove(game_index);
		PrivateOutcomes::<T>::insert(game_index, PrivateGameOutcome::Abandoned { key_count });
		info.phase = PrivateGamePhase::CleaningUp;
		PrivateGames::<T>::insert(game_index, info);
		Self::queue_private_ring_delivery(game_index);
		Self::deposit_event(Event::<T>::PrivateRingAbandoned { game_index, key_count, required });
	}

	/// Validate a [`Pallet::clean_up_private_game`] submission.
	pub(crate) fn authorize_clean_up_private_game(
		source: TransactionSource,
		game_index: &GameIdx,
	) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
		if !matches!(source, TransactionSource::InBlock | TransactionSource::Local) {
			return Err(AuthorizeInvalidity::TransactionNotLocal.into());
		}

		if !Self::private_clean_up_due(*game_index) {
			return Err(AuthorizeInvalidity::NoPrivateGameToCleanUp.into());
		}

		Ok((
			ValidTransaction {
				priority: indiv_support::tx_priority::BACKGROUND_PROGRESS,
				requires: Vec::new(),
				provides: Vec::from([(b"nft-credits/clean-up-private-game", game_index).encode()]),
				longevity: crate::CREDIT_TREE_TX_LONGEVITY,
				propagate: false,
			},
			Weight::zero(),
		))
	}

	/// Whether `game_index` has registration state left to drop.
	pub(crate) fn private_clean_up_due(game_index: GameIdx) -> bool {
		PrivateGames::<T>::get(game_index)
			.is_some_and(|info| matches!(info.phase, PrivateGamePhase::CleaningUp))
	}

	/// The body of [`Pallet::clean_up_private_game`], returning the entries it removed.
	///
	/// One call removes at most [`PRIVATE_CLEAN_UP_ITEMS`] entries, so a game is dropped over as
	/// many calls as it takes. The game's record goes last, because it is what says the cleanup is
	/// still owed. The ring outlives the cleanup and is removed once the claims chain has it.
	pub(crate) fn do_clean_up_private_game(game_index: GameIdx) -> Result<u32, DispatchError> {
		ensure!(Self::private_clean_up_due(game_index), Error::<T>::NoPrivateGameToCleanUp);

		// The keys are one entry and the ring that commits to them is built, so they go first and
		// in full. Every step's base cost covers their removal, and `removed` counts only what
		// the refund is measured in.
		PrivateRingKeys::<T>::remove(game_index);

		// The claimants are read before they are removed, rather than cleared by prefix, so that
		// the count the refund is measured in is exact.
		let claimants = PrivateRegistrations::<T>::iter_key_prefix(game_index)
			.take(PRIVATE_CLEAN_UP_ITEMS as usize)
			.collect::<Vec<_>>();
		let registrations = claimants.len() as u32;
		for claimant in &claimants {
			PrivateRegistrations::<T>::remove(game_index, claimant);
		}

		// A step that spent its budget on the registrations leaves the credits to the next one,
		// which keeps every step's cost bounded.
		let budget = PRIVATE_CLEAN_UP_ITEMS.saturating_sub(registrations);
		if budget.is_zero() {
			return Ok(registrations);
		}

		// The credits nobody spent on a registration. They are the private path's own
		// bookkeeping: a game that built its ring mints through it alone, and an abandoned game
		// mints through the credit trees, which this does not touch.
		let claimants = PrivateCreditBalances::<T>::iter_key_prefix(game_index)
			.take(budget as usize)
			.collect::<Vec<_>>();
		let balances = claimants.len() as u32;
		for claimant in &claimants {
			PrivateCreditBalances::<T>::remove(game_index, claimant);
		}

		if balances < budget {
			PrivateGames::<T>::remove(game_index);
			Self::deposit_event(Event::<T>::PrivateGameCleanedUp { game_index });
		}

		Ok(registrations.saturating_add(balances))
	}

	/// Queue the ring of `game_index` for delivery to the claims chain.
	fn queue_private_ring_delivery(game_index: GameIdx) {
		if PrivateRingDeliveryQueue::<T>::mutate(|queue| queue.try_push(game_index)).is_err() {
			// The queue fills only after delivery failed for as many rings as it holds. A replay
			// has to repair that.
			log::error!(
				target: LOG_TARGET,
				"Private ring delivery queue full, ring for game {game_index} not queued",
			);
		}
	}

	/// Validate a [`Pallet::send_private_ring`] submission.
	pub(crate) fn authorize_send_private_ring(
		source: TransactionSource,
		game_index: &GameIdx,
	) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
		if !matches!(source, TransactionSource::InBlock | TransactionSource::Local) {
			return Err(AuthorizeInvalidity::TransactionNotLocal.into());
		}

		let queue = PrivateRingDeliveryQueue::<T>::get();
		if queue.first() != Some(game_index) {
			return Err(AuthorizeInvalidity::NoQueuedPrivateRings.into());
		}

		Ok((
			ValidTransaction {
				priority: indiv_support::tx_priority::BACKGROUND_PROGRESS,
				requires: Vec::new(),
				provides: Vec::from([(b"nft-credits/send-private-ring", game_index).encode()]),
				longevity: crate::CREDIT_TREE_TX_LONGEVITY,
				propagate: false,
			},
			Weight::zero(),
		))
	}

	/// The per-message room the claims channel needs for one private claim ring, which the router
	/// compares against its `max_message_size`.
	///
	/// A ring root is far larger than a Merkle root, so it is measured on its own and not against
	/// the credit trees' capacity. Every field of a delivery is fixed-size, so its
	/// `MaxEncodedLen` is the size of the one ring a message carries.
	pub fn private_ring_channel_size() -> u32 {
		let empty = PrivateRingBatchOf::<T> { source_time: 0, rings: BoundedVec::default() };
		let empty_call =
			(u8::MAX, NftClaimsCall::<T>::ReceivePrivateRings { batch: empty }).encode();

		// The call once the batch holds its one delivery. The empty vector's length prefix gives
		// way to the prefix for a single ring.
		let call_len = empty_call.len() - Compact(0u32).encoded_size() +
			Compact(1u32).encoded_size() +
			PrivateRingDelivery::<crate::PrivateRingRoot<T>>::max_encoded_len();
		// The XCM around the call. Only the router knows what the envelope encodes to, so it is
		// measured with the empty call in it and subtracted again.
		let envelope = VersionedXcm::<()>::from(Self::credit_tree_xcm(empty_call.clone()))
			.encode()
			.len() - empty_call.encoded_size();

		(envelope +
			Compact(call_len as u32).encoded_size() +
			call_len + crate::CREDIT_TREE_ROUTER_HEADROOM) as u32
	}

	/// Whether the claims channel takes a message carrying one private game outcome.
	///
	/// Checked before every delivery. The router drops a message that does not fit, and until
	/// the claims chain has the outcome the game's credits mint on neither path.
	fn private_ring_fits_channel() -> bool {
		T::ChannelInfo::get_channel_info(T::NftClaimsParaId::get())
			.is_some_and(|info| Self::private_ring_channel_size() <= info.max_message_size)
	}

	/// The body of [`Pallet::send_private_ring`].
	///
	/// A message that cannot be sent leaves the queue untouched and reports
	/// `PrivateRingSendFailed`, so the next offchain-worker cycle retries the same ring. The call
	/// succeeds either way, because a failing dispatch would revert that event.
	pub(crate) fn do_send_private_ring(game_index: GameIdx) -> DispatchResult {
		let info = PrivateGames::<T>::get(game_index).ok_or(Error::<T>::NotAPrivateGame)?;
		let outcome = PrivateOutcomes::<T>::get(game_index).ok_or(Error::<T>::NotAPrivateGame)?;

		if !Self::private_ring_fits_channel() {
			log::warn!(
				target: LOG_TARGET,
				"No channel room for a private ring ({} bytes), retrying next offchain worker cycle",
				Self::private_ring_channel_size(),
			);
			Self::deposit_event(Event::<T>::PrivateRingSendFailed { game_index });
			return Ok(());
		}

		let delivery = PrivateRingDelivery { game_index, slots: info.slots, outcome };

		let rings = BoundedVec::try_from(Vec::from([delivery])).map_err(|_| {
			frame_support::defensive!("a one-ring batch must fit a one-ring bound");
			Error::<T>::PrivateRingXcmFailed
		})?;
		let batch = PrivateRingBatchOf::<T> { source_time: T::UnixTime::now().as_secs(), rings };

		let call =
			(T::NftClaimsPalletIndex::get(), NftClaimsCall::<T>::ReceivePrivateRings { batch })
				.encode();
		let destination = Location::new(1, [Parachain(T::NftClaimsParaId::get().into())]);

		if let Err(error) = send_xcm::<T::XcmRouter>(destination, Self::credit_tree_xcm(call)) {
			log::warn!(
				target: LOG_TARGET,
				"Private ring XCM failed: {error:?}, retrying next offchain worker cycle",
			);
			Self::deposit_event(Event::<T>::PrivateRingSendFailed { game_index });
			return Ok(());
		}

		PrivateRingDeliveryQueue::<T>::mutate(|queue| {
			if queue.first() == Some(&game_index) {
				queue.remove(0);
			}
		});
		PrivateOutcomes::<T>::remove(game_index);
		Self::deposit_event(Event::<T>::PrivateRingSent { game_index });

		Ok(())
	}

	/// Submits the private claim work of the moment: one build step, one delivery, or one cleanup
	/// step.
	///
	/// Building comes first, because an unfinished ring has nothing to deliver. Delivery comes
	/// before cleanup, because claims wait on the ring and on nothing the cleanup drops. One
	/// ring fills a message, so only the front of the queue is delivered per block.
	pub(crate) fn submit_private_ring_work(block_number: BlockNumberFor<T>) {
		let discriminator = block_number / crate::CREDIT_TREE_RETRY_WINDOW.into();

		let mut cleanup = None;
		for (game_index, _) in PrivateGames::<T>::iter() {
			if let Some(to_include) = Self::private_ring_build_step(game_index) {
				Self::submit_private_call(
					Call::<T>::build_private_ring { game_index, to_include, discriminator },
					"build_private_ring",
				);
				return;
			}
			if cleanup.is_none() && Self::private_clean_up_due(game_index) {
				cleanup = Some(game_index);
			}
		}

		if let Some(game_index) = PrivateRingDeliveryQueue::<T>::get().first().copied() {
			Self::submit_private_call(
				Call::<T>::send_private_ring { game_index, discriminator },
				"send_private_ring",
			);
			return;
		}

		if let Some(game_index) = cleanup {
			Self::submit_private_call(
				Call::<T>::clean_up_private_game { game_index, discriminator },
				"clean_up_private_game",
			);
		}
	}

	/// Submit one authorized private claim call, logging a rejection rather than failing.
	fn submit_private_call(call: Call<T>, name: &str) {
		let tx = <T as frame_system::offchain::CreateAuthorizedTransaction<Call<T>>>::
			create_authorized_transaction(call.into());
		if frame_system::offchain::SubmitTransaction::<T, Call<T>>::submit_transaction(tx).is_err()
		{
			log::debug!(
				target: LOG_TARGET,
				"offchain worker: failed to submit `{name}`",
			);
		}
	}

	/// Assert that the private claim path's calls fit the offchain-worker block budget, and that
	/// its entry price stays inside what one game awards.
	#[cfg(feature = "std")]
	pub(crate) fn private_integrity_test(budget: &indiv_support::weight_budget::OcwWeightBudget) {
		// One game awards a claimant one credit per co-player that reports them a person in each
		// round, so a full attendance holds this many. Nobody can pay an entry price above it,
		// and every game's ring is then abandoned.
		let price = T::PrivateClaimEntryCredits::get();
		let max_credits =
			T::MaxRounds::get().saturating_mul(T::MaxGroupSize::get().saturating_sub(1));
		assert!(
			price <= max_credits,
			"`PrivateClaimEntryCredits` ({price}) is above the {max_credits} credits one game \
			 awards",
		);

		// A game queues one ring, so the queue has to hold the games a delivery outage spans.
		assert!(
			!T::MaxQueuedPrivateRings::get().is_zero(),
			"`MaxQueuedPrivateRings` must be at least one",
		);

		budget.assert_fits(
			"build_private_ring",
			<T as Config>::WeightInfo::build_private_ring(T::PrivateKeysPerBuild::get())
				.saturating_add(<T as Config>::WeightInfo::authorize_build_private_ring()),
		);
		budget.assert_fits(
			"finish_private_ring",
			<T as Config>::WeightInfo::finish_private_ring()
				.saturating_add(<T as Config>::WeightInfo::authorize_build_private_ring()),
		);
		budget.assert_fits(
			"send_private_ring",
			<T as Config>::WeightInfo::send_private_ring()
				.saturating_add(<T as Config>::WeightInfo::authorize_send_private_ring())
				.saturating_add(T::PrivateRingRemoteWeight::get()),
		);
		budget.assert_fits(
			"clean_up_private_game",
			<T as Config>::WeightInfo::clean_up_private_game(PRIVATE_CLEAN_UP_ITEMS)
				.saturating_add(<T as Config>::WeightInfo::authorize_clean_up_private_game()),
		);
	}
}
