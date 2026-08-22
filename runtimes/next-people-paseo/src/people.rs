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

use super::*;
use assets_common::local_and_foreign_assets::TargetFromLeft;
use codec::{Decode, Encode};
use cumulus_primitives_core::Junction::{GeneralIndex, PalletInstance, Parachain};
use frame_support::{
	pallet_prelude::PhantomData,
	parameter_types,
	traits::{
		fungible::{HoldConsideration, ItemOf},
		ConstU128, ConstU32, ConstU8, ConstUint, Footprint, Get, LinearStoragePrice, Randomness,
	},
	PalletId,
};
#[cfg(feature = "runtime-benchmarks")]
use indiv_support::traits::PersonalId;
#[cfg(feature = "runtime-benchmarks")]
use indiv_support::traits::{Alias, RingIndex};
use indiv_support::{
	fungibles::CombineAssetsWithHolder,
	traits::{AllocateStorage, Context, PEOPLE_IDENTIFIER, PEOPLE_LITE_IDENTIFIER},
	utils::TypedGetToGet,
};
use paseo_runtime_constants::system_parachain::{
	NextAssetHubParaId, ASSET_HUB_ID, NEXT_ASSET_HUB_ID,
};
#[cfg(feature = "runtime-benchmarks")]
use sp_runtime::BoundedVec;
use sp_runtime::{
	traits::{AccountIdConversion, ConstI8, ConstU16},
	DispatchResult, MultiSignature, MultiSigner, Percent, SaturatedConversion,
};
use sp_statement_store::StatementAllowance;
use xcm::v5::{Location, WeightLimit};

use crate::{
	parameters::{
		AccountsApiAllowance, LiteNotificationSlotsPerPeriod, LitePersonRegistrationFee,
		LitePersonStatementLimit, LiteStmtStoreSlotsPerPeriod,
		LongTermStorageAllowanceForLitePeople, LongTermStorageAllowanceForPeople,
		LongTermStorageClaimsPerPeriod, LongTermStorageCleanupLimit, LongTermStorageGraceWindow,
		LongTermStoragePeriodDuration, NotificationAllowance, NotificationPeriodDuration,
		NotificationSlotsPerPeriod, PeopleAirdropsPrizeSource, PersonStatementLimit,
		StmtStoreCleanupLimit, StmtStoreGraceWindow, StmtStoreReplacementCooldown,
		StmtStoreSlotsPerPeriod,
	},
	paseo_constants::{CENTS, UNITS},
};

/// Asset id of the external asset as registered on Paseo Asset Hub.
pub const EXTERNAL_ASSET_ID: u32 = 50_000_413;

parameter_types! {
	pub const NetworkSuffix: &'static [u8] = b"paseo";
	pub const StaleAliasCleanupInterval: BlockNumber = 5 * MINUTES;
	pub ExternalAssetLocation: Location = Location::new(
		1,
		[Parachain(ASSET_HUB_ID), PalletInstance(50), GeneralIndex(EXTERNAL_ASSET_ID as u128)],
	);
}

/// The full featured fungibles implementation with both regular and hold functionality.
pub type AssetsWithHolder = CombineAssetsWithHolder<Assets, AssetsHolder>;

/// Native plus assets, with the native token under the parent location.
pub type NativeAndAssets = frame_support::traits::fungible::UnionOf<
	Balances,
	AssetsWithHolder,
	TargetFromLeft<crate::xcm_config::RelayLocation, Location>,
	Location,
	AccountId,
>;

/// A fungible implementation using the external asset id from Asset Hub.
pub type FungibleExternalAsset = ItemOf<AssetsWithHolder, ExternalAssetLocation, AccountId>;

#[cfg(feature = "runtime-benchmarks")]
pub struct BenchmarkClock;
#[cfg(feature = "runtime-benchmarks")]
impl frame_support::traits::UnixTime for BenchmarkClock {
	fn now() -> core::time::Duration {
		let now = pallet_timestamp::Now::<Runtime>::get();
		core::time::Duration::from_millis(now)
	}
}

/// Wall-clock source used by pallet `Config`s in this runtime.
#[cfg(not(feature = "runtime-benchmarks"))]
pub type RuntimeClock = Timestamp;
#[cfg(feature = "runtime-benchmarks")]
pub type RuntimeClock = BenchmarkClock;

// The `AccountContexts` type, which must implement `trait Contains` and return true only for the
// contexts the runtime supports.
pub struct AccountContexts;
impl frame_support::traits::Contains<Context> for AccountContexts {
	fn contains(l: &Context) -> bool {
		l == &indiv_pallet_mob_rule::MOB_CONTEXT ||
			l == &indiv_pallet_score::Pallet::<Runtime>::score_context() ||
			l == &indiv_pallet_resources::Pallet::<Runtime>::resources_context()
	}
}

use indiv_pallet_game::PhaseDurationValues;
use indiv_support::crypto::{BandersnatchVrfVerifiable, GenerateVerifiable};

parameter_types! {
	/// Controls the ring size for the people members collection, used for anonymous ring VRF
	/// proofs. `R2e9` maps to the small Bandersnatch ring setting. The underlying `2^9`
	/// domain reserves 257 slots for proof-system overhead (blinding and internal rows),
	/// so the effective member capacity is 255.
	pub const MembersFlexibleRingExponent: indiv_support::traits::RingExponent =
		indiv_support::traits::RingExponent::R2e9;
	/// Controls the ring size for recycler rings in coinage append-only collections.
	pub const RecyclerRingExponent: indiv_support::traits::RingExponent =
		indiv_support::traits::RingExponent::R2e10;
	/// Controls the ring size for paid unload token rings in coinage append-only collections.
	pub const PaidUnloadTokenRingExponent: indiv_support::traits::RingExponent =
		indiv_support::traits::RingExponent::R2e10;
	/// The owner of the people collection. This is set to the people pallet's own location.
	pub PeopleCollectionOwner: Location = Location::new(0, [PalletInstance(51)]);
	/// The owner of the lite people collection. This matches the `PeopleLite` pallet index.
	pub LitePeopleCollectionOwner: Location = Location::new(0, [PalletInstance(62)]);
	/// Ring exponent for lite people collection.
	pub const LitePeopleRingExponent: indiv_support::traits::RingExponent =
		indiv_support::traits::RingExponent::R2e9;
	/// Onboarding size for lite people collection.
	pub const LitePeopleOnboardingSize: u32 = 3;
	/// Pallet identifier used to derive the account that receives lite-person registration fees.
	pub const LitePeoplePotId: PalletId = PalletId(*b"plitefee");
	/// The page size for chunks manager.
	pub const ChunkPageSize: u32 = 255;
	/// Self-inclusion delay: 60 minutes.
	pub const SelfInclusionDelayValue: u64 = 3600;
}

impl indiv_pallet_chunks_manager::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_chunks_manager::WeightInfo<Runtime>;
	type Chunk = <BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk;
	type PageSize = ChunkPageSize;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = benchmark_utils::ChunksManagerBenchHelper;
}

impl indiv_pallet_members::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_members::WeightInfo<Runtime>;
	type Crypto = BandersnatchVrfVerifiable;
	type Location = xcm::v5::Location;
	type ChunksManager = ChunksManager;
	type Clock = RuntimeClock;
	type MaxCollections = ConstU32<100>;
	type OnboardingQueuePageSize = ConstU32<255>;
	type MaxFlexibleRingExponent = MembersFlexibleRingExponent;
	type RingBuildingMemberLimit = ConstU32<100>;
	/// 10 minutes in seconds for old root retention.
	type OldRootRetentionDuration = ConstU64<600>;
	type OnRingRootChange = MembersNotifier;
	type OffchainWorkerInterval = ConstU32<1>;
	type ManagerOrigin = EnsureRoot<AccountId>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = benchmark_utils::MembersBenchHelper;
}

parameter_types! {
	pub const RingBakingInterval: BlockNumber = MINUTES;
	pub const QueuePageMergingInterval: BlockNumber = 5 * MINUTES;
	pub const MaxTaskLifespan: BlockNumber = 5 * MINUTES;
}

impl indiv_pallet_people::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_people::WeightInfo<Runtime>;
	type MemberService = Members;
	type RingExponent = MembersFlexibleRingExponent;
	type CollectionOwner = PeopleCollectionOwner;
	type AccountContexts = AccountContexts;
	type OnboardingQueuePageSize = ConstU32<30>;
	type StaleAliasCleanupInterval = StaleAliasCleanupInterval;
	type SelfInclusionDelay = SelfInclusionDelayValue;
	type ManagerOrigin = EnsureRoot<AccountId>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = benchmark_utils::PeopleBenchHelper;
}

impl indiv_pallet_dummy_dim::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_dummy_dim::WeightInfo<Runtime>;
	type UpdateOrigin = EnsureRoot<AccountId>;
	type MaxPersonBatchSize = ConstU32<1000>;
	type People = People;
}

/// Shared benchmark helpers for individuality pallets.
#[cfg(feature = "runtime-benchmarks")]
pub mod benchmark_utils {
	use super::*;
	use alloc::vec::Vec;
	use frame_support::{
		pallet_prelude::PalletInfoAccess,
		traits::fungibles::{Create, Inspect},
	};
	use indiv_support::genesis::ring_verifier_builder_params;
	use verifiable::ring::RingDomainSize;

	pub fn member_from_seed(
		seed: u64,
	) -> <BandersnatchVrfVerifiable as GenerateVerifiable>::Member {
		let mut entropy = [0u8; 32];
		entropy[..8].copy_from_slice(&seed.to_le_bytes()[..]);
		let secret = BandersnatchVrfVerifiable::new_secret(entropy);
		BandersnatchVrfVerifiable::member_from_secret(&secret)
	}

	pub fn ensure_external_asset_exists() {
		if !Assets::asset_exists(ExternalAssetLocation::get()) {
			<Assets as Create<_>>::create(
				ExternalAssetLocation::get(),
				ParaId::new(<Assets as PalletInfoAccess>::index() as u32).into_account_truncating(),
				true,
				1u32.into(),
			)
			.expect("Failed to create asset");
		}
	}

