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

//! The private claim path: the ring ladder one game's claimants register into, and its delivery.
//!
//! A game that opts in mints privately only. Its credits still form the per-block Merkle trees,
//! which record who earned what, but the claims chain refuses a public claim against such a tree
//! and takes the ladder instead.
//!
//! ## The ladder
//!
//! Ring `t` holds the key of every registrant that earned at least `t` credits, so ring 1 contains
//! ring 2 contains ring 3. A claimant proves membership in rings 1 to their own credit count, once
//! each, and mints one NFT per proof. A registrant mints what they earned rather than a flat
//! number.
//!
//! The tiers a claimant proves against narrow them down no further than the highest one does,
//! because a proof against ring 5 already implies rings 1 to 4. A tier states nothing new about
//! earnings either: `AwardedNftClaimCredits` is keyed by `(game, claimant)`, so who earned what is
//! public whatever the rings do.
//!
//! The rings are prefixes of one push sequence, so the ladder costs one pass over the keys. A
//! registration goes into the bucket of its owner's credit count. The build pushes the buckets in
//! descending order and clones the intermediate at each bucket boundary, which is the ladder's
//! whole extra cost against a single ring.
//!
//! Registration costs one credit, the ladder's base, so every claimant that earned anything is in
//! ring 1 and mints at least once. A key that pads ring `t` has to carry `t` credits, so the
//! smallest sets cost the most to fill.
//!
//! ## The ladder's height
//!
//! A ring holds fewer keys the higher its tier sits and a ring of one names its claimant. A tier
//! is built only if its ring clears the game's anonymity floor, which is
//! [`Config::MinPrivateRingKeys`] raised to [`Config::MinPrivateRingParticipation`] of the
//! claimants that could have registered for it. The height is the first tier that falls short,
//! less one.
//!
//! Both the ring and the floor shrink as the tier rises, so a tier above one that fell short can
//! clear its own floor. Such a tier is dropped with the rest: the ladder is the range 1 to its
//! height, which is what the delivery carries and what a claim indexes into, so it holds no gap.
//! Credits above the height are forfeit.
//!
//! An absolute floor alone is a fixed number of keys to buy: a group that registers with keys it
//! never claims with fills the set of one target, and a floor of sixteen in a game of hundreds is
//! sixteen keys. The share ties that cost to the size of the game.
//!
//! Ring 1 is every registrant, so a game whose ring 1 falls short has no ring anyone can hide in.
//! It is abandoned and the claims chain is told, which reopens the public claim path for the
//! game's credit trees. Nobody can have claimed privately by then, the outcome being delivered
//! only once the ladder is whole, so no credit is claimed twice.
//!
//! [`Config::MaxPrivateRingTiers`] caps the height. A game whose claimants earn more credits than
//! the runtime carries tiers for forfeits the top ones.

use alloc::vec::Vec;
use codec::{Compact, MaxEncodedLen};
use cumulus_primitives_core::GetChannelInfo;
use frame_support::{ensure, pallet_prelude::*, traits::UnixTime, weights::Weight};
use frame_system::pallet_prelude::BlockNumberFor;
use indiv_pallet_chunks_manager::ChunksApi;
use indiv_pallet_game::{GameIdx, GameTimes};
use indiv_support::{
	credit_trees::{ClaimPath, PrivateClaimTier, PrivateGameOutcome},
	identity::AccountOrPerson,
	offchain::{RETRY_WINDOW, TX_LONGEVITY},
};
use sp_runtime::{traits::Zero, DispatchError, DispatchResult, SaturatedConversion};
use verifiable::GenerateVerifiable;
use xcm::{latest::prelude::*, VersionedXcm};

use crate::{
	pallet::*, AuthorizeInvalidity, Config, Error, Event, NftClaimsCall, Pallet,
	PrivateClaimantState, PrivateGameInfoOf, PrivateGamePhase, PrivateRingBatchOf,
	PrivateRingDeliveryOf, PrivateTierCounts, WeightInfo as _, LOG_TARGET,
};

/// How many registration entries one [`Pallet::clean_up_private_game`] call removes.
///
/// A game holds one claimant entry per credited player and one key-index entry per registrant, so
/// the cleanup runs in bounded steps. It is a pallet constant because only the weight of one call
/// depends on it.
pub const PRIVATE_CLEAN_UP_ITEMS: u32 = 32;

/// How many failed build steps a private game takes before it is abandoned.
///
/// A push fails on a trusted-setup chunk the ring cannot be built from, which the next block
/// serves just as it served this one. Without a limit the game holds its keys, its registrations
/// and its credits forever and nothing can claim them. It is a pallet constant because no runtime
/// has a reason to pick a different number.
pub const PRIVATE_RING_BUILD_RETRIES: u8 = 8;

