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

//! Resource allocation and consumer lifecycle management.
//!
//! This pallet registers people as consumers and allocates their statement-store resources.
//! Anonymous members can claim temporary notification allowances by proving membership in a
//! context that contains a period and a slot. A period is a fixed-duration window set by
//! `NotificationPeriodDuration`, and a slot is an allowance identifier valid only within that
//! period. A notification allowance authorizes a statement account to publish a notification
//! without revealing the member's identity.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
pub mod extension;
pub mod migration;
pub mod types;
pub mod weights;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use pallet::*;
pub use weights::WeightInfo;

use frame_support::{
	dispatch::DispatchResultWithPostInfo,
	traits::{EnsureOriginWithArg, IsSubType, OriginTrait, UnixTime},
};
use frame_system::offchain::{CreateAuthorizedTransaction, SubmitTransaction};
use indiv_support::{
	context::{build_product_context, personhood, ProductContextSuffix},
	traits::{
		Alias, AllocateStorage, AppendOnlyMembers, CommunicationIdentifier, ConsumerRegistrar,
		Context, MembershipProver, RingExponent,
	},
	tx_priority,
	utils::BigEndianU32,
	weight_budget::OcwWeightBudget,
};
use sp_runtime::traits::{IdentifyAccount, Verify};
use types::{
	ConsumerInfo, Credibility, LongTermStorageAllocation, MembershipCollection,
	NotificationReference, StmtStoreAllowanceEntry,
};
use verifiable::GenerateVerifiable;