	pub fn initialize_chunks(
		domain_size: RingDomainSize,
	) -> Vec<<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk> {
		ring_verifier_builder_params(domain_size)
	}

	pub struct ChunksManagerBenchHelper;

	impl
		indiv_pallet_chunks_manager::BenchmarkHelper<
			<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk,
		> for ChunksManagerBenchHelper
	{
		fn chunk_page() -> Vec<<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk> {
			let chunks = ring_verifier_builder_params(RingDomainSize::Domain16);
			chunks.into_iter().take(ChunkPageSize::get() as usize).collect()
		}
	}

	pub struct MembersBenchHelper;

	impl
		indiv_pallet_members::BenchmarkHelper<
			<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk,
		> for MembersBenchHelper
	{
		fn initialize_chunks(
			ring_size: indiv_support::traits::RingExponent,
		) -> Vec<<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk> {
			let domain_size: RingDomainSize =
				ring_size.try_into().expect("ring_size should be convertible to RingDomainSize");
			ring_verifier_builder_params(domain_size)
		}
		fn set_time(now: core::time::Duration) {
			pallet_timestamp::Now::<Runtime>::put(now.as_millis() as u64);
		}
		fn set_valid_time() {
			let duration = core::time::Duration::from_secs(5);
			pallet_timestamp::Now::<Runtime>::put(duration.as_millis() as u64);
		}
	}

	pub struct PeopleBenchHelper;

	impl
		indiv_pallet_people::BenchmarkHelper<
			<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk,
		> for PeopleBenchHelper
	{
		fn valid_account_context() -> Context {
			// Identity aliases are removed, and this runtime no longer accepts any
			// storage-backed alias contexts through `AccountContexts`.
			indiv_pallet_mob_rule::MOB_CONTEXT
		}

		fn worst_case_account_context() -> Context {
			indiv_pallet_resources::Pallet::<Runtime>::resources_context()
		}

		fn initialize_chunks() -> Vec<<BandersnatchVrfVerifiable as GenerateVerifiable>::StaticChunk>
		{
			initialize_chunks(RingDomainSize::Domain11)
		}
	}

	pub struct ResourcesBenchHelper;

	impl indiv_pallet_resources::benchmarking::BenchmarkHelper<Runtime> for ResourcesBenchHelper {
		fn set_time(now: core::time::Duration) {
			pallet_timestamp::Now::<Runtime>::put(now.as_millis() as u64);
		}

		fn sign_message(message: &[u8]) -> (sp_runtime::AccountId32, MultiSignature) {
			use sp_core::Pair;
			use sp_runtime::traits::IdentifyAccount;
			let entropy = [1u8; 32];
			let pair = sp_core::ed25519::Pair::from_seed(&entropy);
			let account = pair.public().into_account().into();
			let secret = ed25519_zebra::SigningKey::from(entropy);
			let signature = sp_core::ed25519::Signature::from_raw(secret.sign(message).into());
			(account, signature.into())
		}
	}

	/// Benchmark helper for members_notifier pallet.
	pub struct MembersNotifierBenchHelper;

	impl indiv_pallet_members_notifier::benchmarking::BenchmarkHelper<Runtime>
		for MembersNotifierBenchHelper
	{
		fn init() {
			use cumulus_pallet_parachain_system::RelevantMessagingState;
			use cumulus_primitives_core::relay_chain::AbridgedHrmpChannel;

			// Timestamp must exceed ReplayCooldownSeconds (60s) so
			// request_replay benchmark passes the cooldown check.
			pallet_timestamp::Now::<Runtime>::put(120_000u64);

			// Fake HRMP egress channels so benchmarks that send XCM succeed.
			// Benchmarks use para_ids 0..MaxSubscribers and 1000..1000+MaxSubscribers.
			let max_subscribers =
				<Runtime as indiv_pallet_members_notifier::Config>::MaxSubscribers::get();
			let channel = AbridgedHrmpChannel {
				max_capacity: 1000,
				max_total_size: 1_000_000,
				max_message_size: 100_000,
				msg_count: 0,
				total_size: 0,
				mqc_head: None,
			};
			let mut egress_channels: Vec<(ParaId, AbridgedHrmpChannel)> = (0..max_subscribers)
				.chain(1000..1000 + max_subscribers)
				.map(|i| (ParaId::from(i), channel.clone()))
				.collect();
			egress_channels.sort_by_key(|(id, _)| *id);
			egress_channels.dedup_by_key(|(id, _)| *id);

			let messaging_state =
				cumulus_pallet_parachain_system::relay_state_snapshot::MessagingStateSnapshot {
					dmq_mqc_head: Default::default(),
					relay_dispatch_queue_remaining_capacity: Default::default(),
					ingress_channels: Vec::new(),
					egress_channels,
				};
			RelevantMessagingState::<Runtime>::put(messaging_state);
		}

		fn setup_ring_roots(count: u32) {
			use indiv_support::traits::Identifier;
			use verifiable::ring::RingDomainSize;

			// Creating a valid intermediate and root using the smallest domain size.
			let intermediate = BandersnatchVrfVerifiable::start_members(RingDomainSize::Domain11);
			let root = BandersnatchVrfVerifiable::finish_members(intermediate.clone());

			// Matching the test_identifier helper from the notifier benchmarking module.
			fn test_identifier(index: u32) -> Identifier {
				let mut id = [0u8; 32];
				id[..4].copy_from_slice(&index.to_be_bytes());
				id
			}
			assert_eq!(
				test_identifier(0xDEADBEEF),
				hex_literal::hex!(
					"deadbeef00000000000000000000000000000000000000000000000000000000"
				),
				"test_identifier drifted — sync with pallets/members-notifier/src/benchmarking.rs",
			);

			// Populating ring roots for all identifiers that benchmarks may reference.
			// Benchmarks spread pending updates across MaxCollections identifiers.
			let max_collections =
				<Runtime as indiv_pallet_members_notifier::Config>::MaxCollections::get();
			for coll in 0..max_collections {
				let identifier = test_identifier(coll);
				for i in 0..count {
					let ring_root = indiv_pallet_members::RingRoot::<Runtime> {
						root: root.clone(),
						revision: 0,
						intermediate: intermediate.clone(),
					};
					indiv_pallet_members::Root::<Runtime>::insert(identifier, i, ring_root);
				}
				indiv_pallet_members::CurrentRingIndex::<Runtime>::insert(identifier, count - 1);
			}
		}

		fn set_max_message_size(size: u32) {
			use cumulus_pallet_parachain_system::RelevantMessagingState;

			// Shrinking each egress channel's max_message_size triggers the worst-case
			// chunking path in send_batch. init() must have run before this.
			let mut state = RelevantMessagingState::<Runtime>::get()
				.expect("BenchmarkHelper::init must run before set_max_message_size");
			for (_, channel) in state.egress_channels.iter_mut() {
				channel.max_message_size = size;
			}
			RelevantMessagingState::<Runtime>::put(state);
		}
	}
}

parameter_types! {
	pub const MobRulePotId: PalletId = PalletId(*b"MobRwrds");
	pub const MinTurnoutPercentage: Percent = Percent::from_percent(10);
	pub const VotingPenaltyDuration: BlockNumber = DAYS;
	pub const OffchainWorkInterval: BlockNumber = 5 * MINUTES;
	pub const MinimumVoterThreshold: u32 = 3;
}

impl indiv_pallet_mob_rule::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_mob_rule::WeightInfo<Runtime>;
	type Currency = FungibleExternalAsset;
	type CurrencyLocationInfo = ExternalAssetLocation;
	// 24 hours
	type Clock = RuntimeClock;
	type EnsurePerson = indiv_pallet_people::EnsurePersonalAliasInContext<Runtime>;
	type MaxVoteClaimDuration = ConstU64<86_400>;
	type MinCaseDuration = ConstU32<{ 10 * 60 }>;
	type MaxVotingDuration = ConstU32<{ 20 * 60 }>;
	type MinTurnoutNominal = ConstU32<1>;
	type MinTurnoutPercentage = MinTurnoutPercentage;
	type MaxPayoutRoundSchedules = ConstU32<5>;
	type VotingPenaltyDuration = VotingPenaltyDuration;
	type InterventionOrigin = EnsureRoot<AccountId>;
	type PotId = MobRulePotId;
	type MaxVotesClaimable = ConstU32<10>;
	type OffchainWorkInterval = OffchainWorkInterval;
	type CleanVotesBatchSize = ConstU32<1000>;
	type VotesOpenForClaimsDuration = ConstU32<{ 10 * 60 }>;
	type MinimumVoterThreshold = MinimumVoterThreshold;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = MobRuleBenchHelper;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct MobRuleBenchHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_mob_rule::benchmarking::BenchmarkHelper<Runtime> for MobRuleBenchHelper {
	fn set_valid_time() {
		// Needed to allow for all time-based constraints.
		// Max voting duration (20 minutes) + Max claim duration (86_400s == 24h) + buffer.
		let sufficient_time = (20 * 60 + 86_400 + 3600) * 1000u64; // in ms
		pallet_timestamp::Now::<Runtime>::put(sufficient_time);
	}

	fn setup_currency() {
		benchmark_utils::ensure_external_asset_exists();
	}
}

parameter_types! {
	pub const ProofOfInkBaseDeposit: Balance = 100 * CENTS;
	// One cent: $10,000 / MB
	pub const ProofOfInkByteDeposit: Balance = CENTS;
	pub const ProofOfInkHoldReason: RuntimeHoldReason = RuntimeHoldReason::ProofOfInk(indiv_pallet_proof_of_ink::HoldReason::ProofOfInk);
	pub const ProofOfInkPotId: PalletId = PalletId(*b"PoIPot__");
}

#[cfg(feature = "runtime-benchmarks")]
pub struct PoIBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
use indiv_pallet_proof_of_ink::ReferralTicket;