impl<T: Config> Pallet<T> {
	/// Open the private claim path of `game_index` if its game opted into one, and report the
	/// path. [`None`] is a public game.
	///
	/// Call it when the game's first credit is awarded. That is the last moment the game is
	/// readable, because it is killed once its player process ends.
	pub(crate) fn note_private_game(game_index: GameIdx) -> Option<PrivateGameInfoOf<T>> {
		if let Some(info) = PrivateGames::<T>::get(game_index) {
			return Some(info);
		}

		// The running game is the only one whose schedule is still readable, so a credit awarded
		// for another game opens no private path. Play awards reach the running game only, while
		// the testnet grant can name another.
		let game = indiv_pallet_game::Game::<T>::get().filter(|game| game.index == game_index)?;
		if game.claims == ClaimPath::Public {
			return None;
		}

		// Key registration opens when the credits are final, at the end of the player process, and
		// runs for the configured window.
		let player_process_end = GameTimes::<T>::player_process_end(&game);
		let info = PrivateGameInfoOf::<T> {
			key_registration_starts: player_process_end,
			key_registration_ends: player_process_end
				.saturating_add(T::PrivateKeyRegistrationSeconds::get()),
			key_count: 0,
			tiers: Self::private_tiers(game.rounds, game.max_group_size),
			phase: PrivateGamePhase::Registering,
		};
		PrivateGames::<T>::insert(game_index, info.clone());

		Some(info)
	}

	/// The tier counters a game of this shape needs, one per credit a claimant can earn.
	///
	/// A full attendance earns one credit per co-player in each round, which is the tallest ladder
	/// the game can fill. A runtime whose [`Config::MaxPrivateRingTiers`] is below that carries
	/// fewer and the credits above the last tier are forfeit.
	fn private_tiers(
		rounds: u8,
		max_group_size: u32,
	) -> BoundedVec<PrivateTierCounts, T::MaxPrivateRingTiers> {
		let full_attendance = u32::from(rounds).saturating_mul(max_group_size.saturating_sub(1));
		let tiers = full_attendance.min(T::MaxPrivateRingTiers::get()).max(1);

		BoundedVec::try_from(vec![PrivateTierCounts::default(); tiers as usize]).unwrap_or_else(
			|_| {
				// The count is capped at `MaxPrivateRingTiers`, which is the bound itself.
				frame_support::defensive!("a game's tiers must fit `MaxPrivateRingTiers`");
				BoundedVec::default()
			},
		)
	}

	/// Note that `claimant` holds `credits` of the private game `info` describes and move them up
	/// one tier.
	///
	/// `credits` is the count of [`AwardedNftClaimCredits`], which the caller has just written, so
	/// this counts nothing of its own. Every award lands before registration opens, so the count
	/// is final by then and names the tier the claimant's key goes into.
	pub(crate) fn note_private_credit(
		game_index: GameIdx,
		claimant: &AccountOrPerson<T::AccountId>,
		credits: u32,
		mut info: PrivateGameInfoOf<T>,
	) {
		// A claimant that earned more credits than the runtime carries tiers for stays in the top
		// one, so neither record changes.
		if !info.note_tier_credit(credits) {
			return;
		}
		PrivateGames::<T>::insert(game_index, info);
		PrivateClaimants::<T>::insert(
			game_index,
			claimant,
			PrivateClaimantState::Eligible { credits },
		);
	}

	/// The keys tier `tier` of `info`'s ladder has to hold, which is the anonymity set its claims
	/// get.
	///
	/// It is the greater of [`Config::MinPrivateRingKeys`] and
	/// [`Config::MinPrivateRingParticipation`] of the claimants that could register for the tier,
	/// capped at the room a registration has. It is measured per tier, so a game whose upper tiers
	/// are thin still serves its lower ones.
	fn private_ring_floor(info: &PrivateGameInfoOf<T>, tier: PrivateClaimTier) -> u32 {
		let share = T::MinPrivateRingParticipation::get().mul_ceil(info.eligible_at(tier));

		T::MinPrivateRingKeys::get().max(share).min(T::MaxPrivateRingKeys::get())
	}

	/// The tallest tier of `info` whose ring hides its claimants, which is the ladder's height.
	///
	/// It is the first tier that falls short, less one. A tier above one that fell short can clear
	/// its own floor and is given up with the rest, because a ladder is the range 1 to its height
	/// and holds no gap. Zero means tier 1 itself fell short, which abandons the game.
	fn private_ladder_height(info: &PrivateGameInfoOf<T>) -> PrivateClaimTier {
		let mut height = 0;
		for tier in 1..=info.tiers.len() as PrivateClaimTier {
			if info.ring_size(tier) < Self::private_ring_floor(info, tier) {
				break;
			}
			height = tier;
		}

		height
	}