#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	use sp_runtime::traits::Zero;
	use sp_statement_store::{decrease_allowance_by, increase_allowance_by, StatementAllowance};

	const LOG_TARGET: &str = "runtime::indiv-pallet-resources";
	pub(crate) const SECONDS_PER_DAY: u64 = 86_400;

	const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config:
		frame_system::Config<
			RuntimeOrigin: From<Origin>
			                   + From<<Self::RuntimeOrigin as OriginTrait>::PalletsOrigin>
			                   + OriginTrait<
				PalletsOrigin: From<Origin>
				                   + TryInto<
					Origin,
					Error = <Self::RuntimeOrigin as OriginTrait>::PalletsOrigin,
				>,
			>,
			RuntimeCall: IsSubType<Call<Self>>,
			AccountId: From<sp_statement_store::AccountId> + Into<sp_statement_store::AccountId>,
		> + CreateAuthorizedTransaction<Call<Self>>
		+ Send
		+ Sync
	{
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Runtime-wide network suffix used to derive product contexts.
		type Suffix: Get<indiv_support::context::ProductContextNetworkSuffix>;

		/// Trait allowing cryptographic proof of membership without exposing the underlying member.
		/// Normally a Ring-VRF.
		///
		/// This must be the member service for both people and lite people collections.
		type MemberService: AppendOnlyMembers
			+ MembershipProver<
				Crypto: GenerateVerifiable<
					Proof: Send + Sync + DecodeWithMemTracking,
					Signature: Send + Sync + DecodeWithMemTracking,
					Member: DecodeWithMemTracking,
					Config: TryFrom<RingExponent>,
				>,
			>;

		/// The duration of time, in seconds, for which a person's authorization is valid. After
		/// this period elapses, people will no longer be considered active, but their resource
		/// allowances should default to the same values used for lite people.
		#[pallet::constant]
		type PersonAuthDuration: Get<u32>;

		/// The minimum interval of time, in seconds, which must pass before updating a person's
		/// authorization.
		#[pallet::constant]
		type MinPersonAuthUpdateInterval: Get<u32>;

		/// The Statement Store allowance for the accounts API.
		///
		/// Changing this value requires a migration that updates the existing state.
		#[pallet::constant]
		type AccountsApiAllowance: Get<StatementAllowance>;

		/// Maximum number of statement store slots a person can claim within one period.
		///
		/// This parameter can be changed at any time.
		type StmtStoreSlotsPerPeriod: Get<u32>;

		/// Maximum number of statement store slots a lite person can claim within one period.
		///
		/// Same semantics as `StmtStoreSlotsPerPeriod` but applied when the proof targets the
		/// lite-people collection via `MembershipCollection::LitePeople`.
		///
		/// This parameter can be changed at any time.
		type LiteStmtStoreSlotsPerPeriod: Get<u32>;

		/// Maximum number of stale statement store allowance entries to remove per cleanup call.
		///
		/// This parameter can be changed at any time.
		type StmtStoreCleanupLimit: Get<u32>;

		/// Minimum time, in seconds, that must pass before an alias can replace its own
		/// statement store allowance entry within the same period.
		///
		/// This parameter can be changed at any time.
		type StmtStoreReplacementCooldown: Get<u32>;

		/// Extra time, in seconds, during which statement-store allowances from an ended period
		/// remain active before cleanup may revoke them.
		///
		/// After this elapses, the allowances will eventually be cleaned by the OCW.
		///
		/// This parameter can be changed at any time.
		type StmtStoreGraceWindow: Get<u32>;

		/// The Statement Store allowance for notification statement registration.
		///
		/// Changing this value requires a migration that updates the existing state.
		#[pallet::constant]
		type NotificationAllowance: Get<StatementAllowance>;

		/// Highest valid notification slot identifier for a person within one period.
		///
		/// A period is a fixed-duration window set by `NotificationPeriodDuration`. Each slot can
		/// be claimed at most once by a person within that period.
		///
		/// For example, if this is `8`, the valid slot identifiers are `0..=8`, so each person
		/// can claim up to 9 notification allowances during the period selected by
		/// `NotificationPeriodDuration`. When the period advances, the slots reset.
		///
		/// This parameter can be changed at any time.
		type NotificationSlotsPerPeriod: Get<u8>;

		/// Highest valid notification slot identifier for a lite person within one period.
		///
		/// Same semantics as `NotificationSlotsPerPeriod` but applied when the proof targets the
		/// lite-people collection via `MembershipCollection::LitePeople`.
		///
		/// This parameter can be changed at any time.
		type LiteNotificationSlotsPerPeriod: Get<u8>;

		/// Rolling time window for rate-limiting notifications, in seconds.
		///
		/// Time is divided into fixed-duration periods. The period index is computed as
		/// `now_secs / NotificationPeriodDuration`.
		///
		/// For example, if this is `86_400` (24 hours), period `0` is the first 24 hours since the
		/// Unix epoch, period `1` is the next 24 hours, and so on. Combined with
		/// `NotificationSlotsPerPeriod`, this defines how many notifications can be sent in each
		/// period.
		///
		/// Changing this value requires a migration that updates the existing state.
		#[pallet::constant]
		type NotificationPeriodDuration: Get<u32>;

		/// Number of blocks between offchain-worker maintenance runs.
		#[pallet::constant]
		type OffchainWorkerInterval: Get<BlockNumberFor<Self>>;

		/// How to recognise an origin representing a person.
		type EnsurePerson: EnsureOriginWithArg<OriginFor<Self>, Context, Success = Alias>;

		/// How to recognise an origin representing a lite person.
		type EnsureLitePerson: EnsureOrigin<OriginFor<Self>, Success = Self::AccountId>;

		/// The origin allowed to perform privileged management operations on this pallet.
		type ManagerOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// The source of time.
		type Clock: UnixTime;

		/// Signature type for ensuring ownership of provided accounts in case of registrations
		/// through alias.
		type OffchainSignature: Verify<Signer: IdentifyAccount<AccountId = Self::AccountId>>
			+ Parameter;

		/// The limit for the statement store usage for lite people.
		type LitePersonStatementLimit: Get<StatementAllowance>;

		/// The limit for the statement store usage for people. Must be equal to or greater than the
		/// lite person limit.
		type PersonStatementLimit: Get<StatementAllowance>;

		/// The duration of a long-term storage claiming period, in seconds.
		///
		/// Time is divided into fixed-duration periods. The period index is computed as
		/// `now_secs / LongTermStoragePeriodDuration`. Each person can submit up to
		/// `LongTermStorageClaimsPerPeriod` claims per period.
		///
		/// Changing this value requires a migration that updates the existing state.
		#[pallet::constant]
		type LongTermStoragePeriodDuration: Get<u32>;

		/// Maximum number of long-term storage claims per person per period.
		///
		/// Each claim uses a different counter value (0..claims_per_period) which produces a
		/// distinct alias in the proof context, ensuring one claim per counter slot.
		///
		/// This parameter can be changed at any time.
		type LongTermStorageClaimsPerPeriod: Get<u8>;

		/// Extra time, in seconds, during which the previous long-term storage period is still
		/// accepted after a rollover.
		///
		/// Extra time, in seconds, during which the previous long-term storage period is accepted
		/// after a rollover.
		///
		/// Changing this value requires a migration that updates the existing state.
		#[pallet::constant]
		type LongTermStorageGraceWindow: Get<u32>;

		/// The long-term storage allocation granted per claim for people.
		type LongTermStorageAllowanceForPeople: Get<LongTermStorageAllocation>;

		/// The long-term storage allocation granted per claim for lite people.
		type LongTermStorageAllowanceForLitePeople: Get<LongTermStorageAllocation>;

		/// The data store used to allocate long-term storage on a remote chain.
		type LongTermStorageDataStore: AllocateStorage<Self::AccountId>;

		/// Maximum number of spent long-term storage aliases that can be cleared in a single
		/// `clear_expired_long_term_storage_aliases` call.
		///
		/// Bounds the worst-case weight of the cleanup extrinsic; callers must pass a `limit`
		/// no greater than this value.
		///
		/// This parameter can be changed at any time.
		type LongTermStorageCleanupLimit: Get<u32>;

		/// Benchmark helper trait.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: benchmarking::BenchmarkHelper<Self>;
	}

	#[pallet::origin]
	#[derive(
		Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
	)]
	pub enum Origin {
		/// A notification alias origin, produced by the `AsResources` transaction extension.
		NotificationAlias(Alias),
		/// A statement store slot alias origin, produced by the `AsResources` transaction
		/// extension after validating a ring-VRF proof for a specific slot context.
		StmtStoreAlias(Alias),
		/// A long-term storage claim origin, produced by the `AsResources` transaction extension.
		/// Carries the anonymous alias and the collection used for proof verification.
		LongTermStorageClaim(Alias, MembershipCollection),
	}

	/// Accounts used to identify consumers mapped to their consumer information.
	#[pallet::storage]
	pub type Consumers<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, ConsumerInfo>;

	/// Accounts associated with a statement store slot through an anonymous allowance, per period.
	///
	/// The period key is a big-endian encoded day number (seconds since Unix epoch / 86400) so
	/// that `Identity`-hashed iteration yields entries in chronological order to be removed by the
	/// offchain worker.
	#[pallet::storage]
	pub type StatementStoreAllowances<T: Config> = StorageDoubleMap<
		_,
		Identity,
		BigEndianU32,
		Blake2_128Concat,
		Alias,
		StmtStoreAllowanceEntry<T>,
		OptionQuery,
	>;

	/// Reverse lookup from a statement account to all its active anonymous allowances.
	///
	/// Keyed by `(AccountId, (BigEndianU32 period, u32 seq, Alias))` → `()`. Multiple
	/// entries per account are possible when the same statement account is authorized by
	/// different aliases or across grace-window overlaps.
	#[pallet::storage]
	pub type StmtStoreAllowanceByAccount<T: Config> = StorageDoubleMap<
		_,
		Blake2_128Concat,
		T::AccountId,
		Blake2_128Concat,
		(BigEndianU32, u32, Alias),
		(),
		OptionQuery,
	>;

	/// Notification allowance registration by anonymous alias.
	#[pallet::storage]
	pub type NotificationRegistrationByAlias<T: Config> = StorageMap<
		_,
		Blake2_128Concat,
		Alias,
		types::NotificationRegistration<T::AccountId>,
		OptionQuery,
	>;

	/// Reverse lookup from notification statement account to anonymous alias.
	#[pallet::storage]
	pub type NotificationAliasByAccount<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, Alias, OptionQuery>;

	/// Aliases that have already been used to claim long-term storage in a given period.
	///
	/// Keyed by `(period, alias)`. Each counter value in the proof context produces a unique
	/// alias, so a person can have up to `LongTermStorageClaimsPerPeriod` entries per period.
	/// Old periods can be cleaned up via `clear_expired_long_term_storage_aliases`.
	///
	/// The period key is `BigEndianU32` with `Identity` so iteration yields entries in
	/// chronological order, matching `StatementStoreAllowances`.
	#[pallet::storage]
	pub type SpentLongTermStorageAliases<T: Config> =
		StorageDoubleMap<_, Identity, BigEndianU32, Blake2_128Concat, Alias, (), OptionQuery>;

	/// Reverse lookup from registered aliases to the `AccountId` used to register as a consumer.
	#[pallet::storage]
	pub type AccountOfAlias<T: Config> =
		StorageMap<_, Blake2_128Concat, Alias, T::AccountId, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A person has registered as a consumer.
		PersonRegistered { alias: Alias, account: T::AccountId },
		/// A lite person has registered as a consumer.
		LitePersonRegistered { account: T::AccountId },
		/// Notification statement usage has been assigned for a sequence.
		NotificationStmtUsageSet { alias: Alias, period: u32, seq: u8, account: T::AccountId },
		/// Notification statement usage has been removed.
		NotificationStmtUsageRemoved { account: T::AccountId },
		/// A person's authorization was touched.
		PersonAuthorizationTouched { account: T::AccountId },
		/// A consumer's identifier key was updated.
		IdentifierKeyUpdated { account: T::AccountId },
		/// An anonymous statement store allowance was granted.
		StmtStoreAllowanceSet { alias: Alias, period: u32, seq: u32, account: T::AccountId },
		/// Expired statement store allowances were cleaned up.
		StmtStoreAllowancesCleared { period: u32, first_key: Alias, count: u32 },
		/// A full person was demoted due to expired authorization.
		PersonDemoted { account: T::AccountId },
		/// Long-term storage has been claimed for an account.
		LongTermStorageClaimed {
			alias: Alias,
			period: u32,
			counter: u8,
			account: T::AccountId,
			collection: MembershipCollection,
		},
		/// A long-term storage claim was accepted but the downstream allocation failed. The alias
		/// is still marked spent for the period.
		LongTermStorageAllocationFailed {
			alias: Alias,
			period: u32,
			counter: u8,
			account: T::AccountId,
			collection: MembershipCollection,
		},
		/// Expired long-term storage aliases have been cleared for a period.
		LongTermStorageAliasesCleared { period: u32, count: u32 },
	}

	#[pallet::error]
	pub enum Error<T> {
		/// Consumer is already registered.
		AlreadyRegistered,
		/// Provided proof of ownership is invalid.
		InvalidProofOfOwnership,
		/// Person is not registered as a consumer.
		NotRegistered,
		/// Consumer is not a full person.
		NotFullPerson,
		/// Attempted to update person authorization too early.
		TouchNotReady,
		/// There is no lite consumer to be linked.
		NoLinkedIdentity,
		/// The lite consumer is already linked to a full person consumer.
		AlreadyLinked,
		/// The person's authorization has not expired yet.
		PersonAuthNotExpired,
		/// The person has already been demoted.
		AlreadyDemoted,
		/// Notification sequence is invalid for the consumer.
		InvalidNotificationSequence,
		/// Notification period is outside the accepted claim window.
		InvalidNotificationPeriod,
		/// Notification registration is not expired yet.
		NotificationRegistrationNotExpired,
		/// Notification registration already exists for the alias/context.
		NotificationRegistrationAlreadyExists,
		/// The replacement cooldown has not elapsed since the entry was last set.
		StmtStoreReplacementTooEarly,
		/// The provided `limit` exceeds `LongTermStorageCleanupLimit`.
		LongTermStorageCleanupLimitExceeded,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: BlockNumberFor<T>) {
			if !(block_number % T::OffchainWorkerInterval::get()).is_zero() {
				return;
			}

			for registration in NotificationRegistrationByAlias::<T>::iter_values() {
				if !Self::should_clear_notification_registration(&registration) {
					continue;
				}

				let call = Call::clear_expired_notification_sequence {
					account: registration.account_id,
					seq: registration.reference.seq,
				};
				Self::submit_authorized_transaction(call, "Clear expired notification sequence");
			}

			// Clean up stale statement store allowances.
			// Check the first (oldest) period key in the map. Because the period key is
			// big-endian encoded under `Identity`, iteration yields periods in ascending order.
			if let Some((oldest_period_key, first_alias, _)) =
				StatementStoreAllowances::<T>::iter().next()
			{
				let oldest_period: u32 = oldest_period_key.into();
				if Self::is_stmt_store_period_clearable(oldest_period) {
					let call = Call::clear_expired_stmt_store_allowances {
						period: oldest_period,
						first_entry: first_alias,
					};
					Self::submit_authorized_transaction(
						call,
						"Clear expired statement store allowances",
					);
				}
			}

			// Clean up stale long-term storage aliases. Same trick: the period key is
			// `BigEndianU32` under `Identity`, so iteration yields the oldest period first;
			// if the oldest isn't clearable, none are.
			if let Some((oldest_period_key, _alias)) =
				SpentLongTermStorageAliases::<T>::iter_keys().next()
			{
				let oldest_period: u32 = oldest_period_key.into();
				if Self::is_long_term_storage_period_clearable(oldest_period) {
					let call = Call::clear_expired_long_term_storage_aliases {
						period: oldest_period,
						limit: T::LongTermStorageCleanupLimit::get(),
					};
					Self::submit_authorized_transaction(
						call,
						"Clear expired long-term storage aliases",
					);
				}
			}
		}

		fn integrity_test() {
			assert!(
				T::NotificationSlotsPerPeriod::get() > 0,
				"NotificationSlotsPerPeriod must be non-zero",
			);
			assert!(
				T::LiteNotificationSlotsPerPeriod::get() > 0,
				"LiteNotificationSlotsPerPeriod must be non-zero",
			);
			assert!(
				T::LiteNotificationSlotsPerPeriod::get() <= T::NotificationSlotsPerPeriod::get(),
				"LiteNotificationSlotsPerPeriod must be <= NotificationSlotsPerPeriod",
			);
			assert!(
				T::NotificationPeriodDuration::get() > 0,
				"NotificationPeriodDuration must be non-zero",
			);
			assert_eq!(
				T::NotificationPeriodDuration::get() as u64,
				SECONDS_PER_DAY,
				"NotificationPeriodDuration must be one day to match statement store periods",
			);
			assert!(
				T::OffchainWorkerInterval::get() > Zero::zero(),
				"OffchainWorkerInterval must be greater than 0",
			);
			assert!(
				T::StmtStoreSlotsPerPeriod::get() > 0,
				"StmtStoreSlotsPerPeriod must be non-zero",
			);
			assert!(
				T::LiteStmtStoreSlotsPerPeriod::get() > 0,
				"LiteStmtStoreSlotsPerPeriod must be non-zero",
			);
			assert!(
				T::LiteStmtStoreSlotsPerPeriod::get() <= T::StmtStoreSlotsPerPeriod::get(),
				"LiteStmtStoreSlotsPerPeriod must be <= StmtStoreSlotsPerPeriod",
			);
			assert!(T::StmtStoreCleanupLimit::get() > 0, "StmtStoreCleanupLimit must be non-zero",);
			assert!(
				T::StmtStoreReplacementCooldown::get() > 0,
				"StmtStoreReplacementCooldown must be non-zero",
			);
			assert!(
				(T::StmtStoreReplacementCooldown::get() as u64) <= SECONDS_PER_DAY,
				"StmtStoreReplacementCooldown must be at most one day (the period length)",
			);
			assert!(T::StmtStoreGraceWindow::get() > 0, "StmtStoreGraceWindow must be non-zero",);
			assert!(
				T::LongTermStoragePeriodDuration::get() > 0,
				"LongTermStoragePeriodDuration must be non-zero",
			);
			assert!(
				T::LongTermStorageGraceWindow::get() < T::LongTermStoragePeriodDuration::get(),
				"LongTermStorageGraceWindow must be smaller than LongTermStoragePeriodDuration",
			);
			assert!(
				T::LongTermStorageClaimsPerPeriod::get() > 0,
				"LongTermStorageClaimsPerPeriod must be non-zero",
			);
			assert!(
				T::LongTermStorageCleanupLimit::get() > 0,
				"LongTermStorageCleanupLimit must be non-zero",
			);

			// Every OCW-submitted authorized extrinsic must fit in
			// `Normal.max_extrinsic`, otherwise the transaction is silently dropped at
			// the transaction-pool level and the cleanup flow stalls forever.
			let budget = OcwWeightBudget::from_normal_max::<T>();

			budget.assert_fits(
				"clear_expired_notification_sequence",
				<T as Config>::WeightInfo::clear_expired_notification_sequence().saturating_add(
					<T as Config>::WeightInfo::authorize_clear_expired_notification_sequence(),
				),
			);
			budget.assert_fits(
				"clear_expired_stmt_store_allowances",
				<T as Config>::WeightInfo::clear_expired_stmt_store_allowances(
					T::StmtStoreCleanupLimit::get(),
				)
				.saturating_add(
					<T as Config>::WeightInfo::authorize_clear_expired_stmt_store_allowances(),
				),
			);
			budget.assert_fits(
				"clear_expired_long_term_storage_aliases",
				<T as Config>::WeightInfo::clear_expired_long_term_storage_aliases(
					T::LongTermStorageCleanupLimit::get(),
				)
				.saturating_add(
					<T as Config>::WeightInfo::authorize_clear_expired_long_term_storage_aliases(),
				),
			);
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Register a lite person as a consumer.
		#[pallet::call_index(0)]
		#[pallet::weight(<T as Config>::WeightInfo::register_lite_person())]
		pub fn register_lite_person(
			origin: OriginFor<T>,
			identifier_key: CommunicationIdentifier,
		) -> DispatchResultWithPostInfo {
			// Ensure this is a lite person.
			let lite_person_account = T::EnsureLitePerson::ensure_origin(origin)?;
			Self::register_lite_consumer_inner(lite_person_account, identifier_key)?;
			Ok(Pays::No.into())
		}

		/// Register a proven person as a consumer.
		///
		/// The person must link a previously recognized lite identity, which will be upgraded to a
		/// full person consumer. In order to prove they hold the lite identity they want to link,
		/// users must provide a `lite_identity_proof` signature, created by signing the alias bytes
		/// using their lite consumer account.
		#[pallet::call_index(1)]
		#[pallet::weight(<T as Config>::WeightInfo::register_person())]
		pub fn register_person(
			origin: OriginFor<T>,
			linked_lite_identity: T::AccountId,
			lite_identity_proof: T::OffchainSignature,
		) -> DispatchResultWithPostInfo {
			let alias = T::EnsurePerson::ensure_origin(origin, &Self::resources_context())?;
			ensure!(!AccountOfAlias::<T>::contains_key(alias), Error::<T>::AlreadyRegistered);

			// Verifying proof of ownership of the linked lite person.
			ensure!(
				lite_identity_proof.verify(&alias[..], &linked_lite_identity),
				Error::<T>::InvalidProofOfOwnership,
			);
			// Ensuring the linked lite person was not already linked to another full person.
			let mut linked_consumer_info =
				Consumers::<T>::get(&linked_lite_identity).ok_or(Error::<T>::NoLinkedIdentity)?;
			ensure!(
				matches!(linked_consumer_info.credibility, Credibility::Lite),
				Error::<T>::AlreadyLinked
			);

			// Updating the linked lite consumer's record with the full person credibility. From
			// this moment onward, this consumer will be registered as a full person through this
			// upgrade.
			let now = T::Clock::now().as_secs();
			linked_consumer_info.credibility =
				Credibility::Person { alias, last_update: now, demoted: false };

			// Marking the alias as used.
			AccountOfAlias::<T>::insert(alias, &linked_lite_identity);
			// Setting the consumer's record.
			Consumers::<T>::insert(&linked_lite_identity, linked_consumer_info);

			// Increasing the allowance by the difference between the lite person allowance the user
			// already has and the full person allowance they now have.
			let allowance =
				T::PersonStatementLimit::get().saturating_sub(T::LitePersonStatementLimit::get());
			increase_allowance_by(linked_lite_identity.clone().into(), allowance);

			Self::deposit_event(Event::PersonRegistered { alias, account: linked_lite_identity });
			Ok(Pays::No.into())
		}

		/// Update a person's authorization by ensuring they can still authenticate as people.
		///
		/// This call must be performed at least `MinPersonAuthUpdateInterval` seconds after the
		/// last update in order to prevent spam.
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::touch_person_authorization())]
		pub fn touch_person_authorization(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			// Ensure this is a person.
			let alias = T::EnsurePerson::ensure_origin(origin, &Self::resources_context())?;
			let account = AccountOfAlias::<T>::get(alias).ok_or(Error::<T>::NotRegistered)?;
			let consumer_info = Consumers::<T>::get(&account).ok_or(Error::<T>::NotRegistered)?;
			let Credibility::Person { last_update, demoted: was_demoted, .. } =
				consumer_info.credibility
			else {
				return Err(Error::<T>::NotFullPerson.into());
			};
			// Ensure the authorization is old enough to be touched.
			let now = T::Clock::now().as_secs();
			ensure!(
				now > last_update.saturating_add(T::MinPersonAuthUpdateInterval::get() as u64),
				Error::<T>::TouchNotReady
			);

			// Set the consumer's updated record.
			Consumers::<T>::insert(
				&account,
				ConsumerInfo {
					credibility: Credibility::Person { alias, last_update: now, demoted: false },
					..consumer_info
				},
			);

			if was_demoted {
				// A person's allowance should be given back.
				let person_allowance = T::PersonStatementLimit::get();
				let lite_person_allowance = T::LitePersonStatementLimit::get();
				let remaining_allowance = person_allowance.saturating_sub(lite_person_allowance);
				increase_allowance_by(account.clone().into(), remaining_allowance);
			}

			Self::deposit_event(Event::PersonAuthorizationTouched { account });
			Ok(Pays::No.into())
		}

		/// Update the communication identifier key of a consumer.
		///
		/// The origin must be the account registered for that consumer, regardless of their
		/// credibility.
		#[pallet::call_index(4)]
		#[pallet::weight(<T as Config>::WeightInfo::update_identifier_key())]
		pub fn update_identifier_key(
			origin: OriginFor<T>,
			identifier_key: CommunicationIdentifier,
		) -> DispatchResult {
			let who = ensure_signed(origin)?;
			let mut consumer_info = Consumers::<T>::get(&who).ok_or(Error::<T>::NotRegistered)?;
			consumer_info.identifier_key = identifier_key;
			Consumers::<T>::insert(&who, consumer_info);
			Self::deposit_event(Event::IdentifierKeyUpdated { account: who });
			Ok(())
		}

		/// Demote a full person to a lite person after their authorization has expired.
		///
		/// This is a permissionless call; the origin must be authorized.
		#[pallet::call_index(7)]
		#[pallet::authorize(|_source, account| {
		    Self::authorize_demote_auth_expired(account)
		})]
		#[pallet::weight_of_authorize(<T as Config>::WeightInfo::authorize_demote_auth_expired())]
		#[pallet::weight(<T as Config>::WeightInfo::demote_auth_expired())]
		pub fn demote_auth_expired(origin: OriginFor<T>, account: T::AccountId) -> DispatchResult {
			ensure_authorized(origin)?;
			let consumer_info = Self::validate_demotion(&account)?;
			if let Credibility::Person { alias, last_update, .. } = consumer_info.credibility {
				let allowance = T::PersonStatementLimit::get()
					.saturating_sub(T::LitePersonStatementLimit::get());
				decrease_allowance_by(account.clone().into(), allowance);
				Consumers::<T>::insert(
					&account,
					ConsumerInfo {
						credibility: Credibility::Person { alias, last_update, demoted: true },
						..consumer_info
					},
				);
			}
			Self::deposit_event(Event::PersonDemoted { account });
			Ok(())
		}

		/// Associate a statement account with a notification context sequence.
		///
		/// The associated account can submit statements while this notification registration is
		/// active.
		/// The origin must be `Origin::NotificationAlias`, created by the `AsResources`
		/// (`RegisterNotificationWithProof(..)`) transaction extension after proof validation.
		/// On success, increases statement allowance and stores registration state
		/// `{account_id, reference}`.
		///
		/// Parameters:
		/// * `reference`: notification period/sequence pair.
		///   - `reference.period` must be the current statement-store period.
		///   - `reference.seq` must be in `0..=NotificationSlotsPerPeriod`.
		/// * `account_id`: statement account to authorize. Must not already be used by another
		///   notification registration.
		#[pallet::call_index(8)]
		#[pallet::weight(<T as Config>::WeightInfo::set_notification_statement_account_for_sequence())]
		pub fn set_notification_statement_account_for_sequence(
			origin: OriginFor<T>,
			reference: NotificationReference,
			account_id: T::AccountId,
		) -> DispatchResultWithPostInfo {
			// Ensure this is a notification alias origin produced by `AsResources`.
			let alias = Self::ensure_notification_alias(origin)?;
			// Fail fast if parameters are invalid.
			Self::validate_notification_period(reference.period)?;
			// `AsResources` enforces the collection-specific slot bound before dispatch. By the
			// time this call executes, only the broader catch-all notification bound remains.
			Self::validate_notification_seq(reference.seq)?;

			Self::validate_notification_registration(alias, &account_id)?;

			increase_allowance_by(account_id.clone().into(), T::NotificationAllowance::get());
			NotificationRegistrationByAlias::<T>::insert(
				alias,
				types::NotificationRegistration { account_id: account_id.clone(), reference },
			);
			NotificationAliasByAccount::<T>::insert(&account_id, alias);

			Self::deposit_event(Event::NotificationStmtUsageSet {
				alias,
				period: reference.period,
				seq: reference.seq,
				account: account_id,
			});
			Ok(Pays::No.into())
		}

		/// Clear a stale notification registration and revoke its statement allowance.
		///
		/// This is a permissionless call; the origin must be authorized.
		/// Succeeds only when the registration's period-derived expiry has elapsed.
		/// On success, removes notification registration state and decreases statement allowance.
		///
		/// Parameters:
		/// * `account`: statement account previously associated with a notification registration.
		/// * `seq`: notification sequence to clear. Must match stored registration sequence and be
		///   in `0..=NotificationSlotsPerPeriod`.
		#[pallet::call_index(9)]
		#[pallet::authorize(|source, account, seq| {
			Self::authorize_clear_expired_notification_sequence(source, account, seq)
		})]
		#[pallet::weight_of_authorize(<T as Config>::WeightInfo::authorize_clear_expired_notification_sequence())]
		#[pallet::weight(<T as Config>::WeightInfo::clear_expired_notification_sequence())]
		pub fn clear_expired_notification_sequence(
			origin: OriginFor<T>,
			account: T::AccountId,
			_seq: u8,
		) -> DispatchResultWithPostInfo {
			ensure_authorized(origin)?;
			let alias = Self::notification_alias_for_account(&account)?;
			NotificationRegistrationByAlias::<T>::remove(alias);
			NotificationAliasByAccount::<T>::remove(&account);
			decrease_allowance_by(account.clone().into(), T::NotificationAllowance::get());

			Self::deposit_event(Event::NotificationStmtUsageRemoved { account });
			Ok(Pays::No.into())
		}

		/// Claim an anonymous statement store allowance for a target account.
		///
		/// The origin must be `Origin::StmtStoreAlias`, produced by the `AsResources`
		/// (`RegisterStatementStoreAllowance(..)`) transaction extension after proof validation.
		/// On success, increases the statement allowance for `target_account` and stores the
		/// mapping in `StatementStoreAllowances`.
		///
		/// Parameters:
		/// * `period`: day number since Unix epoch. Must be in the accepted period window.
		/// * `seq`: slot number within the period, bounded by the collection-specific limit.
		/// * `target_account`: statement account to authorize.
		#[pallet::call_index(10)]
		#[pallet::weight(<T as Config>::WeightInfo::set_statement_store_account())]
		pub fn set_statement_store_account(
			origin: OriginFor<T>,
			period: u32,
			seq: u32,
			target_account: T::AccountId,
		) -> DispatchResultWithPostInfo {
			let alias = Self::ensure_stmt_store_alias(origin)?;
			let period_key = BigEndianU32::from(period);
			let now = T::Clock::now().as_secs();

			// If an entry already exists for this alias in this period, the cooldown
			// since `existing.since` must have elapsed before it can be replaced.
			if let Some(existing) = StatementStoreAllowances::<T>::get(period_key, alias) {
				ensure!(
					now > existing
						.since
						.saturating_add(T::StmtStoreReplacementCooldown::get() as u64),
					Error::<T>::StmtStoreReplacementTooEarly
				);
				// Revoke the old allowance and clear the reverse lookup.
				decrease_allowance_by(
					existing.account_id.clone().into(),
					T::AccountsApiAllowance::get(),
				);
				StmtStoreAllowanceByAccount::<T>::remove(
					&existing.account_id,
					(period_key, existing.seq, alias),
				);
			}

			// Grant the statement store allowance to the target account.
			increase_allowance_by(target_account.clone().into(), T::AccountsApiAllowance::get());
			StatementStoreAllowances::<T>::insert(
				period_key,
				alias,
				StmtStoreAllowanceEntry { account_id: target_account.clone(), seq, since: now },
			);
			StmtStoreAllowanceByAccount::<T>::insert(&target_account, (period_key, seq, alias), ());
			Self::deposit_event(Event::StmtStoreAllowanceSet {
				alias,
				period,
				seq,
				account: target_account,
			});
			Ok(().into())
		}

		/// Remove expired statement store allowances for a past period.
		///
		/// This is a permissionless call; the origin must be authorized.
		/// Removes up to `StmtStoreCleanupLimit` entries from `StatementStoreAllowances` for
		/// the given `period`, decreasing the statement allowance for each removed account.
		#[pallet::call_index(11)]
		#[pallet::authorize(|source, period, first_entry| {
			Self::authorize_clear_expired_stmt_store_allowances(source, period, first_entry)
		})]
		#[pallet::weight_of_authorize(<T as Config>::WeightInfo::authorize_clear_expired_stmt_store_allowances())]
		#[pallet::weight(<T as Config>::WeightInfo::clear_expired_stmt_store_allowances(T::StmtStoreCleanupLimit::get()))]
		pub fn clear_expired_stmt_store_allowances(
			origin: OriginFor<T>,
			period: u32,
			first_entry: Alias,
		) -> DispatchResultWithPostInfo {
			ensure_authorized(origin)?;

			let period_key = BigEndianU32::from(period);
			let limit = T::StmtStoreCleanupLimit::get();
			let mut count = 0u32;

			for (alias, entry) in
				StatementStoreAllowances::<T>::drain_prefix(period_key).take(limit as usize)
			{
				decrease_allowance_by(
					entry.account_id.clone().into(),
					T::AccountsApiAllowance::get(),
				);
				StmtStoreAllowanceByAccount::<T>::remove(
					entry.account_id,
					(period_key, entry.seq, alias),
				);
				count = count.saturating_add(1);
			}

			Self::deposit_event(Event::StmtStoreAllowancesCleared {
				period,
				first_key: first_entry,
				count,
			});
			Ok(Some(T::WeightInfo::clear_expired_stmt_store_allowances(count)).into())
		}

		/// Claim long-term storage on a remote chain using an anonymous membership proof.
		///
		/// The origin must be `Origin::LongTermStorageClaim(alias, collection)`, created by the
		/// `AsResources` (`ClaimLongTermStorage(..)`) transaction extension after ring-VRF proof
		/// validation.
		///
		/// Parameters:
		/// * `period`: the claiming period. Must be the current period or the previous one if
		///   within the grace window.
		/// * `counter`: the claim counter within the period. Must be less than
		///   `LongTermStorageClaimsPerPeriod`. Each counter produces a distinct alias.
		/// * `account_id`: the account to authorize for storage on the remote chain.
		#[pallet::call_index(12)]
		#[pallet::weight(<T as Config>::WeightInfo::claim_long_term_storage())]
		pub fn claim_long_term_storage(
			origin: OriginFor<T>,
			period: u32,
			counter: u8,
			account_id: T::AccountId,
		) -> DispatchResultWithPostInfo {
			let (alias, collection) = Self::ensure_long_term_storage_claim(origin)?;

			let allocation = match collection {
				MembershipCollection::People => T::LongTermStorageAllowanceForPeople::get(),
				MembershipCollection::LitePeople => T::LongTermStorageAllowanceForLitePeople::get(),
			};

			SpentLongTermStorageAliases::<T>::insert(BigEndianU32::from(period), alias, ());

			// The alias is consumed regardless of allocation success, so a failing remote chain
			// cannot be used to spam this extrinsic with the same proof.
			if T::LongTermStorageDataStore::allocate_storage(
				&account_id,
				allocation.bytes,
				allocation.transactions,
			)
			.is_err()
			{
				Self::deposit_event(Event::LongTermStorageAllocationFailed {
					alias,
					period,
					counter,
					account: account_id,
					collection,
				});
				return Ok(Pays::No.into());
			}

			Self::deposit_event(Event::LongTermStorageClaimed {
				alias,
				period,
				counter,
				account: account_id,
				collection,
			});
			Ok(Pays::No.into())
		}

		/// Clear spent long-term storage aliases for an expired period.
		///
		/// This is a permissionless call authorized via the `authorize` attribute. It can be
		/// called by anyone once a period has fully expired (past the grace window).
		///
		/// Parameters:
		/// * `period`: the expired period to clear aliases for.
		/// * `limit`: the maximum number of entries to remove in this call.
		#[pallet::call_index(13)]
		#[pallet::authorize(|_source, period, _limit| {
			Self::authorize_clear_long_term_storage_aliases(period)
		})]
		#[pallet::weight_of_authorize(
			<T as Config>::WeightInfo::authorize_clear_expired_long_term_storage_aliases()
		)]
		#[pallet::weight(<T as Config>::WeightInfo::clear_expired_long_term_storage_aliases(*limit))]
		pub fn clear_expired_long_term_storage_aliases(
			origin: OriginFor<T>,
			period: u32,
			limit: u32,
		) -> DispatchResultWithPostInfo {
			ensure_authorized(origin)?;
			ensure!(
				limit <= T::LongTermStorageCleanupLimit::get(),
				Error::<T>::LongTermStorageCleanupLimitExceeded,
			);
			let mut count = 0u32;
			for _ in SpentLongTermStorageAliases::<T>::drain_prefix(BigEndianU32::from(period))
				.take(limit as usize)
			{
				count = count.saturating_add(1);
			}
			Self::deposit_event(Event::LongTermStorageAliasesCleared { period, count });
			Ok(Pays::No.into())
		}
	}

	#[pallet::view_functions]
	impl<T: Config> Pallet<T> {
		/// Returns the current statement store allowance period (day number since Unix epoch).
		pub fn current_stmt_store_period() -> u32 {
			Self::stmt_store_period_from_timestamp(T::Clock::now().as_secs())
		}

		/// Returns the proof context for a statement store slot claim at the given
		/// `period` and `seq`.
		///
		/// Uses the product-owned statement-store slot context family.
		pub fn stmt_store_slot_context_for(period: u32, seq: u32) -> Context {
			Self::stmt_store_slot_context(period, seq)
		}

		/// Returns the proof context for a notification registration at the given
		/// `period` and `seq`.
		pub fn notification_context_for(period: u32, seq: u8) -> Context {
			Self::notification_context(NotificationReference { period, seq })
		}

		/// Returns the current value of [`Config::StmtStoreSlotsPerPeriod`].
		pub fn get_stmt_store_slots_per_period() -> u32 {
			T::StmtStoreSlotsPerPeriod::get()
		}

		/// Returns the current value of [`Config::LiteStmtStoreSlotsPerPeriod`].
		pub fn get_lite_stmt_store_slots_per_period() -> u32 {
			T::LiteStmtStoreSlotsPerPeriod::get()
		}

		/// Returns the current value of [`Config::StmtStoreCleanupLimit`].
		pub fn get_stmt_store_cleanup_limit() -> u32 {
			T::StmtStoreCleanupLimit::get()
		}

		/// Returns the current value of [`Config::StmtStoreReplacementCooldown`].
		pub fn get_stmt_store_replacement_cooldown() -> u32 {
			T::StmtStoreReplacementCooldown::get()
		}

		/// Returns the current value of [`Config::StmtStoreGraceWindow`].
		pub fn get_stmt_store_grace_window() -> u32 {
			T::StmtStoreGraceWindow::get()
		}

		/// Returns the current value of [`Config::NotificationSlotsPerPeriod`].
		pub fn get_notification_slots_per_period() -> u8 {
			T::NotificationSlotsPerPeriod::get()
		}

		/// Returns the current value of [`Config::LiteNotificationSlotsPerPeriod`].
		pub fn get_lite_notification_slots_per_period() -> u8 {
			T::LiteNotificationSlotsPerPeriod::get()
		}

		/// Returns the current value of [`Config::LongTermStorageClaimsPerPeriod`].
		pub fn get_long_term_storage_claims_per_period() -> u8 {
			T::LongTermStorageClaimsPerPeriod::get()
		}

		/// Returns the current value of [`Config::LongTermStorageCleanupLimit`].
		pub fn get_long_term_storage_cleanup_limit() -> u32 {
			T::LongTermStorageCleanupLimit::get()
		}
	}

	impl<T: Config> Pallet<T> {
		fn authorize_demote_auth_expired(
			account: &T::AccountId,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			Self::validate_demotion(account)
				.map_err(|_| crate::extension::CustomValidity::InvalidPersonDemotion)?;
			ValidTransaction::with_tag_prefix("PersonhoodResourcesDemoteAuthExpired")
				.and_provides(account)
				.propagate(true)
				// Not storage reclamation: this downgrades the account's privileges
				// (cuts its statement allowance, marks it demoted), a user-visible
				// transition that should not keep yielding while authorization is expired.
				.priority(tx_priority::BACKGROUND_PROGRESS)
				.build()
				.map(|valid_tx| (valid_tx, Weight::zero()))
		}

		fn submit_authorized_transaction(call: Call<T>, description: &str) {
			let tx = T::create_authorized_transaction(call.into());
			match SubmitTransaction::<T, _>::submit_transaction(tx) {
				Ok(()) => log::debug!(
					target: LOG_TARGET,
					"offchain worker: submitted authorized transaction successfully for `{description}`",
				),
				Err(()) => log::warn!(
					target: LOG_TARGET,
					"offchain worker: failed to submit authorized transaction for `{description}`",
				),
			}
		}

		fn should_clear_notification_registration(
			registration: &types::NotificationRegistration<T::AccountId>,
		) -> bool {
			Self::validate_clear_notification_sequence(
				&registration.account_id,
				registration.reference.seq,
			)
			.is_ok()
		}

		/// Reject any non-local transaction source. Used by authorize closures of calls
		/// that are submitted exclusively by the offchain worker, so they should never
		/// arrive over the network from external peers.
		fn ensure_local_source(source: TransactionSource) -> Result<(), TransactionValidityError> {
			match source {
				TransactionSource::Local | TransactionSource::InBlock => Ok(()),
				TransactionSource::External => Err(InvalidTransaction::BadSigner.into()),
			}
		}

		fn authorize_clear_expired_notification_sequence(
			source: TransactionSource,
			account: &T::AccountId,
			seq: &u8,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			Self::ensure_local_source(source)?;
			Self::validate_clear_notification_sequence(account, *seq)
				.map_err(|_| crate::extension::CustomValidity::InvalidExpiredNotificationCleanup)?;
			ValidTransaction::with_tag_prefix("PersonhoodResourcesClearExpiredNotificationSequence")
				.and_provides(account)
				.propagate(true)
				.priority(tx_priority::CLEANUP)
				.build()
				.map(|valid_tx| (valid_tx, Weight::zero()))
		}

		fn authorize_clear_expired_stmt_store_allowances(
			source: TransactionSource,
			period: &u32,
			first_entry: &Alias,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			Self::ensure_local_source(source)?;
			if !Self::is_stmt_store_period_clearable(*period) {
				return Err(crate::extension::CustomValidity::InvalidExpiredStmtStoreCleanup.into());
			}
			let Some((first_period, actual_first)) =
				StatementStoreAllowances::<T>::iter_keys().next()
			else {
				return Err(crate::extension::CustomValidity::InvalidExpiredStmtStoreCleanup.into());
			};
			if first_period.0 != *period {
				return Err(crate::extension::CustomValidity::InvalidExpiredStmtStoreCleanup.into());
			}
			if actual_first != *first_entry {
				return Err(crate::extension::CustomValidity::InvalidExpiredStmtStoreCleanup.into());
			}
			ValidTransaction::with_tag_prefix("PersonhoodResourcesClearExpiredStmtStore")
				.and_provides((period, first_entry))
				.propagate(true)
				.priority(tx_priority::CLEANUP)
				.build()
				.map(|valid_tx| (valid_tx, Weight::zero()))
		}

		fn ensure_notification_alias(origin: OriginFor<T>) -> Result<Alias, DispatchError> {
			match origin.into_caller().try_into() {
				Ok(Origin::NotificationAlias(alias)) => Ok(alias),
				_ => Err(DispatchError::BadOrigin),
			}
		}

		fn ensure_long_term_storage_claim(
			origin: OriginFor<T>,
		) -> Result<(Alias, MembershipCollection), DispatchError> {
			match origin.into_caller().try_into() {
				Ok(Origin::LongTermStorageClaim(alias, collection)) => Ok((alias, collection)),
				_ => Err(DispatchError::BadOrigin),
			}
		}

		fn authorize_clear_long_term_storage_aliases(
			period: &u32,
		) -> Result<(ValidTransaction, Weight), TransactionValidityError> {
			Self::validate_clear_long_term_storage_period(*period)?;
			ensure!(
				SpentLongTermStorageAliases::<T>::iter_key_prefix(BigEndianU32::from(*period))
					.next()
					.is_some(),
				TransactionValidityError::from(
					crate::extension::CustomValidity::NothingToClearForLongTermStoragePeriod,
				)
			);
			ValidTransaction::with_tag_prefix(
				"PersonhoodResourcesClearExpiredLongTermStorageAliases",
			)
			.and_provides(period)
			.propagate(true)
			.priority(tx_priority::CLEANUP)
			.build()
			.map(|valid_tx| (valid_tx, Weight::zero()))
		}

		pub fn notification_period_from_timestamp(now_secs: u64) -> u32 {
			let period_duration = u64::from(T::NotificationPeriodDuration::get().max(1));
			let period = now_secs.checked_div(period_duration).unwrap_or(0);
			period.try_into().unwrap_or(u32::MAX)
		}

		pub fn notification_expiration_time(period: u32) -> u64 {
			let period_end = u64::from(period.saturating_add(1))
				.saturating_mul(u64::from(T::NotificationPeriodDuration::get().max(1)));
			period_end.saturating_add(u64::from(T::StmtStoreGraceWindow::get()))
		}

		/// Whether a notification period is currently accepted.
		///
		/// Only the current statement-store period is accepted for new notification claims.
		fn is_accepted_notification_period(period: u32) -> bool {
			let now_secs = T::Clock::now().as_secs();
			period == Self::notification_period_from_timestamp(now_secs)
		}

		pub fn resources_context() -> Context {
			Self::build_context(personhood::RESOURCES)
		}

		pub fn notification_context(reference: NotificationReference) -> Context {
			Self::build_context(personhood::resources_notification(reference.period, reference.seq))
		}

		/// Build the context for a statement store slot proof.
		///
		/// The raw suffix contains the allocated family followed by the period and sequence.
		pub fn stmt_store_slot_context(period: u32, seq: u32) -> Context {
			Self::build_context(personhood::statement_store_slot(period, seq))
		}

		/// Compute the statement store period from a timestamp.
		///
		/// The period is the day number since the Unix epoch (`now_secs / 86400`).
		pub fn stmt_store_period_from_timestamp(now_secs: u64) -> u32 {
			let period = now_secs.checked_div(SECONDS_PER_DAY).unwrap_or(0);
			period.try_into().unwrap_or(u32::MAX)
		}

		/// Whether a statement store period is eligible for cleanup.
		///
		/// A period becomes clearable only after its end plus the grace window. This
		/// ensures that allowances from the previous period remain active for a short
		/// overlap into the new period, giving users continuous coverage while they
		/// claim fresh slots.
		///
		/// Period `P` ends at `(P + 1) * SECONDS_PER_DAY`. Cleanup is allowed when
		/// `now > period_end + StmtStoreGraceWindow`.
		fn is_stmt_store_period_clearable(period: u32) -> bool {
			let now_secs = T::Clock::now().as_secs();
			let period_end = u64::from(period.saturating_add(1)).saturating_mul(SECONDS_PER_DAY);
			let clearable_after =
				period_end.saturating_add(u64::from(T::StmtStoreGraceWindow::get()));
			now_secs > clearable_after
		}

		fn ensure_stmt_store_alias(origin: OriginFor<T>) -> Result<Alias, DispatchError> {
			match origin.into_caller().try_into() {
				Ok(Origin::StmtStoreAlias(alias)) => Ok(alias),
				_ => Err(DispatchError::BadOrigin),
			}
		}

		// Validation functions for notification registration and clearing.
		// These are used both in the dispatchable calls and in the authorization logic.
		pub(crate) fn validate_notification_period(period: u32) -> Result<(), Error<T>> {
			ensure!(
				Self::is_accepted_notification_period(period),
				Error::<T>::InvalidNotificationPeriod
			);
			Ok(())
		}

		// The sequence number is an arbitrary identifier provided by the caller to
		// distinguish different registrations within the same period.
		// These are used both in the dispatchable calls and in the authorization logic.
		fn validate_notification_seq_with_limit(seq: u8, limit: u8) -> Result<(), Error<T>> {
			ensure!(seq <= limit, Error::<T>::InvalidNotificationSequence);
			Ok(())
		}

		pub(crate) fn validate_notification_seq(seq: u8) -> Result<(), Error<T>> {
			Self::validate_notification_seq_with_limit(seq, T::NotificationSlotsPerPeriod::get())
		}

		pub(crate) fn validate_lite_notification_seq(seq: u8) -> Result<(), Error<T>> {
			Self::validate_notification_seq_with_limit(
				seq,
				T::LiteNotificationSlotsPerPeriod::get(),
			)
		}

		// Notification registrations must reject duplicate account and alias registrations.
		// This helper is shared by dispatch and pre-dispatch validation.
		pub(crate) fn validate_notification_registration(
			alias: Alias,
			account_id: &T::AccountId,
		) -> Result<(), Error<T>> {
			ensure!(
				!NotificationRegistrationByAlias::<T>::contains_key(alias),
				Error::<T>::NotificationRegistrationAlreadyExists
			);
			if NotificationAliasByAccount::<T>::contains_key(account_id) {
				log::error!(
					target: LOG_TARGET,
					"notification registration validation found an existing account mapping for a new alias",
				);
				return Err(Error::<T>::NotificationRegistrationAlreadyExists);
			}
			Ok(())
		}

		// To clear an expired notification registration, the caller must provide
		// the account associated with the registration and the sequence number.
		fn validate_clear_notification_sequence(
			account: &T::AccountId,
			seq: u8,
		) -> Result<Alias, Error<T>> {
			let alias = Self::notification_alias_for_account(account)?;
			let Some(registration) = NotificationRegistrationByAlias::<T>::get(alias) else {
				log::error!(
					target: LOG_TARGET,
					"notification storage corruption: missing registration for alias {alias:?} mapped from account {account:?}",
				);
				return Err(Error::<T>::InvalidNotificationSequence);
			};
			if registration.account_id != *account {
				log::error!(
					target: LOG_TARGET,
					"notification storage corruption: alias {:?} maps from account {:?} but registration points to {:?}",
					alias,
					account,
					registration.account_id,
				);
				return Err(Error::<T>::InvalidNotificationSequence);
			}
			ensure!(registration.reference.seq == seq, Error::<T>::InvalidNotificationSequence);
			let now = T::Clock::now().as_secs();
			let expires_at = Self::notification_expiration_time(registration.reference.period);
			ensure!(now > expires_at, Error::<T>::NotificationRegistrationNotExpired);
			Ok(alias)
		}

		fn notification_alias_for_account(account: &T::AccountId) -> Result<Alias, Error<T>> {
			NotificationAliasByAccount::<T>::get(account)
				.ok_or(Error::<T>::InvalidNotificationSequence)
		}

		/// Construct the context for a long-term storage claim.
		pub fn long_term_storage_context(period: u32, counter: u8) -> Context {
			Self::build_context(personhood::long_term_storage(period, counter))
		}

		fn build_context(suffix: ProductContextSuffix) -> Context {
			build_product_context(personhood::PRODUCT_NAME, &T::Suffix::get(), suffix)
		}

		pub fn long_term_storage_period_from_timestamp(now_secs: u64) -> u32 {
			(now_secs / T::LongTermStoragePeriodDuration::get() as u64) as u32
		}

		pub fn is_accepted_long_term_storage_period(period: u32) -> bool {
			let now_secs = T::Clock::now().as_secs();
			let current_period = Self::long_term_storage_period_from_timestamp(now_secs);
			if period == current_period {
				return true;
			}
			let now_secs_minus_grace =
				now_secs.saturating_sub(u64::from(T::LongTermStorageGraceWindow::get()));
			let previous_period_in_grace =
				Self::long_term_storage_period_from_timestamp(now_secs_minus_grace);
			period == previous_period_in_grace
		}

		/// Whether the long-term storage `period` is past its grace window and can be cleared.
		pub(crate) fn is_long_term_storage_period_clearable(period: u32) -> bool {
			let now_secs = T::Clock::now().as_secs();
			let period_duration = T::LongTermStoragePeriodDuration::get() as u64;
			let grace = T::LongTermStorageGraceWindow::get() as u64;
			let period_claimable_until =
				(period as u64 + 1).saturating_mul(period_duration).saturating_add(grace);
			now_secs > period_claimable_until
		}

		pub(crate) fn validate_clear_long_term_storage_period(
			period: u32,
		) -> Result<(), TransactionValidityError> {
			ensure!(
				Self::is_long_term_storage_period_clearable(period),
				TransactionValidityError::from(
					crate::extension::CustomValidity::LongTermStoragePeriodNotExpired,
				)
			);
			Ok(())
		}

		/// Register a lite consumer using the provided information.
		///
		/// IMPORTANT
		///
		/// This function does not check for authorization. The caller is responsible for ensuring
		/// the `account` to be registered is a lite person and that the user's consent was
		/// provided, usually through a signature verified by the caller.
		pub fn register_lite_consumer_inner(
			account: T::AccountId,
			identifier_key: CommunicationIdentifier,
		) -> Result<(), Error<T>> {
			// Must not already be registered.
			ensure!(!Consumers::<T>::contains_key(&account), Error::<T>::AlreadyRegistered);
			// Set the consumer's record.
			Consumers::<T>::insert(
				&account,
				ConsumerInfo { identifier_key, credibility: Credibility::Lite },
			);
			frame_system::Pallet::<T>::inc_sufficients(&account);

			// A new lite person has been registered, so the initial allowance should be given
			increase_allowance_by(account.clone().into(), T::LitePersonStatementLimit::get());

			Self::deposit_event(Event::LitePersonRegistered { account });
			Ok(())
		}

		fn validate_demotion(account: &T::AccountId) -> Result<ConsumerInfo, Error<T>> {
			let consumer_info = Consumers::<T>::get(account).ok_or(Error::<T>::NotRegistered)?;
			let Credibility::Person { last_update, demoted, .. } = consumer_info.credibility else {
				return Err(Error::<T>::NotFullPerson);
			};
			let now = T::Clock::now().as_secs();
			ensure!(
				now > last_update.saturating_add(T::PersonAuthDuration::get() as u64),
				Error::<T>::PersonAuthNotExpired
			);
			ensure!(!demoted, Error::<T>::AlreadyDemoted);
			Ok(consumer_info)
		}
	}

	impl<T: Config> ConsumerRegistrar<T::AccountId> for Pallet<T> {
		type Error = Error<T>;

		fn register_lite_consumer(
			account: T::AccountId,
			identifier_key: CommunicationIdentifier,
		) -> Result<(), Error<T>> {
			Self::register_lite_consumer_inner(account, identifier_key)
		}
	}
}