#[cfg(feature = "runtime-benchmarks")]
use alloc::{boxed::Box, vec};

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_proof_of_ink::BenchmarkHelper<Runtime> for PoIBenchmarkHelper {
	fn create_tickets(seed: u64) -> BoundedVec<ReferralTicket<AccountId>, ConstU32<10>> {
		let (_, ticket) = Self::create_ticket(seed);

		BoundedVec::<ReferralTicket<AccountId>, ConstU32<10>>::try_from(vec![ReferralTicket {
			ticket,
		}])
		.unwrap()
	}

	fn create_ticket(seed: u64) -> (MultiSigner, AccountId) {
		use sp_core::Pair;
		use sp_runtime::traits::IdentifyAccount;
		let mut entropy = [0u8; 32];
		entropy[..8].copy_from_slice(&seed.to_le_bytes()[..]);
		let pair = sp_core::ed25519::Pair::from_seed(&entropy);
		let account = pair.public().into_account().into();
		let signer: MultiSigner = pair.public().into();
		(signer, account)
	}

	fn sign(seed: u64, msg: &[u8]) -> MultiSignature {
		let mut entropy = [0u8; 32];
		entropy[..8].copy_from_slice(&seed.to_le_bytes()[..]);
		// sp-core doesn't expose the signing for the runtime, so we use the underlying library
		let secret = ed25519_zebra::SigningKey::from(entropy);
		sp_core::ed25519::Signature::from_raw(secret.sign(msg).into()).into()
	}

	fn build_person_origin(personal_id: PersonalId) -> RuntimeOrigin {
		indiv_pallet_people::Origin::PersonalIdentity(personal_id).into()
	}

	fn setup_currency() {
		benchmark_utils::ensure_external_asset_exists();
	}
}

impl indiv_pallet_proof_of_ink::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_proof_of_ink::WeightInfo<Runtime>;
	type Deposit = HoldConsideration<
		AccountId,
		Balances,
		ProofOfInkHoldReason,
		LinearStoragePrice<ProofOfInkBaseDeposit, ProofOfInkByteDeposit, Balance>,
	>;
	type People = People;
	type EnsurePerson = indiv_pallet_people::EnsurePersonalIdentity<Runtime>;
	type TicketSignature = MultiSignature;
	type TicketPublic = MultiSigner;
	type Ticket = AccountId;
	type Oracle = MobRule;
	type Randomness = SubjectBlockRandommess<Runtime>;
	#[cfg(not(feature = "runtime-benchmarks"))]
	type DataStore = BulletinDataStore;
	#[cfg(feature = "runtime-benchmarks")]
	type DataStore = BenchmarkBulletinDataStore;
	type MaxActiveReferrals = ConstU32<10>;
	type MaxRetryAttempts = ConstU32<1>;
	type MaxReimbursementValues = ConstU32<50>;
	type Currency = FungibleExternalAsset;
	type PotId = ProofOfInkPotId;
	type InvitationsOrigin = EnsureRoot<Self::AccountId>;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	type Crypto = BandersnatchVrfVerifiable;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = PoIBenchmarkHelper;
}

parameter_types! {
	pub const ScorePotId: PalletId = PalletId(*b"scorepot");
}

#[cfg(feature = "runtime-benchmarks")]
pub struct ScoreBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_score::benchmarking::BenchmarkHelper<Runtime> for ScoreBenchmarkHelper {
	fn create_member(seed: u64) -> indiv_pallet_score::MemberOf<Runtime> {
		benchmark_utils::member_from_seed(seed)
	}
	fn setup_currency() {
		benchmark_utils::ensure_external_asset_exists();
	}
}

impl indiv_pallet_score::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_score::WeightInfo<Runtime>;
	type Suffix = NetworkSuffix;
	type EnsurePerson = indiv_pallet_people::EnsurePersonalAliasInContext<Runtime>;
	type ScorePotId = ScorePotId;
	type Currency = FungibleExternalAsset;
	type CurrencyLocationInfo = ExternalAssetLocation;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	type MaxPayoutRoundSchedules = ConstU32<10>;
	type OffchainWorkInterval = ConstU32<2>;
	type People = People;
	type Crypto = BandersnatchVrfVerifiable;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ScoreBenchmarkHelper;
}

parameter_types! {
	pub const PlayDepositReason: RuntimeHoldReason =
		RuntimeHoldReason::Game(indiv_pallet_game::HoldReason::PlayDeposit);
	pub const PlayDepositDefault: Balance = 2 * UNITS;
	// TODO(paritytech/individuality#1124): find a reasonable value.
	pub PlayerStatementLimit: StatementAllowance = StatementAllowance {
		max_size: 1_000_000,
		max_count: 1_000_000,
	};
	pub GameAirdropSource: AccountId = PalletId(*b"pop/gads").into_account_truncating();
}

impl indiv_pallet_game::Config for Runtime {
	const TESTNET: bool = true;
	type WeightInfo = weights::indiv_pallet_game::WeightInfo<Runtime>;
	// The game benchmarks sweep `1..=MaxGroupSize` and `1..=MaxRounds` for their linear
	// regressions. The production bounds (6/3) are too small to fit accurate per-player and
	// per-round slopes, so the benchmarking build widens them to 10. The fitted weight
	// formulas stay valid at the production bounds, which only interpolate within the measured
	// range.
	#[cfg(not(feature = "runtime-benchmarks"))]
	type MaxGroupSize = ConstU32<6>;
	#[cfg(feature = "runtime-benchmarks")]
	type MaxGroupSize = ConstU32<10>;
	type UnixTime = RuntimeClock;
	#[cfg(not(feature = "runtime-benchmarks"))]
	type MaxRounds = ConstU32<3>;
	#[cfg(feature = "runtime-benchmarks")]
	type MaxRounds = ConstU32<10>;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	type InviteIssuer = EnsureRoot<Self::AccountId>;
	type EnsureLiteAlias = indiv_pallet_people_lite::EnsureLiteAliasInContext<Runtime>;
	type NonPlayingKickoutTime = ConstU32<{ 90 * DAYS }>;
	type NativeFungible = Balances;
	type PlayDeposit = HoldConsideration<
		AccountId,
		Balances,
		PlayDepositReason,
		sp_runtime::traits::Identity,
		Balance,
	>;
	type DefaultPlayDeposit = PlayDepositDefault;
	type TicketSignature = MultiSignature;
	type MaxGameSchedules = ConstU32<12>;
	type MaxAttendanceHistoryDepth = ConstU32<12>;
	type NftClaimCredits = NftCredits;
	type DefaultPhaseDurations = GamePhaseDurations;
	type AccountSignature = Signature;
	type PlayerStatementLimit = PlayerStatementLimit;
	type PeopleVoteWeight = ConstUint<2>;
	type CandidateVoteWeight = ConstUint<1>;
	// This is for the testnet, the value must be at least 2 in production.
	type MinGroupSize = ConstUint<0>;
	type AirdropAssetId = <Runtime as pallet_assets::Config>::AssetId;
	type AirdropAssetBalance = Balance;
	type Airdrop = Airdrop;
	type AirdropSource = GameAirdropSource;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = GamePalletBenchmarkHelper;
}

/// What the credits benchmarks cannot set up themselves: only the runtime knows how its HRMP
/// channels are made.
#[cfg(feature = "runtime-benchmarks")]
pub struct NftCreditsBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_nft_credits::benchmarking::BenchmarkHelper for NftCreditsBenchmarkHelper {
	fn set_unix_time(secs: u64) {
		// `pallet_timestamp` holds the clock in milliseconds, and its `set` is an inherent, so this
		// writes the value straight to storage.
		pallet_timestamp::Now::<Runtime>::put(secs.saturating_mul(1_000));
	}

	fn open_nft_claims_channel(max_message_size: u32) {
		use cumulus_pallet_parachain_system::RelevantMessagingState;
		use cumulus_primitives_core::relay_chain::AbridgedHrmpChannel;

		let channel = AbridgedHrmpChannel {
			max_capacity: 1000,
			max_total_size: 1_000_000,
			max_message_size,
			msg_count: 0,
			total_size: 0,
			mqc_head: None,
		};
		let claims_chain = <Runtime as indiv_pallet_nft_credits::Config>::NftClaimsParaId::get();
		let mut messaging_state = RelevantMessagingState::<Runtime>::get().unwrap_or(
			cumulus_pallet_parachain_system::relay_state_snapshot::MessagingStateSnapshot {
				dmq_mqc_head: Default::default(),
				relay_dispatch_queue_remaining_capacity: Default::default(),
				ingress_channels: Vec::new(),
				egress_channels: Vec::new(),
			},
		);
		messaging_state.egress_channels.retain(|(id, _)| *id != claims_chain);
		messaging_state.egress_channels.push((claims_chain, channel));
		messaging_state.egress_channels.sort_by_key(|(id, _)| *id);
		RelevantMessagingState::<Runtime>::put(messaging_state);
	}
}