	/// Whether key registration for `game_index` is open right now.
	///
	/// Both ends matter. Before the player process is over the credits are not final, so the tier
	/// a claimant's key belongs in is undecided.
	fn private_key_registration_open(info: &PrivateGameInfoOf<T>) -> bool {
		let now = T::UnixTime::now().as_secs().saturated_into::<u32>();
		info.accepts_keys(now)
	}

	/// Whether key registration for `game_index` is over, which is when its ladder can be built.
	///
	/// This is not the opposite of [`Pallet::private_key_registration_open`]. A game whose player
	/// process still runs has opened no key registration and its ladder must not be built.
	fn private_key_registration_closed(info: &PrivateGameInfoOf<T>) -> bool {
		let now = T::UnixTime::now().as_secs().saturated_into::<u32>();
		now >= info.key_registration_ends
	}

	/// The body of [`Pallet::register_private_claim_key`].
	pub(crate) fn do_register_private_claim_key(
		game_index: GameIdx,
		claimant: AccountOrPerson<T::AccountId>,
		key: PrivateRingKey<T>,
	) -> DispatchResult {
		let mut info = PrivateGames::<T>::get(game_index).ok_or(Error::<T>::NotAPrivateGame)?;

		ensure!(
			Self::private_key_registration_open(&info),
			Error::<T>::PrivateKeyRegistrationClosed
		);
		ensure!(T::RingVrf::is_member_valid(&key), Error::<T>::InvalidRingKey);

		let credits = match PrivateClaimants::<T>::get(game_index, &claimant) {
			Some(PrivateClaimantState::Eligible { credits }) => credits,
			Some(PrivateClaimantState::Registered { .. }) =>
				return Err(Error::<T>::AlreadyRegistered.into()),
			None => return Err(Error::<T>::InsufficientCredits.into()),
		};

		// Registrations are public, so a claimant can read another's key and enrol it. A ring
		// would then count a member it does not have and the floor would pass on a set smaller
		// than it names. The check is a lookup because scanning the buckets reads all of them.
		ensure!(
			!PrivateRingKeyIndex::<T>::contains_key(game_index, &key),
			Error::<T>::DuplicateRingKey
		);

		// Tier 1 is every registrant, so the game's whole registration has to fit one ring. The
		// bound is on the total and not on a bucket: a game that took a full bucket per tier
		// builds no ring at all and puts every claimant back on the public path.
		ensure!(info.key_count < T::MaxPrivateRingKeys::get(), Error::<T>::PrivateRingFull);

		// The key goes into the bucket of its owner's credit count, which is the tallest tier they
		// can prove against. Every tier at or below it holds that bucket, so one push enrols the
		// key in each of them.
		let tier = Self::private_key_tier(&info, credits);
		PrivateRingKeys::<T>::try_mutate(game_index, tier, |keys| {
			keys.try_push(key.clone()).map_err(|_| Error::<T>::PrivateRingFull)
		})?;
		PrivateRingKeyIndex::<T>::insert(game_index, &key, ());

		PrivateClaimants::<T>::insert(
			game_index,
			&claimant,
			PrivateClaimantState::Registered { credits },
		);

		info.key_count = info.key_count.saturating_add(1);
		info.note_tier_registration(credits);
		PrivateGames::<T>::insert(game_index, info);

		Self::deposit_event(Event::<T>::PrivateClaimKeyRegistered {
			game_index,
			claimant,
			credits,
		});

		Ok(())
	}

	/// The tier a registrant that earned `credits` goes into, which is the tallest ring they can
	/// prove against.
	///
	/// A registrant that earned more credits than the runtime carries tiers for joins the top tier
	/// rather than a bucket the build never reaches.
	fn private_key_tier(info: &PrivateGameInfoOf<T>, credits: u32) -> PrivateClaimTier {
		let top = info.tiers.len() as PrivateClaimTier;

		PrivateClaimTier::try_from(credits).unwrap_or(top).clamp(1, top)
	}