impl indiv_pallet_nft_credits::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_nft_credits::WeightInfo<Runtime>;
	// Sized above what one block can award, so no `report` awards a credit the block has no room
	// for, which would be committed to no root and lost. A worst-case report awards
	// `(MaxGroupSize - 1) * MaxRounds = 15` credits and is charged the awards entry's
	// `MaxEncodedLen` of `2 + 65 * MaxCreditsPerBlock` bytes, so at 1200 about 65 reports fit the
	// 7,864,320-byte `Normal` proof budget, awarding 975 credits together. The game pallet's
	// `integrity_test` recomputes that floor from the block limits and the generated `report`
	// weight, so a value below it fails `runtime_integrity_tests`.
	//
	// The remaining fifth is margin against a regeneration that makes a report cheaper, fitting
	// more per block and lifting the floor. It is cheap, since the charge that buys it lowers the
	// floor in turn: at 1080 the floor was 1050, thin enough that any regeneration would have
	// moved it past.
	//
	// The claims chain sizes its claimed-leaf bitmap from its own copy of this bound and refuses a
	// tree over it, so raising this needs `MaxCreditsPerAwardBlock` raised there first.
	type MaxCreditsPerBlock = ConstU32<1200>;
	type XcmRouter = crate::xcm_config::XcmRouter;
	type NftClaimsParaId = NextAssetHubParaId;
	// Matches the `NftClaims` index in next-asset-hub-paseo's `construct_runtime!`.
	type NftClaimsPalletIndex = ConstU8<96>;
	type ChannelInfo = ParachainSystem;
	// One tree per block at most, and the offchain worker ships them every block, so the queue
	// only fills while delivery to Asset Hub is down. Matched to `MaxRetainedAwardBlocks`, which
	// counts the same award blocks: the oldest tree still queued is then one whose awards are
	// ordinarily still in state, so a delivery that outlasts the outage needs no proof rebuilt
	// from events. A `replay_credit_trees` during the outage breaks that, its tree being claimable
	// on Asset Hub while the delivery is still queued here, so its last claim there has that
	// chain ask for a deletion the queue entry then finds nothing to deliver. Eight full messages
	// drain it.
	//
	// An entry is 12 bytes and the queue is read at the value's `MaxEncodedLen`, so
	// `authorize_send_credit_trees` pays about 3 KB of the `Normal` proof budget for it. A tree
	// past the bound is dropped from delivery, not lost: its root stays on chain for
	// `replay_credit_trees`.
	type MaxQueuedCreditTrees = ConstU32<256>;
	type MaxCreditTreesPerMessage = ConstU32<32>;
	type ReplayCooldownSeconds = ConstU64<60>;
	type NftClaimsRemoteWeight = NftClaimsRemoteWeight;
	// Entries are the distinct blocks a claimant was awarded in, not a window of consecutive
	// ones, so the bound counts games rather than time. One game awards a claimant at most
	// `(MaxGroupSize - 1) * MaxRounds = 15` credits, one per co-player that reported `Person`
	// on them, plus the attendance backfill, which awards the rest in a single call. Those
	// land in 16 distinct blocks only if no two reports ever share one, out of the 300 blocks
	// the 10-minute reporting phase spans; reports cluster, so a few per game is the norm.
	//
	// A game cycle runs 17.5 minutes, so back to back games fill this in about two hours at
	// the usual few entries each, and in two games if both hit the worst case. That is the
	// intended horizon: the index is a lookup aid for trees recent enough to still be worth
	// minting against, not a record for the chain's lifetime, and the oldest block drops out
	// once it is full.
	type MaxCreditBlocksPerClaimant = ConstU32<32>;
	// The window in which a claim is provable from state alone, counted in award blocks. Reports
	// cluster inside a game's 10-minute reporting phase, so a game contributes a few dozen award
	// blocks and this covers several games, well past the two hours the per-claimant index spans.
	//
	// It is also the state the chain carries for them: at most this many entries of
	// `MaxCreditsPerBlock` awards, an award being 65 bytes, so about 17 MB were every retained
	// block saturated, and proportional to the mints actually outstanding otherwise. A block that
	// drops out delays no mint, because its root stays on chain until the claims chain is finished
	// with it or the root TTL runs out. Its awards then have to come from the block's events.
	type MaxRetainedAwardBlocks = ConstU32<256>;
	type EnsureClaimsChainOrigin = EnsureClaimsChainSibling;
	// At least the claims pallet's `MaxTreeDeletionsPerMessage`. A larger message fails to decode
	// here, and the root TTL then removes the roots its deletions named.
	type MaxTreeDeletionsPerMessage = ConstU32<64>;
	type ClaimsChainTreeTtl = ClaimsChainTreeTtl;
	// One block records at most one root, so a day holds 43200 of them at 2 seconds a block, which
	// 64 a block clears in about 20 minutes. The root TTL is the longer of the two, so a sweep only
	// removes roots the claims chain has already given up on, with a month of slack for a backlog.
	type MaxRootsPerSweep = ConstU32<64>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = NftCreditsBenchmarkHelper;
}

parameter_types! {
	/// The claims chain's `TreeTtl`, duplicated here. The root TTL this chain sweeps by is
	/// `ROOT_TTL_GRACE` past it, so a root outlives the tree built from it.
	///
	/// Keep it in step with `CreditTreeTtl` in next-asset-hub-paseo. A value below the real one keeps
	/// roots for less time than the claims chain gives a claimant, which strands credits inside their
	/// deadline. A value above it keeps roots after the last credit has expired.
	pub const ClaimsChainTreeTtl: u64 = 90 * 24 * 60 * 60;
}

/// Origin check for the parachain the credit trees are delivered to. Only that chain may name the
/// roots this chain deletes.
///
/// Any origin that passes this check can strand a credit, so it accepts that one chain, not
/// siblings in general.
pub struct EnsureClaimsChainSibling;
impl frame_support::traits::EnsureOrigin<RuntimeOrigin> for EnsureClaimsChainSibling {
	type Success = ();

	fn try_origin(o: RuntimeOrigin) -> Result<Self::Success, RuntimeOrigin> {
		let claims_chain = <Runtime as indiv_pallet_nft_credits::Config>::NftClaimsParaId::get();
		match o.clone().into() {
			Ok(cumulus_pallet_xcm::Origin::SiblingParachain(id)) if id == claims_chain => Ok(()),
			_ => Err(o),
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
		let claims_chain = <Runtime as indiv_pallet_nft_credits::Config>::NftClaimsParaId::get();
		Ok(cumulus_pallet_xcm::Origin::SiblingParachain(claims_chain).into())
	}
}

parameter_types! {
	pub const HonourPointFreezeDuration: indiv_pallet_honour::Seconds = 24 * 60 * 60;
	pub const HonourCallMortality: indiv_pallet_honour::Seconds = 5 * 60;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct HonourBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_honour::benchmarking::BenchmarkHelper<Runtime> for HonourBenchmarkHelper {
	fn set_time(now: indiv_pallet_honour::Seconds) {
		pallet_timestamp::Now::<Runtime>::put(now.saturating_mul(1_000));
	}

	fn seed_and_create_proof(
		vote: &indiv_pallet_honour::VoteData,
		message: &[u8],
	) -> indiv_pallet_honour::RingProofOf<Runtime> {
		use alloc::{vec, vec::Vec};
		use indiv_support::traits::{AppendOnlyMembers, RingMode};
		use verifiable::ring::RingDomainSize;

		let ring_exponent = <Runtime as indiv_pallet_people::Config>::RingExponent::get();
		let ring_index: RingIndex = 0;

		// Build a one-member people ring in the configured member service. Mirrors the targeted
		// setup used by `indiv_pallet_people`'s own proof benchmarks (`create_collection` +
		// `add_members` + `onboard_all_and_build_ring`) rather than the full `process_maintenance`
		// sweep, which is heavier and runs once per benchmark repeat.
		Members::create_collection(
			PeopleCollectionOwner::get(),
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			1,
			RingMode::Flexible,
			ring_exponent,
			None,
		)
		.expect("benchmark: people collection must be created");

		let secret =
			BandersnatchVrfVerifiable::new_secret(sp_core::twox_256(b"honour-bench-voter"));
		let member = BandersnatchVrfVerifiable::member_from_secret(&secret);

		Members::add_members(indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER, vec![member])
			.expect("benchmark: ring member must be added");
		Members::initialize_chunks(ring_exponent);
		Members::onboard_all_and_build_ring(
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			ring_index,
		)
		.expect("benchmark: people ring must be built");

		// Open a commitment against the ring members the member service just baked in.
		let ring_members =
			Members::ring_members(indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER, ring_index);
		let domain: RingDomainSize =
			ring_exponent.try_into().expect("people ring exponent maps to a domain size");
		let commitment = BandersnatchVrfVerifiable::open(domain, &member, ring_members.into_iter())
			.expect("benchmark: commitment must open");

		let contexts = vote.get_contexts();
		let contexts: Vec<&[u8]> = contexts.iter().map(|c| &c[..]).collect();
		let (proof, _) = BandersnatchVrfVerifiable::create_multi_context(
			commitment, &secret, &contexts, message,
		)
		.expect("benchmark: proof creation must succeed");
		proof
	}
}

impl indiv_pallet_honour::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_honour::WeightInfo<Runtime>;
	type MemberService = Members;
	type Clock = Timestamp;
	type PointFreezeDuration = HonourPointFreezeDuration;
	type CallMortality = HonourCallMortality;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = HonourBenchmarkHelper;
}

parameter_types! {
	pub const AirdropPalletId: PalletId = PalletId(*b"pop/adrp");
}

impl indiv_pallet_airdrop::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_airdrop::WeightInfo<Runtime>;
	type MemberService = Members;
	type Fungibles = AssetsWithHolder;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	type PalletId = AirdropPalletId;
	type UnixTime = RuntimeClock;
	// The relay block randomness is known by the validator producing it as soon as the previous
	// epoch ends. This pallet is used alongside indiv-pallet-game. Therefore new player can
	// register with new keys while knowing the randomness if the validator shared it to them.
	// If there is less than 5K players, they can then reach personhood and win, if there is more
	// than 5K players they can't reach personhood but prevent others from winning.
	//
	// Alternatively we can use the relay one epoch ago randomness, this would result in waiting one
	// epoch after closing registration before drawing the winners.
	type Randomness = indiv_pallet_relay_randomness::RelayBlockRandomness<Runtime>;
	type AccountIdToPublic = AccountIdToSr25519Public;
	type ClearLimit = ConstU32<100>;
	type DrawLimit = ConstU32<100>;
	type OffchainWorkerInterval = ConstU32<1>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = AirdropBenchmarkHelper;
}

/// Direct byte-level reinterpretation of an `AccountId32` as an sr25519 public key.
pub struct AccountIdToSr25519Public;
impl sp_runtime::traits::TryConvert<AccountId, sp_core::sr25519::Public>
	for AccountIdToSr25519Public
{
	fn try_convert(account: AccountId) -> Result<sp_core::sr25519::Public, AccountId> {
		let raw: [u8; 32] = account.clone().into();
		Ok(sp_core::sr25519::Public::from_raw(raw))
	}
}

#[cfg(feature = "runtime-benchmarks")]
pub struct AirdropBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_airdrop::benchmarking::BenchmarkHelper<Runtime> for AirdropBenchmarkHelper {
	fn set_unix_time(now: core::time::Duration) {
		pallet_timestamp::Now::<Runtime>::put(now.as_millis() as u64);
	}

	fn create_asset_id_parameter(id: u32) -> <Runtime as pallet_assets::Config>::AssetId {
		// Mirror `AssetsBenchmarkHelper::create_asset_id_parameter` and ensure the asset exists
		// so the airdrop pallet's pot can hold/transfer.
		use frame_support::traits::fungibles::Create;
		let location = xcm::latest::Location::new(
			1,
			[
				xcm::latest::Junction::Parachain(ASSET_HUB_ID),
				xcm::latest::Junction::PalletInstance(50),
				xcm::latest::Junction::GeneralIndex(id as u128),
			],
		);
		if !<Assets as frame_support::traits::fungibles::Inspect<AccountId>>::asset_exists(
			location.clone(),
		) {
			let owner: AccountId =
				parachain_info::Pallet::<Runtime>::parachain_id().into_account_truncating();
			<Assets as Create<AccountId>>::create(location.clone(), owner, true, 1u32.into())
				.expect("create asset for airdrop bench");
		}
		location
	}

	fn build_membership_proof(
		context: &indiv_support::traits::Context,
		message: &[u8],
		member_seed: u32,
	) -> (indiv_pallet_airdrop::ProofOf<Runtime>, indiv_support::traits::Alias) {
		use indiv_support::{
			crypto::BandersnatchSuite,
			genesis::ring_verifier_builder_params,
			traits::{RingMode, PEOPLE_IDENTIFIER},
		};
		use verifiable::ring::RingDomainSize;

		type Crypto = BandersnatchVrfVerifiable;

		let ring_exponent = MembersFlexibleRingExponent::get();
		let domain: RingDomainSize =
			ring_exponent.try_into().expect("RingExponent → RingDomainSize");
		let chunks = ring_verifier_builder_params::<BandersnatchSuite>(domain);

		let mut entropy = [0u8; 32];
		entropy[..4].copy_from_slice(&member_seed.to_le_bytes());
		let secret = Crypto::new_secret(entropy);
		let member = Crypto::member_from_secret(&secret);

		// Build a single-member ring with `member`. The resulting `members` value is the on-chain
		// ring root we seed below so verification at `(PEOPLE_IDENTIFIER, ring=0, rev=0)` succeeds.
		let mut intermediate = Crypto::start_members(domain);
		Crypto::push_members(&mut intermediate, core::iter::once(member), |range| {
			Ok(chunks[range].to_vec())
		})
		.expect("push_members for single bench member");
		let members = Crypto::finish_members(intermediate.clone());

		// Seed the Members pallet so `verify_membership(PEOPLE_IDENTIFIER, 0, 0, ...)`
		// works.
		if indiv_pallet_members::Collections::<Runtime>::get(PEOPLE_IDENTIFIER).is_none() {
			indiv_pallet_members::Collections::<Runtime>::insert(
				PEOPLE_IDENTIFIER,
				indiv_pallet_members::types::CollectionInfo {
					owner: indiv_pallet_members::types::CollectionOwner::External(
						PeopleCollectionOwner::get(),
					),
					mode: RingMode::Flexible,
					ring_size: ring_exponent,
					self_inclusion_delay: Some(SelfInclusionDelayValue::get()),
				},
			);
		}
		indiv_pallet_members::Root::<Runtime>::insert(
			PEOPLE_IDENTIFIER,
			0u32,
			indiv_pallet_members::types::RingRoot {
				root: members.clone(),
				revision: 0,
				intermediate,
			},
		);

		let commitment =
			Crypto::open(domain, &member, core::iter::once(member)).expect("open commitment");
		let (proof, _aliases) =
			Crypto::create_multi_context(commitment, &secret, &[&context[..]], message)
				.expect("create membership proof");
		let alias = Crypto::alias_in_context(&secret, &context[..]).expect("alias_in_context");
		(proof, alias)
	}

	fn account_keypair_for(seed: u32) -> (AccountId, sp_core::sr25519::Pair) {
		use sp_core::Pair as _;
		use sp_runtime::traits::IdentifyAccount;
		let mut entropy = [0u8; 32];
		entropy[..4].copy_from_slice(&seed.to_le_bytes());
		let pair = sp_core::sr25519::Pair::from_seed(&entropy);
		let account_id: AccountId = MultiSigner::Sr25519(pair.public()).into_account();
		(account_id, pair)
	}
}

impl indiv_pallet_people_airdrops::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_people_airdrops::WeightInfo<Runtime>;
	type Suffix = NetworkSuffix;
	type EnsurePerson = indiv_pallet_people::EnsurePersonalAliasInContext<Runtime>;
	type AirdropAssetId = <Runtime as pallet_assets::Config>::AssetId;
	type AirdropAssetBalance = Balance;
	type Airdrop = Airdrop;
	type ManagerOrigin = EnsureRoot<Self::AccountId>;
	type PrizeSource = PeopleAirdropsPrizeSource;
	type Randomness = indiv_pallet_relay_randomness::RelayBlockRandomness<Runtime>;
	type UnixTime = RuntimeClock;
	type MaxScheduleBatch = ConstU32<16>;
	type MaxRegisterBatch = ConstU32<16>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = PeopleAirdropsBenchmarkHelper;
}

/// Benchmark hooks for the people-airdrops pallet: places draws into lifecycle phases by writing
/// the airdrop pallet's storage directly, since the `Airdrop` trait deliberately cannot.
#[cfg(feature = "runtime-benchmarks")]
pub struct PeopleAirdropsBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_people_airdrops::benchmarking::BenchmarkHelper<Runtime>
	for PeopleAirdropsBenchmarkHelper
{
	fn fund_prize_source(
		source: &AccountId,
		draws: u32,
	) -> alloc::vec::Vec<indiv_pallet_people_airdrops::AirdropEventInfoOf<Runtime>> {
		use frame_support::traits::fungibles::Mutate;
		use indiv_pallet_airdrop::{benchmarking::BenchmarkHelper as _, pallet::SupportedAssets};
		const BENCH_ASSET_BASE: u32 = 42;
		const BENCH_PRIZE: Balance = 1_000;
		// `UnixTime::now` is read on the claim path; make sure it is past genesis.
		if pallet_timestamp::Now::<Runtime>::get() == 0 {
			Self::set_unix_time(1);
		}
		let pot = indiv_pallet_airdrop::Pallet::<Runtime>::airdrop_pot_id();
		// One distinct asset per draw so a batch schedule touches distinct asset storage per draw
		// (see the `BenchmarkHelper` trait doc).
		(0..draws)
			.map(|i| {
				let asset_id =
					AirdropBenchmarkHelper::create_asset_id_parameter(BENCH_ASSET_BASE + i);
				// Mirror `enable_asset`: mark the asset supported and keep the pot's asset account
				// alive.
				if !SupportedAssets::<Runtime>::contains_key(&asset_id) {
					Assets::mint_into(asset_id.clone(), &pot, 1).expect("fund pot ed");
					SupportedAssets::<Runtime>::insert(&asset_id, 1u128);
				}
				Assets::mint_into(asset_id.clone(), source, BENCH_PRIZE)
					.expect("fund prize source");
				indiv_pallet_people_airdrops::AirdropEventInfoOf::<Runtime> {
					prize: indiv_pallet_airdrop::types::AirdropPrize {
						asset_id,
						asset_amount: BENCH_PRIZE,
						max_winners: 1,
						winner_cap: sp_runtime::Permill::one(),
					},
					registration_starts: 100,
					draw_time: 200,
					end_time: 300,
				}
			})
			.collect()
	}

	fn open_registration(event_id: &indiv_pallet_airdrop::types::EventId) {
		indiv_pallet_airdrop::pallet::Events::<Runtime>::mutate(event_id, |event| {
			if let Some(event) = event {
				event.status =
					indiv_pallet_airdrop::types::Status::Registering { total_participants: 0 };
			}
		});
	}

	fn start_claiming(event_id: &indiv_pallet_airdrop::types::EventId) {
		use indiv_pallet_airdrop::pallet::{Registrations, Winners};
		let registrations =
			Registrations::<Runtime>::iter_prefix(event_id).collect::<alloc::vec::Vec<_>>();
		for (slot, entry) in &registrations {
			Winners::<Runtime>::insert(event_id, entry.clone(), *slot);
		}
		indiv_pallet_airdrop::pallet::Events::<Runtime>::mutate(event_id, |event| {
			if let Some(event) = event {
				event.status = indiv_pallet_airdrop::types::Status::Claiming {
					total_participants: registrations.len() as u32,
					effective_winners: registrations.len() as u32,
					claimed: 0,
				};
			}
		});
	}

	fn count_registrations(event_id: &indiv_pallet_airdrop::types::EventId) -> u32 {
		indiv_pallet_airdrop::pallet::Registrations::<Runtime>::iter_prefix(event_id).count() as u32
	}

	fn count_winners(event_id: &indiv_pallet_airdrop::types::EventId) -> u32 {
		indiv_pallet_airdrop::pallet::Winners::<Runtime>::iter_prefix(event_id).count() as u32
	}

	fn set_unix_time(now_secs: u64) {
		pallet_timestamp::Now::<Runtime>::put(now_secs * 1_000);
	}
}

#[cfg(feature = "runtime-benchmarks")]
pub struct GamePalletBenchmarkHelper {}

#[cfg(feature = "runtime-benchmarks")]
impl GamePalletBenchmarkHelper {
	fn sign(seed: u64, msg: &[u8]) -> Signature {
		let mut entropy = [0u8; 32];
		entropy[..8].copy_from_slice(&seed.to_le_bytes()[..]);
		// sp-core doesn't expose the signing for the runtime, so we use the underlying library
		let secret = ed25519_zebra::SigningKey::from(entropy);
		sp_core::ed25519::Signature::from_raw(secret.sign(msg).into()).into()
	}