	/// How many keys the build step due for `game_index` pushes, or [`None`] when the game owes
	/// no build work.
	///
	/// Zero closes what is open. It opens the ladder for a game that has not started one, closes
	/// the current tier and drops to the next, or abandons a game whose tier 1 is too small or
	/// whose retries are spent.
	///
	/// Registration must be closed first. A ring built while keys still arrive changes under the
	/// claimants that already proved against it.
	pub(crate) fn private_ring_build_step(game_index: GameIdx) -> Option<u32> {
		let info = PrivateGames::<T>::get(game_index)?;
		if !Self::private_key_registration_closed(&info) {
			return None;
		}

		match info.phase {
			// The ladder is not open yet, so the step that decides its height is what is due.
			PrivateGamePhase::Registering => Some(0),
			PrivateGamePhase::Building { tier, included, failures } => {
				// The same zero-key step abandons a game whose retries are spent and closes a
				// tier whose own keys are all pushed.
				if failures >= PRIVATE_RING_BUILD_RETRIES {
					return Some(0);
				}
				let outstanding = info
					.tiers
					.get(usize::from(tier).saturating_sub(1))
					.map_or(0, |counts| counts.registered.saturating_sub(included));

				Some(outstanding.min(T::PrivateKeysPerBuild::get()))
			},
			PrivateGamePhase::Delivering | PrivateGamePhase::CleaningUp => None,
		}
	}

	/// The validity of one private claim job of `game_index`, which only this pallet's offchain
	/// worker submits.
	///
	/// The tag is the job and the game, so every attempt at one of them shares it and the pool
	/// keeps one. The block number breaks the tie inside `tier`, so a fresh attempt outranks the
	/// one holding the tag: the pool replaces it only for a strictly higher priority.
	fn private_job_validity(
		tag: &'static str,
		tier: TransactionPriority,
		game_index: &GameIdx,
	) -> ValidTransaction {
		ValidTransaction::with_tag_prefix(tag)
			.and_provides(game_index)
			.priority(indiv_support::tx_priority::add_tie_break(
				tier,
				frame_system::Pallet::<T>::block_number().saturated_into::<u64>(),
			))
			.longevity(TX_LONGEVITY)
			// Propagation is off because peers validate a gossiped transaction with a source of
			// `External`, which these calls reject.
			.propagate(false)
			.build()
			.expect("tag prefix is not empty; qed")
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

		// A build step gates every private claim of the game, so it ranks as deferrable
		// progress rather than cleanup.
		Ok((
			Self::private_job_validity(
				"nft-credits:build-private-ring",
				indiv_support::tx_priority::BACKGROUND_PROGRESS,
				game_index,
			),
			Weight::zero(),
		))
	}

	/// The body of [`Pallet::build_private_ring`].
	pub(crate) fn do_build_private_ring(
		game_index: GameIdx,
		to_include: u32,
	) -> Result<Weight, DispatchError> {
		let mut info = PrivateGames::<T>::get(game_index).ok_or(Error::<T>::NotAPrivateGame)?;

		let (tier, included, failures) = match info.phase {
			// The ladder's height is decided once, on the step that opens it, because a tier's
			// ring is only counted after registration closes.
			PrivateGamePhase::Registering => return Ok(Self::open_private_ladder(game_index, info)),
			PrivateGamePhase::Building { tier, included, failures } => (tier, included, failures),
			_ => return Err(Error::<T>::NoPrivateRingToBuild.into()),
		};

		// A game whose retries are spent is built no further and falls back to the public path.
		// The tiers already closed commit to nothing anyone can prove against, the outcome being
		// delivered only once the ladder is whole.
		if failures >= PRIVATE_RING_BUILD_RETRIES {
			return Ok(Self::abandon_private_game(game_index, info));
		}

		if to_include.is_zero() {
			return Self::close_private_tier(game_index, info, tier, included);
		}

		let Ok(capacity) = T::PrivateRingExponent::get().try_into() else {
			// The runtime's exponent is one the crypto does not take, so no retry builds this
			// ladder or any other.
			Self::note_private_build_failure(game_index, info, "the ring exponent is invalid");
			return Ok(<T as Config>::WeightInfo::build_private_ring(to_include));
		};
		let mut intermediate = PrivateRingIntermediates::<T>::get(game_index)
			.unwrap_or_else(|| T::RingVrf::start_members(capacity));

		// Only the tier's own bucket is read. Every key below it is already in the intermediate,
		// pushed while a higher tier was open.
		let pushed = PrivateRingKeys::<T>::get(game_index, tier)
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
			return Ok(<T as Config>::WeightInfo::build_private_ring(to_include));
		}

		PrivateRingIntermediates::<T>::insert(game_index, intermediate);
		info.phase = PrivateGamePhase::Building {
			tier,
			included: included.saturating_add(pushed_count),
			failures: 0,
		};
		PrivateGames::<T>::insert(game_index, info);

		Ok(<T as Config>::WeightInfo::build_private_ring(to_include))
	}

	/// Decide how tall `game_index`'s ladder is and open its top tier.
	///
	/// The height is the tallest tier whose ring still hides its claimants. A game whose tier 1
	/// falls short has no ring anyone can hide in, so it is abandoned and its credits go back to
	/// the public path.
	fn open_private_ladder(game_index: GameIdx, mut info: PrivateGameInfoOf<T>) -> Weight {
		let height = Self::private_ladder_height(&info);
		if height == 0 {
			return Self::abandon_private_game(game_index, info);
		}

		info.phase = PrivateGamePhase::Building { tier: height, included: 0, failures: 0 };
		PrivateGames::<T>::insert(game_index, info);
		Self::deposit_event(Event::<T>::PrivateLadderOpened { game_index, height });

		<T as Config>::WeightInfo::open_private_ring_ladder()
	}

	/// Snapshot tier `tier` of `game_index`'s ladder and drop to the tier below it.
	///
	/// At this point the intermediate holds every key of tier `tier` and nothing else, so
	/// finishing a clone of it yields that tier's root while the original carries on into the next
	/// bucket. Closing tier 1 finishes the ladder.
	fn close_private_tier(
		game_index: GameIdx,
		mut info: PrivateGameInfoOf<T>,
		tier: PrivateClaimTier,
		included: u32,
	) -> Result<Weight, DispatchError> {
		let owed = info
			.tiers
			.get(usize::from(tier).saturating_sub(1))
			.map_or(0, |counts| counts.registered);
		ensure!(included >= owed, Error::<T>::NoPrivateRingToBuild);

		let intermediate = PrivateRingIntermediates::<T>::get(game_index)
			.ok_or(Error::<T>::NoPrivateRingToBuild)?;
		let root = T::RingVrf::finish_members(intermediate);

		// Each root is prepended, because the ladder is built from the top down and the delivered
		// vector is indexed by tier.
		let key_count = info.key_count;
		let closed = PrivateOutcomes::<T>::try_mutate(game_index, |outcome| {
			let roots = match outcome {
				Some(PrivateGameOutcome::Ring { roots, .. }) => roots,
				_ => {
					*outcome =
						Some(PrivateGameOutcome::Ring { roots: BoundedVec::default(), key_count });
					match outcome {
						Some(PrivateGameOutcome::Ring { roots, .. }) => roots,
						_ => return Err(Error::<T>::NoPrivateRingToBuild),
					}
				},
			};
			roots.try_insert(0, root).map_err(|_| Error::<T>::NoPrivateRingToBuild)?;

			Ok::<u32, Error<T>>(roots.len() as u32)
		})?;

		// The root commits to the bucket and registration is closed, so nothing reads the bucket
		// again. It goes here rather than in the cleanup, which would carry a whole bucket into
		// the proof of one of its steps.
		PrivateRingKeys::<T>::remove(game_index, tier);
		Self::deposit_event(Event::<T>::PrivateRingBuilt { game_index, tier });

		if tier > 1 {
			info.phase = PrivateGamePhase::Building {
				tier: tier.saturating_sub(1),
				included: 0,
				failures: 0,
			};
			PrivateGames::<T>::insert(game_index, info);
			return Ok(<T as Config>::WeightInfo::finish_private_ring(closed));
		}

		PrivateRingIntermediates::<T>::remove(game_index);
		info.phase = PrivateGamePhase::Delivering;
		PrivateGames::<T>::insert(game_index, info);
		Self::deposit_event(Event::<T>::PrivateLadderBuilt {
			game_index,
			height: closed as PrivateClaimTier,
			key_count,
		});

		Ok(<T as Config>::WeightInfo::finish_private_ring(closed))
	}

	/// Count one failed build step of `game_index`.
	///
	/// The step that follows [`PRIVATE_RING_BUILD_RETRIES`] failures is the closing one, which
	/// abandons the game. Counting failures here and giving up there keeps every abandonment on
	/// the branch its weight is measured on.
	///
	/// `info` is the game's record as the failed step read it, so the caller must not have
	/// written it back.
	fn note_private_build_failure(
		game_index: GameIdx,
		mut info: PrivateGameInfoOf<T>,
		reason: &str,
	) {
		let PrivateGamePhase::Building { tier, included, failures } = info.phase else {
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

		info.phase = PrivateGamePhase::Building { tier, included, failures };
		PrivateGames::<T>::insert(game_index, info);
		Self::deposit_event(Event::<T>::PrivateRingBuildFailed { game_index, failures });
	}

	/// Give up on `game_index`'s ladder and record the abandonment for the claims chain, which
	/// reopens the public claim path for the game's credit trees.
	///
	/// `info` is the game's record with its phase not yet advanced. The weight returned covers the
	/// key buckets the step dropped.
	fn abandon_private_game(game_index: GameIdx, mut info: PrivateGameInfoOf<T>) -> Weight {
		let key_count = info.key_count;
		let required = Self::private_ring_floor(&info, 1);

		// A ladder half built commits to nothing anyone can prove against: its tiers are
		// delivered together or not at all. The keys go with it, so the game mints through its
		// credit trees from here on.
		PrivateRingIntermediates::<T>::remove(game_index);
		let buckets = Self::drop_private_ring_keys(game_index, &info);
		PrivateOutcomes::<T>::insert(game_index, PrivateGameOutcome::Abandoned { key_count });
		info.phase = PrivateGamePhase::Delivering;
		PrivateGames::<T>::insert(game_index, info);
		Self::deposit_event(Event::<T>::PrivateRingAbandoned { game_index, key_count, required });

		<T as Config>::WeightInfo::abandon_private_ring(buckets)
	}

	/// Drop every key bucket of `game_index` and report how many it cleared for.
	///
	/// A bucket exists only for a tier a registrant landed in, which the game's record counts, so
	/// the prefix is cleared at that bound and the cursor it returns is always spent. A tier
	/// already closed gave its bucket up, so the bound is an upper one.
	fn drop_private_ring_keys(game_index: GameIdx, info: &PrivateGameInfoOf<T>) -> u32 {
		let buckets =
			info.tiers.iter().filter(|counts| !counts.registered.is_zero()).count() as u32;
		if PrivateRingKeys::<T>::clear_prefix(game_index, buckets, None)
			.maybe_cursor
			.is_some()
		{
			log::error!(
				target: LOG_TARGET,
				"Game {game_index} holds more key buckets than its {buckets} registered tiers",
			);
		}

		buckets
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

		// The cleanup only frees storage, so it yields to every other transaction. The ring is
		// delivered by then and no claim reads what it drops.
		Ok((
			Self::private_job_validity(
				"nft-credits:clean-up-private-game",
				indiv_support::tx_priority::CLEANUP,
				game_index,
			),
			Weight::zero(),
		))
	}

	/// Whether `game_index` has registration state left to drop.
	///
	/// A game whose outcome is not delivered owes none, which the phase says. The last step
	/// removes the game's record and [`Pallet::do_send_private_ring`] reads the phase from it, so
	/// cleaning up first would leave an outcome that can never be sent.
	pub(crate) fn private_clean_up_due(game_index: GameIdx) -> bool {
		PrivateGames::<T>::get(game_index)
			.is_some_and(|info| matches!(info.phase, PrivateGamePhase::CleaningUp))
	}

	/// The body of [`Pallet::clean_up_private_game`], returning the claimants it removed.
	///
	/// One call removes at most [`PRIVATE_CLEAN_UP_ITEMS`] claimants and as many key-index
	/// entries, so a game is dropped over as many calls as it takes. The count returned is the
	/// claimants, which the weight is measured in. The game's record goes last, because it is what
	/// says the cleanup is still owed.
	///
	/// This drops the private path's own bookkeeping and nothing a claim reads: the key buckets
	/// went with the build steps that closed the tiers, and the credit trees an abandoned game
	/// mints through are untouched.
	pub(crate) fn do_clean_up_private_game(game_index: GameIdx) -> Result<u32, DispatchError> {
		ensure!(Self::private_clean_up_due(game_index), Error::<T>::NoPrivateGameToCleanUp);

		// The claimants are read before they are removed, rather than cleared by prefix, so that
		// the count the refund is measured in is exact.
		let claimants = PrivateClaimants::<T>::iter_key_prefix(game_index)
			.take(PRIVATE_CLEAN_UP_ITEMS as usize)
			.collect::<Vec<_>>();
		let claimants_removed = claimants.len() as u32;
		for claimant in &claimants {
			PrivateClaimants::<T>::remove(game_index, claimant);
		}

		// The key index holds one entry per registrant, which is a subset of the claimants. It is
		// cleared by prefix because nothing reads the keys back. Both maps give up the same number
		// of entries per call, so this step removes no more keys than claimants.
		let keys = PrivateRingKeyIndex::<T>::clear_prefix(game_index, PRIVATE_CLEAN_UP_ITEMS, None);

		if claimants_removed < PRIVATE_CLEAN_UP_ITEMS && keys.maybe_cursor.is_none() {
			PrivateGames::<T>::remove(game_index);
			Self::deposit_event(Event::<T>::PrivateGameCleanedUp { game_index });
		}

		// The claimants alone, because the benchmark prices one claimant together with the key
		// entry that can accompany it. Adding `keys.unique` would measure the refund in entries
		// where the weight is measured in claimants, and it counts only what the backend held, so
		// a key written in the same block would cost nothing.
		Ok(claimants_removed)
	}

	/// Validate a [`Pallet::send_private_ring`] submission.
	pub(crate) fn authorize_send_private_ring(
		source: TransactionSource,
		game_index: &GameIdx,
	) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
		if !matches!(source, TransactionSource::InBlock | TransactionSource::Local) {
			return Err(AuthorizeInvalidity::TransactionNotLocal.into());
		}

		// The game's phase is what says the delivery is owed. It is read rather than the outcome
		// itself, which carries a ring root into the proof.
		let owed = PrivateGames::<T>::get(game_index)
			.is_some_and(|info| matches!(info.phase, PrivateGamePhase::Delivering));
		if !owed {
			return Err(AuthorizeInvalidity::NoPrivateRingToSend.into());
		}

		// The claims chain takes no claim of the game until the ring lands, so the delivery
		// ranks as deferrable progress.
		Ok((
			Self::private_job_validity(
				"nft-credits:send-private-ring",
				indiv_support::tx_priority::BACKGROUND_PROGRESS,
				game_index,
			),
			Weight::zero(),
		))
	}

	/// The per-message room the claims channel needs for one private claim ring, which the router
	/// compares against its `max_message_size`.
	///
	/// A ring root is far larger than a Merkle root, so it is measured on its own and not against
	/// the credit trees' capacity. The size is the delivery's `MaxEncodedLen`, which reserves a
	/// full-height ladder whatever height the game reached.
	pub fn private_ring_channel_size() -> u32 {
		let empty = PrivateRingBatchOf::<T> { source_time: 0, rings: BoundedVec::default() };
		let empty_call =
			(u8::MAX, NftClaimsCall::<T>::ReceivePrivateRings { batch: empty }).encode();

		// The call once the batch holds its one delivery. The empty vector's length prefix gives
		// way to the prefix for a single ring.
		let call_len = empty_call.len() - Compact(0u32).encoded_size() +
			Compact(1u32).encoded_size() +
			PrivateRingDeliveryOf::<T>::max_encoded_len();
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
	/// Checked before every delivery. The router drops a message that does not fit, and the game's
	/// credits mint on neither path until the claims chain has the outcome.
	fn private_ring_fits_channel() -> bool {
		T::ChannelInfo::get_channel_info(T::NftClaimsParaId::get())
			.is_some_and(|info| Self::private_ring_channel_size() <= info.max_message_size)
	}

	/// The body of [`Pallet::send_private_ring`].
	///
	/// A message that cannot be sent leaves the outcome in place and reports
	/// `PrivateRingSendFailed`, so the next offchain-worker cycle retries the same ring. The call
	/// succeeds either way, because a failing dispatch would revert that event.
	pub(crate) fn do_send_private_ring(game_index: GameIdx) -> DispatchResult {
		let mut info = PrivateGames::<T>::get(game_index).ok_or(Error::<T>::NotAPrivateGame)?;
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

		let delivery = PrivateRingDeliveryOf::<T> { game_index, outcome };

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

		PrivateOutcomes::<T>::remove(game_index);
		info.phase = PrivateGamePhase::CleaningUp;
		PrivateGames::<T>::insert(game_index, info);
		Self::deposit_event(Event::<T>::PrivateRingSent { game_index });

		Ok(())
	}

	/// Submits the private claim work of the moment: one build step, one delivery, or one cleanup
	/// step.
	///
	/// Building comes first, because an unfinished ladder has nothing to deliver. Delivery comes
	/// before cleanup, because claims wait on the ladder and on nothing the cleanup drops. One
	/// ladder fills a message, so one outcome is delivered per block.
	pub(crate) fn submit_private_ring_work(block_number: BlockNumberFor<T>) {
		let mut cleanup = None;
		let mut delivery = None;
		for (game_index, info) in PrivateGames::<T>::iter() {
			if let Some(to_include) = Self::private_ring_build_step(game_index) {
				Self::submit_private_call(
					Call::<T>::build_private_ring {
						game_index,
						to_include,
						// The submitting block. A ladder takes as many steps as its keys need
						// and every step but a tier's last pushes `PrivateKeysPerBuild` keys, so
						// the call encodes the same way each time. A window would leave the pool
						// banning the hash of the step it included and the next attempt
						// repeating it.
						discriminator: block_number,
					},
					"build_private_ring",
				);
				return;
			}
			if delivery.is_none() && matches!(info.phase, PrivateGamePhase::Delivering) {
				delivery = Some(game_index);
			}
			if cleanup.is_none() && matches!(info.phase, PrivateGamePhase::CleaningUp) {
				cleanup = Some(game_index);
			}
		}

		if let Some(game_index) = delivery {
			Self::submit_private_call(
				Call::<T>::send_private_ring {
					game_index,
					// The game picked changes once its outcome is sent, so the window paces
					// retries of one delivery alone.
					discriminator: block_number / RETRY_WINDOW.into(),
				},
				"send_private_ring",
			);
			return;
		}

		if let Some(game_index) = cleanup {
			Self::submit_private_call(
				Call::<T>::clean_up_private_game {
					game_index,
					// The submitting block, for the same reason as a build step: a cleanup takes
					// several steps and `game_index` alone cannot tell them apart.
					discriminator: block_number,
				},
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

	/// Assert that the private claim path's calls fit the offchain-worker block budget.
	///
	/// The ladder's height needs no check. It is the tiers that cleared the game's own anonymity
	/// floor, capped at [`Config::MaxPrivateRingTiers`], so no runtime configures it. A cap that
	/// binds costs a claimant their top credits and nothing else.
	///
	/// Whether a delivery fits the claims channel is not asserted here. Only chain state knows the
	/// channel's `max_message_size`, so [`Pallet::do_send_private_ring`] checks it before every
	/// delivery and retries a message that does not fit.
	#[cfg(feature = "std")]
	pub(crate) fn private_integrity_test(budget: &indiv_support::weight_budget::OcwWeightBudget) {
		use crate::WeightInfo as _;

		// A ring that cannot hold every key it accepts fails to build after registration closes,
		// so the game is abandoned and every claimant falls back to a public claim.
		let ring_capacity = T::PrivateRingExponent::get().ring_capacity();
		assert!(
			T::MaxPrivateRingKeys::get() <= ring_capacity,
			"`MaxPrivateRingKeys` ({keys}) exceeds the ring capacity ({ring_capacity})",
			keys = T::MaxPrivateRingKeys::get(),
		);

		// A ring of one names its claimant, and a floor above the ring capacity builds no ring at
		// all.
		assert!(
			T::MinPrivateRingKeys::get() >= 2 &&
				T::MinPrivateRingKeys::get() <= T::MaxPrivateRingKeys::get(),
			"`MinPrivateRingKeys` ({min}) must be between two and `MaxPrivateRingKeys` ({max})",
			min = T::MinPrivateRingKeys::get(),
			max = T::MaxPrivateRingKeys::get(),
		);

		// A build step that pushes nothing never finishes a ring.
		assert!(
			!T::PrivateKeysPerBuild::get().is_zero(),
			"`PrivateKeysPerBuild` must be at least one",
		);

		// Registration has to outlast the block a game ends in, or no claimant can register.
		assert!(
			!T::PrivateKeyRegistrationSeconds::get().is_zero(),
			"`PrivateKeyRegistrationSeconds` must be at least one",
		);

		// A ladder of no tiers pays nobody.
		assert!(
			!T::MaxPrivateRingTiers::get().is_zero(),
			"`MaxPrivateRingTiers` must be at least one",
		);

		budget.assert_fits(
			"build_private_ring",
			<T as Config>::WeightInfo::build_private_ring(T::PrivateKeysPerBuild::get())
				.saturating_add(<T as Config>::WeightInfo::authorize_build_private_ring()),
		);
		// A zero-key step takes one of three branches. The call charges the dearest of them and
		// refunds down, so the dearest is what has to fit.
		budget.assert_fits(
			"build_private_ring (close)",
			<T as Config>::WeightInfo::finish_private_ring(T::MaxPrivateRingTiers::get())
				.max(<T as Config>::WeightInfo::open_private_ring_ladder())
				.max(<T as Config>::WeightInfo::abandon_private_ring(T::MaxPrivateRingTiers::get()))
				.saturating_add(<T as Config>::WeightInfo::authorize_build_private_ring()),
		);
		budget.assert_fits(
			"send_private_ring",
			<T as Config>::WeightInfo::send_private_ring()
				.saturating_add(<T as Config>::WeightInfo::authorize_send_private_ring()),
		);
		budget.assert_fits(
			"clean_up_private_game",
			<T as Config>::WeightInfo::clean_up_private_game(PRIVATE_CLEAN_UP_ITEMS)
				.saturating_add(<T as Config>::WeightInfo::authorize_clean_up_private_game()),
		);
	}
}