	fn create_account_id(seed: u64) -> AccountId {
		use sp_core::Pair;
		use sp_runtime::traits::IdentifyAccount;
		let mut entropy = [0u8; 32];
		entropy[..8].copy_from_slice(&seed.to_le_bytes()[..]);
		let pair = sp_core::ed25519::Pair::from_seed(&entropy);
		pair.public().into_account().into()
	}
}

#[cfg(feature = "runtime-benchmarks")]
impl
	indiv_pallet_game::BenchmarkHelper<
		Signature,
		MultiSignature,
		AccountId,
		AccountId,
		<Runtime as pallet_assets::Config>::AssetId,
	> for GamePalletBenchmarkHelper
{
	fn create_account(seed: u64) -> AccountId {
		Self::create_account_id(seed)
	}

	fn sign_account(seed: u64, msg: &[u8]) -> Signature {
		Self::sign(seed, msg)
	}

	fn create_ticket(seed: u64) -> AccountId {
		Self::create_account_id(seed)
	}

	fn sign_ticket(seed: u64, msg: &[u8]) -> MultiSignature {
		Self::sign(seed, msg)
	}

	fn set_valid_time() {
		Timestamp::set_timestamp(1u32.into());
	}

	fn set_time(now: core::time::Duration) {
		// We don't call `set_timestamp` directly because it triggers checks such as aura slot
		pallet_timestamp::Now::<Runtime>::put(now.as_millis() as u64);
	}

	fn fund_account(acc: AccountId) {
		use frame_support::traits::Currency;
		let balance = 1_000_000_000_000_000u128;
		let _ = Balances::make_free_balance_be(&acc, balance);
	}

	fn airdrop_asset_id() -> <Runtime as pallet_assets::Config>::AssetId {
		ExternalAssetLocation::get()
	}
}

parameter_types! {
	/// Upper bound on what one credit tree of a `receive_credit_trees` batch costs to execute on
	/// the NFT claims chain. Charged to the caller of `replay_credit_trees`, so a repair pays for
	/// the remote work it causes. What bounds replay traffic is `ReplayCooldownSeconds`; this
	/// prices the work the replays that pass it cause.
	///
	/// Derived from the marginal per-tree cost of `receive_credit_trees` on Asset Hub: one
	/// `CreditTrees` read and write, the per-tree execution term, and the `max_size` of a
	/// `CreditTrees` entry in proof. Rounded up, and both dimensions carried, since a batch that
	/// only paid `ref_time` would push a proof no one was charged for. Re-derive it whenever the
	/// claims chain's `indiv_pallet_nft_claims` weights are regenerated; `integrity_test` holds
	/// the whole `replay_credit_trees` charge, surcharge included, to the block's budget.
	pub NftClaimsRemoteWeight: Weight = Weight::from_parts(150_000_000, 2_600);
}

pub struct GamePhaseDurations;
impl Get<PhaseDurationValues> for GamePhaseDurations {
	fn get() -> PhaseDurationValues {
		PhaseDurationValues {
			registration: 5 * 60,
			shuffle: 60,
			post_shuffle_margin: 30,
			reporting: 10 * 60,
			player_process: 60,
		}
	}
}

/// Parachain ID of the Bulletin Chain used for data storage via XCM.
pub const BULLETIN_CHAIN_PARA_ID: u32 = 1501;

#[cfg(feature = "runtime-benchmarks")]
pub struct BenchNoopXcmSender;
#[cfg(feature = "runtime-benchmarks")]
impl xcm::v5::SendXcm for BenchNoopXcmSender {
	type Ticket = ();

	fn validate(
		_destination: &mut Option<Location>,
		_message: &mut Option<xcm::v5::Xcm<()>>,
	) -> xcm::v5::SendResult<Self::Ticket> {
		Ok(((), xcm::v5::Assets::new()))
	}

	fn deliver(_ticket: Self::Ticket) -> Result<xcm::v5::XcmHash, xcm::v5::SendError> {
		Ok([0u8; 32])
	}
}

parameter_types! {
	/// XCM destination location for the Bulletin Chain.
	pub BulletinChainLocation: Location = Location::new(1, [Parachain(BULLETIN_CHAIN_PARA_ID)]);
}

// The contexts in which lite people may set up account aliases. The pallet's own authentication
// context, and the score context, in which a lite person can designate an account to play the game.
pub struct LitePeopleAccountContexts;
impl frame_support::traits::Contains<Context> for LitePeopleAccountContexts {
	fn contains(l: &Context) -> bool {
		l == &indiv_pallet_people_lite::Pallet::<Runtime>::auth_context() ||
			l == &indiv_pallet_score::Pallet::<Runtime>::score_context()
	}
}

#[cfg(feature = "runtime-benchmarks")]
pub struct PeopleLiteBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_people_lite::BenchmarkHelper<AccountId, Signature> for PeopleLiteBenchmarkHelper {
	fn sign_message(message: &[u8]) -> (AccountId, Signature) {
		<() as indiv_pallet_people_lite::BenchmarkHelper<AccountId, Signature>>::sign_message(
			message,
		)
	}

	fn worst_case_account_context(_default: Context) -> Context {
		indiv_pallet_score::Pallet::<Runtime>::score_context()
	}
}

impl indiv_pallet_people_lite::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_people_lite::WeightInfo<Runtime>;
	type Currency = Balances;
	type PotId = LitePeoplePotId;
	type RegistrationFee = LitePersonRegistrationFee;
	type Suffix = NetworkSuffix;
	type AttestationAllowanceManager = EnsureRoot<Self::AccountId>;
	type MemberService = Members;
	type CollectionOwner = LitePeopleCollectionOwner;
	type LiteRingExponent = LitePeopleRingExponent;
	type LiteOnboardingSize = LitePeopleOnboardingSize;
	type AttestationSignature = Signature;
	type LiteConsumerRegistrar = Resources;
	type AccountContexts = LitePeopleAccountContexts;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = PeopleLiteBenchmarkHelper;
}

parameter_types! {
	pub const MinUsernameLength: u32 = 6;
	pub const PersonAuthDuration: u32 = 2 * 24 * 60 * 60; // 2 days
	pub const MinPersonAuthUpdateInterval: u32 = 24 * 60 * 60; // 1 day
	pub const MaxReservationQueueLength: u32 = 10;
}

impl indiv_pallet_resources::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_resources::WeightInfo<Runtime>;
	type Suffix = NetworkSuffix;
	type MemberService = Members;
	type MinUsernameLength = MinUsernameLength;
	type PersonAuthDuration = PersonAuthDuration;
	type AccountsApiAllowance = AccountsApiAllowance;
	type StmtStoreSlotsPerPeriod = StmtStoreSlotsPerPeriod;
	type LiteStmtStoreSlotsPerPeriod = LiteStmtStoreSlotsPerPeriod;
	type StmtStoreCleanupLimit = StmtStoreCleanupLimit;
	type StmtStoreReplacementCooldown = StmtStoreReplacementCooldown;
	type StmtStoreGraceWindow = StmtStoreGraceWindow;
	type NotificationAllowance = NotificationAllowance;
	type NotificationSlotsPerPeriod = NotificationSlotsPerPeriod;
	type LiteNotificationSlotsPerPeriod = LiteNotificationSlotsPerPeriod;
	type NotificationPeriodDuration = NotificationPeriodDuration;
	type OffchainWorkerInterval = ConstU32<1>;
	type MinPersonAuthUpdateInterval = MinPersonAuthUpdateInterval;
	type EnsurePerson = indiv_pallet_people::EnsurePersonalAliasInContext<Runtime>;
	type EnsureLitePerson = indiv_pallet_people_lite::EnsureLitePerson<Runtime>;
	type Clock = RuntimeClock;
	type OffchainSignature = Signature;
	type LitePersonStatementLimit = LitePersonStatementLimit;
	type PersonStatementLimit = PersonStatementLimit;
	type MaxReservationQueueLength = MaxReservationQueueLength;
	type ManagerOrigin = EnsureRoot<AccountId>;
	type LongTermStoragePeriodDuration = LongTermStoragePeriodDuration;
	type LongTermStorageGraceWindow = LongTermStorageGraceWindow;
	type LongTermStorageClaimsPerPeriod = LongTermStorageClaimsPerPeriod;
	type LongTermStorageAllowanceForPeople = LongTermStorageAllowanceForPeople;
	type LongTermStorageAllowanceForLitePeople = LongTermStorageAllowanceForLitePeople;
	#[cfg(not(feature = "runtime-benchmarks"))]
	type LongTermStorageDataStore = BulletinDataStore;
	#[cfg(feature = "runtime-benchmarks")]
	type LongTermStorageDataStore = BenchmarkBulletinDataStore;
	type LongTermStorageCleanupLimit = LongTermStorageCleanupLimit;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = benchmark_utils::ResourcesBenchHelper;
}

parameter_types! {
	pub const CoinagePalletId: PalletId = PalletId(*b"coinage ");
}

/// The asset amount of a coin of denomination zero. The asset has 6 decimals, so this is $0.01.
#[cfg(any(test, feature = "runtime-benchmarks"))]
pub const COINAGE_ASSET_UNIT: Balance = 10u128.pow(4);

#[cfg(feature = "runtime-benchmarks")]
pub struct BenchmarkBulletinDataStore;
#[cfg(feature = "runtime-benchmarks")]
impl AllocateStorage<AccountId> for BenchmarkBulletinDataStore {
	fn allocate_storage(_who: &AccountId, _len: u64, _count: u32) -> DispatchResult {
		Ok(())
	}

	fn refresh_allocation(_who: &AccountId) -> DispatchResult {
		Ok(())
	}
}

#[cfg(feature = "runtime-benchmarks")]
pub struct CoinageBenchHelper;

#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_coinage::BenchmarkHelper<Runtime> for CoinageBenchHelper {
	fn setup_assets() {
		use frame_support::traits::fungibles::{Inspect, Mutate};
		benchmark_utils::ensure_external_asset_exists();
		if indiv_pallet_coinage::AssetToInstance::<Runtime>::iter_key_prefix(
			ExternalAssetLocation::get(),
		)
		.next()
		.is_none()
		{
			// What governance is expected to do before creating an instance: give the pallet
			// account a balance buffer so fee flows that empty its free balance cannot kill it.
			<AssetsWithHolder as Mutate<_>>::mint_into(
				ExternalAssetLocation::get(),
				&indiv_pallet_coinage::Pallet::<Runtime>::pallet_account(),
				<AssetsWithHolder as Inspect<_>>::minimum_balance(ExternalAssetLocation::get()),
			)
			.expect("minting the pallet account buffer should succeed");
			Coinage::create_sufficient_instance(
				RuntimeOrigin::root(),
				ExternalAssetLocation::get(),
				COINAGE_ASSET_UNIT,
			)
			.expect("create_sufficient_instance should succeed");
		}
	}

	fn setup_asset_without_instance() -> Location {
		use frame_support::traits::fungibles::{Inspect, Mutate};
		benchmark_utils::ensure_external_asset_exists();
		// What governance does before `create_sufficient_instance`: the pallet account's balance
		// buffer.
		<AssetsWithHolder as Mutate<_>>::mint_into(
			ExternalAssetLocation::get(),
			&indiv_pallet_coinage::Pallet::<Runtime>::pallet_account(),
			<AssetsWithHolder as Inspect<_>>::minimum_balance(ExternalAssetLocation::get()),
		)
		.expect("minting the pallet account buffer should succeed");
		ExternalAssetLocation::get()
	}

	fn fund_account(who: &AccountId, amount: u128) {
		use frame_support::traits::fungibles::Mutate;
		<AssetsWithHolder as Mutate<_>>::mint_into(ExternalAssetLocation::get(), who, amount)
			.expect("Failed to fund account");
	}

	fn create_extra_asset(seed: u32, who: &AccountId) -> Location {
		use frame_support::traits::{
			fungibles::{Create, Inspect, Mutate},
			PalletInfoAccess,
		};
		let location = Self::extra_asset_id(seed);
		if !Assets::asset_exists(location.clone()) {
			<Assets as Create<_>>::create(
				location.clone(),
				ParaId::new(<Assets as PalletInfoAccess>::index() as u32).into_account_truncating(),
				true,
				1u32.into(),
			)
			.expect("Failed to create extra asset");
		}
		<AssetsWithHolder as Mutate<_>>::mint_into(location.clone(), who, 1_000_000 * UNITS)
			.expect("Failed to fund extra asset");
		location
	}

	fn extra_asset_id(seed: u32) -> Location {
		Location::new(
			1,
			[
				Parachain(ASSET_HUB_ID),
				PalletInstance(50),
				GeneralIndex(1_000_000u128 + seed as u128),
			],
		)
	}
	fn set_time(now: core::time::Duration) {
		pallet_timestamp::Now::<Runtime>::put(now.as_millis() as u64);
	}
	fn setup_fee_conversion() {
		use frame_support::traits::fungible::Mutate as _;

		let native = crate::xcm_config::RelayLocation::get();
		let asset = ExternalAssetLocation::get();
		if pallet_asset_conversion::Pools::<Runtime>::contains_key((native.clone(), asset.clone()))
		{
			return;
		}

		// Native has 10 decimals, the external asset has 6, and 1 raw asset ($10^-6) is worth
		// 10^4 raw native ($10^-10), so the pool holds that ratio. The depth is far above any
		// benchmarked fee so that the conversions do not move the price.
		let native_liquidity: Balance = 1_000 * UNITS;
		let asset_liquidity: Balance = native_liquidity / 10_000;

		let provider: AccountId = [42u8; 32].into();
		Balances::mint_into(&provider, native_liquidity.saturating_mul(2))
			.expect("failed to fund the liquidity provider with native");
		Self::fund_account(&provider, asset_liquidity.saturating_mul(2));

		let origin = RuntimeOrigin::signed(provider.clone());
		crate::AssetConversion::create_pool(
			origin.clone(),
			Box::new(native.clone()),
			Box::new(asset.clone()),
		)
		.expect("failed to create the fee conversion pool");
		crate::AssetConversion::add_liquidity(
			origin,
			Box::new(native),
			Box::new(asset),
			native_liquidity,
			asset_liquidity,
			1,
			1,
			provider,
		)
		.expect("failed to add liquidity to the fee conversion pool");
	}

	fn create_people_proof(
		context: &[u8],
		msg: &[u8],
		_alias: Alias,
	) -> indiv_pallet_people::MembershipProof<Runtime> {
		use frame_support::dispatch::RawOrigin;
		use indiv_support::traits::{AddOnlyPeopleTrait, AppendOnlyMembers};
		use verifiable::ring::RingDomainSize;

		// Initialize the people collection and chunks if not already created
		indiv_pallet_people::Pallet::<Runtime>::initialize_people_collection();
		let ring_exponent = <Runtime as indiv_pallet_people::Config>::RingExponent::get();
		indiv_pallet_members::Pallet::<Runtime>::initialize_chunks(ring_exponent);

		let entropy = sp_core::twox_256(b"people_for_coinage:42");
		let secret = BandersnatchVrfVerifiable::new_secret(entropy);
		let member = BandersnatchVrfVerifiable::member_from_secret(&secret);

		// Set onboarding size so members get onboarded immediately
		indiv_pallet_members::OnboardingSize::<Runtime>::insert(
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			1,
		);

		// Use force_recognize_personhood to add member
		indiv_pallet_people::Pallet::<Runtime>::force_recognize_personhood(
			RawOrigin::Root.into(),
			vec![member],
		)
		.expect("should recognize personhood");

		// Onboard all members and build the ring
		indiv_pallet_members::Pallet::<Runtime>::process_maintenance();

		// Get ring keys from members pallet (page 0)
		let ring_index: RingIndex = 0;
		let ring_keys = indiv_pallet_members::RingKeys::<Runtime>::get((
			indiv_pallet_people::PEOPLE_MEMBER_IDENTIFIER,
			ring_index,
			0u32,
		));

		let commitment = BandersnatchVrfVerifiable::open(
			RingDomainSize::Domain11,
			&member,
			ring_keys.into_iter(),
		)
		.expect("should open commitment");
		let (proof, _alias) = BandersnatchVrfVerifiable::create(commitment, &secret, context, msg)
			.expect("should create proof");

		indiv_pallet_people::MembershipProof { proof, ring: ring_index, revision: 0 }
	}

	fn create_lite_people_proof(
		context: &[u8],
		msg: &[u8],
		_alias: Alias,
	) -> indiv_pallet_people::MembershipProof<Runtime> {
		use indiv_support::traits::AppendOnlyMembers as _;
		use sp_core::Pair;
		use sp_runtime::traits::IdentifyAccount;
		use verifiable::ring::RingDomainSize;

		let ring_exponent = LitePeopleRingExponent::get();
		indiv_pallet_members::Pallet::<Runtime>::initialize_chunks(ring_exponent);

		let entropy = [77u8; 32];
		let pair = sp_core::ed25519::Pair::from_seed(&entropy);
		let account: AccountId = pair.public().into_account().into();

		let ring_secret = BandersnatchVrfVerifiable::new_secret([88u8; 32]);
		let ring_member = BandersnatchVrfVerifiable::member_from_secret(&ring_secret);

		indiv_pallet_people_lite::LitePeople::<Runtime>::insert(
			&account,
			indiv_pallet_people_lite::types::LitePersonInfo {
				ring_vrf_key: ring_member,
				method: indiv_pallet_people_lite::types::RecognitionMethod::UniqueDevice(
					account.clone(),
				),
			},
		);
		frame_system::Pallet::<Runtime>::inc_sufficients(&account);
		Members::create_collection(
			LitePeopleCollectionOwner::get(),
			indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER,
			LitePeopleOnboardingSize::get(),
			indiv_support::traits::RingMode::AppendOnly,
			LitePeopleRingExponent::get(),
			None,
		)
		.expect("benchmark: lite people collection must be created");
		Members::add_members(
			indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER,
			vec![ring_member],
		)
		.expect("benchmark: lite people member must be added");
		indiv_pallet_members::OnboardingSize::<Runtime>::insert(
			indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER,
			1,
		);
		indiv_pallet_members::Pallet::<Runtime>::process_maintenance();

		let ring_index: RingIndex = 0;
		let ring_keys = indiv_pallet_members::RingKeys::<Runtime>::get((
			indiv_pallet_people_lite::LITE_PEOPLE_MEMBER_IDENTIFIER,
			ring_index,
			0u32,
		));
		let commitment = BandersnatchVrfVerifiable::open(
			RingDomainSize::Domain11,
			&ring_member,
			ring_keys.into_iter(),
		)
		.expect("should open commitment");
		let (proof, _) = BandersnatchVrfVerifiable::create(commitment, &ring_secret, context, msg)
			.expect("should create lite proof");

		indiv_pallet_people::MembershipProof { proof, ring: ring_index, revision: 0 }
	}
}

/// Prices the permanent footprint of a sponsored coinage instance.
pub struct CoinageInstanceCreationPrice;
impl sp_runtime::traits::Convert<Footprint, Balance> for CoinageInstanceCreationPrice {
	fn convert(footprint: Footprint) -> Balance {
		deposit(footprint.count.saturated_into(), footprint.size.saturated_into())
	}
}

parameter_types! {
	pub const CoinageInstanceCreationHoldReason: RuntimeHoldReason =
		RuntimeHoldReason::Coinage(indiv_pallet_coinage::HoldReason::InstanceCreationDeposit);
	pub storage CoinageLoadDeposit: (Location, Balance) =
		(crate::xcm_config::RelayLocation::get(), deposit(4, 300));
	pub storage CoinageEnablePermissionless: bool = true;
}

impl indiv_pallet_coinage::Config for Runtime {
	type MemberService = Members;
	type RecyclerRingExponent = RecyclerRingExponent;
	type PaidUnloadTokenRingExponent = PaidUnloadTokenRingExponent;
	type UnixTime = RuntimeClock;
	type PalletId = CoinagePalletId;
	type WeightInfo = weights::indiv_pallet_coinage::WeightInfo<Runtime>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = CoinageBenchHelper;
	type MaximumAge = ConstU16<16>;
	type NativeFungible = Balances;
	type Fungibles = NativeAndAssets;
	type AdminOrigin = EnsureRoot<AccountId>;
	type SponsorOrigin = frame_system::EnsureSigned<AccountId>;
	type EnablePermissionless = CoinageEnablePermissionless;
	type LoadDeposit = CoinageLoadDeposit;
	type InstanceCreationDeposit = HoldConsideration<
		AccountId,
		Balances,
		CoinageInstanceCreationHoldReason,
		CoinageInstanceCreationPrice,
	>;
	type MinimumExponent = ConstI8<0>;
	type MaximumExponent = ConstI8<14>;
	type MinimumExponentForOutputUnloadFee = ConstI8<0>;
	type MaxSplitOutputs = ConstU32<32>;
	type MaxConsolidation = ConstU32<64>;
	type MaxBatchUnpaidLoad = ConstU32<10>;
	type RecyclerExpirationTime = ConstU32<{ 90 * 24 * 60 * 60 }>; // ~3 months
	type UnloadTokenTimePeriodPeopleLitePeople = ConstU32<{ 24 * 60 * 60 }>; // 1 day

	// Allowance of 20 whole native tokens per time period (fee is dynamic based on multiplier)
	// (Usage is also capped by `MaxFreeUnloadTokensPerTimePeriod`)
	type UnloadTokenAllowancePerTimePeriodForPeople = ConstU128<{ 20 * UNITS }>;
	// Allowance of 10 whole native tokens per time period (fee is dynamic based on multiplier)
	// (Usage is also capped by `MaxFreeUnloadTokensPerTimePeriod`)
	type UnloadTokenAllowancePerTimePeriodForLitePeople = ConstU128<{ 10 * UNITS }>;
	// Bumped temporarily; revisit once the wallet handles `maxFee` and the
	// "user ran out of free unloads" UX.
	type MaxFreeUnloadTokensPerTimePeriod = ConstU32<1000>;
	type MembershipProof = People;
	type FeeConversion = crate::AssetConversion;
	type NativeAssetKind = crate::xcm_config::RelayLocation;
	type WeightToFee = TransactionPayment;
	type PaidUnloadTokenTimePeriod = ConstU32<{ 3 * 24 * 60 * 60 }>; // 3 days
	type PaidUnloadTokenRingExpirationTime = ConstU32<{ 4 * 24 * 60 * 60 }>; // 4 days
	type FeeDestination = TypedGetToGet<pallet_collator_selection::StakingPotAccountId<Runtime>>;
	type OffchainWorkerInterval = ConstU32<4>; // higher in prod
	type CoinFailureLockPeriod = ConstU64<60>;
}

/// Origin check that validates the caller is a sibling parachain and extracts its `ParaId`.
pub struct EnsureSiblingParachain;
impl frame_support::traits::EnsureOrigin<RuntimeOrigin> for EnsureSiblingParachain {
	type Success = cumulus_primitives_core::ParaId;

	fn try_origin(o: RuntimeOrigin) -> Result<Self::Success, RuntimeOrigin> {
		match o.clone().into() {
			Ok(cumulus_pallet_xcm::Origin::SiblingParachain(id)) => Ok(id),
			_ => Err(o),
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	fn try_successful_origin() -> Result<RuntimeOrigin, ()> {
		Ok(cumulus_pallet_xcm::Origin::SiblingParachain(ASSET_HUB_ID.into()).into())
	}
}

impl indiv_pallet_members_notifier::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_members_notifier::WeightInfo<Runtime>;
	type XcmRouter = crate::xcm_config::XcmRouter;
	type ChannelInfo = ParachainSystem;
	type ManageOrigin = EnsureRoot<AccountId>;
	type EnsureSubscriberOrigin = EnsureSiblingParachain;
	type Crypto = BandersnatchVrfVerifiable;
	type RingRootsProvider = Members;
	type Clock = RuntimeClock;
	type MaxSubscribers = ConstU32<10>;
	type MaxUpdatesPerBatch = ConstU32<10>;
	type MaxCollectionsPerSubscriber = ConstU32<3>;
	type MaxCollections = ConstU32<100>;
	type UpdateTriggerBlocks = ConstU32<1>;
	type UpdateTriggerThreshold = ConstU32<1>;
	type RequestReplayRemoteWeight = ConstantWeight;
	type OffchainWorkerInterval = ConstU32<1>;
	type StuckBatchTimeout = ConstU32<100>;
	type ReplayCooldownSeconds = ConstU64<60>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = benchmark_utils::MembersNotifierBenchHelper;
}

parameter_types! {
	pub ConstantWeight: Weight = Weight::from_parts(10_000, 0);
}

parameter_types! {
	pub AssetHubSubscriptionWhitelist:
		alloc::vec::Vec<indiv_pallet_members_notifier::GenesisWhitelistEntry> =
			asset_hub_subscription_whitelist();
}

/// Pallet index of `MembersSubscriber` in next-asset-hub-paseo's `construct_runtime!`.
pub const NEXT_ASSET_HUB_MEMBERS_SUBSCRIBER_INDEX: u8 = 97;

/// Subscription whitelist seeded into `MembersNotifier` at genesis.
///
/// permissionlessly one-shot, afterwards only a governance call may re-activate
/// the subscription. Identifiers are required to be sorted ascending.
pub fn asset_hub_subscription_whitelist(
) -> alloc::vec::Vec<indiv_pallet_members_notifier::GenesisWhitelistEntry> {
	alloc::vec![indiv_pallet_members_notifier::GenesisWhitelistEntry {
		para_id: ParaId::from(NEXT_ASSET_HUB_ID),
		collections: alloc::vec![
			(*PEOPLE_IDENTIFIER, MembersFlexibleRingExponent::get().exponent()),
			(*PEOPLE_LITE_IDENTIFIER, LitePeopleRingExponent::get().exponent()),
		],
		pallet_index: NEXT_ASSET_HUB_MEMBERS_SUBSCRIBER_INDEX,
	}]
}

pub struct SubjectBlockRandommess<Runtime>(PhantomData<Runtime>);
impl<Runtime> Randomness<[u8; 32], u32> for SubjectBlockRandommess<Runtime>
where
	Runtime: frame_system::Config,
	[u8; 32]: From<<Runtime as frame_system::Config>::Hash>,
{
	fn random(subject: &[u8]) -> ([u8; 32], u32) {
		// hash subject into 32 bytes
		let subject_hash = subject.using_encoded(sp_io::hashing::blake2_256);

		// hash current block into 32 bytes
		let block_number = frame_system::Pallet::<Runtime>::block_number();
		let block_hash = frame_system::Pallet::<Runtime>::block_hash(block_number);
		let block_hash: [u8; 32] = block_hash.into();

		// bitwise XOR on subject hash and block hash
		let mut randomness = [0u8; 32];
		for byte in 0..subject_hash.len() {
			randomness[byte] = subject_hash[byte] ^ block_hash[byte];
		}
		(randomness, 0)
	}
}

/// Encoding of Bulletin runtime pallets used to construct remote calls.
#[derive(Encode, Decode)]
enum BulletinPallets<AccountId: Encode> {
	/// The Bulletin TransactionStorage pallet.
	#[codec(index = 40)]
	TransactionStorage(TransactionStorageCalls<AccountId>),
}

/// Call encoding for Bulletin TransactionStorage calls invoked over XCM.
#[derive(Encode, Decode)]
enum TransactionStorageCalls<AccountId: Encode> {
	// call index: 3
	// pub fn authorize_account(
	// 	origin: OriginFor<T>,
	// 	who: T::AccountId,
	// 	transactions: u32,
	// 	bytes: u64,
	// )
	#[codec(index = 3)]
	AuthorizeAccount(AccountId, u32, u64),
	// call index: 7
	// pub fn refresh_account_authorization(
	// 	origin: OriginFor<T>,
	// 	who: T::AccountId,
	// )
	#[codec(index = 7)]
	RefreshAccountAuthorization(AccountId),
}

/// XCM router used by `BulletinDataStore`. Under `runtime-benchmarks` it swaps to a
/// no-op sender so benchmarks don't drive `xcmp_queue` (which emits `Deliver error
/// NoChannel` WARN lines) for messages destined to a chain that isn't reachable from
/// the benchmark state.
#[cfg(feature = "runtime-benchmarks")]
type BulletinXcmRouter = BenchNoopXcmSender;
#[cfg(not(feature = "runtime-benchmarks"))]
type BulletinXcmRouter = xcm_config::XcmRouter;

#[allow(unused)]
pub struct BulletinDataStore;
impl AllocateStorage<AccountId> for BulletinDataStore {
	fn allocate_storage(who: &AccountId, len: u64, count: u32) -> DispatchResult {
		Self::send(
			BulletinPallets::TransactionStorage(TransactionStorageCalls::AuthorizeAccount(
				who.clone(),
				count,
				len,
			))
			.encode(),
		)
	}

	fn refresh_allocation(who: &AccountId) -> DispatchResult {
		Self::send(
			BulletinPallets::TransactionStorage(
				TransactionStorageCalls::RefreshAccountAuthorization(who.clone()),
			)
			.encode(),
		)
	}
}

impl BulletinDataStore {
	/// The long-term storage protocol has a fixed sibling-parachain destination.
	fn bulletin_chain_location() -> Location {
		BulletinChainLocation::get()
	}

	fn send(call: alloc::vec::Vec<u8>) -> DispatchResult {
		let program = alloc::vec![
			UnpaidExecution { weight_limit: WeightLimit::Unlimited, check_origin: None },
			Transact { origin_kind: OriginKind::Xcm, fallback_max_weight: None, call: call.into() },
		]
		.into();

		send_xcm::<BulletinXcmRouter>(Self::bulletin_chain_location(), program)
			.map(|_| ())
			.map_err(|_| pallet_xcm::Error::<Runtime>::SendFailure)?;
		Ok(())
	}
}
