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

#![allow(non_local_definitions)]
#![cfg_attr(not(feature = "std"), no_std)]
#![recursion_limit = "256"]
#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

extern crate alloc;

mod genesis_config_presets;
pub mod parameters;
pub mod people;
mod weights;
pub mod xcm_config;

#[cfg(test)]
mod integration_tests;
mod migrations;

#[cfg(not(feature = "std"))]
use alloc::vec;
use alloc::vec::Vec;
use assets_common::local_and_foreign_assets::{ForeignAssetReserveData, TargetFromLeft};
#[cfg(not(feature = "runtime-benchmarks"))]
use assets_common::migrations::foreign_assets_reserves::ForeignAssetsReservesMigration;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use cumulus_pallet_parachain_system::{RelayNumberMonotonicallyIncreases, RelaychainDataProvider};
use cumulus_primitives_core::{AggregateMessageOrigin, ParaId, VerifySchedulingSignature};
#[cfg(not(feature = "runtime-benchmarks"))]
use frame_support::traits::NeverEnsureOrigin;
use frame_support::{
	construct_runtime, derive_impl,
	dispatch::DispatchClass,
	genesis_builder_helper::{build_state, get_preset},
	parameter_types,
	traits::{
		fungible, fungibles::Balanced as _, tokens::imbalance::ResolveAssetTo,
		AsEnsureOriginWithArg, ConstBool, ConstU128, ConstU32, ConstU64, ConstU8, ContainsPair,
		EitherOfDiverse, InstanceFilter, TransformOrigin,
	},
	weights::{ConstantMultiplier, Weight, WeightToFee as _},
	PalletId,
};
use frame_system::{
	limits::{BlockLength, BlockWeights},
	offchain::{CreateAuthorizedTransaction, CreateBare, CreateTransaction, CreateTransactionBase},
	EnsureRoot,
};
use indiv_pallet_origin_restriction::Allowance;
use indiv_support::traits::Alias;
use pallet_xcm::{EnsureXcm, IsVoiceOfBody};
use parachains_common::{
	impls::DealWithFees,
	message_queue::{NarrowOriginToSibling, ParaIdToSibling},
	AccountId, Balance, BlockNumber, Hash, Header, Nonce, Signature, AVERAGE_ON_INITIALIZE_RATIO,
	NORMAL_DISPATCH_RATIO,
};
#[cfg(feature = "runtime-benchmarks")]
use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
use polkadot_runtime_common::{BlockHashCount, SlowAdjustingFeeUpdate};
use sp_api::impl_runtime_apis;
pub use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_core::{crypto::KeyTypeId, OpaqueMetadata};
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;
use sp_runtime::{
	generic, impl_opaque_keys,
	traits::{AccountIdLookup, BlakeTwo256, Block as BlockT},
	transaction_validity::{TransactionSource, TransactionValidity},
	ApplyExtrinsicResult, MultiSignature, MultiSigner,
};
pub use sp_runtime::{MultiAddress, Perbill, Permill};
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;
/// Paseo network constants - defined locally to avoid version conflicts
pub mod paseo_constants {
	use super::*;

	// The following constants are enabling Elastic Scaling with 2s block times:
	/// Build with an offset of 1 behind the relay chain.
	pub const RELAY_PARENT_OFFSET: u32 = 1;
	/// The upper limit of how many parachain blocks are processed by the relay chain per
	/// parent. Limits the number of blocks authored per slot. This determines the minimum
	/// block time of the parachain:
	/// `RELAY_CHAIN_SLOT_DURATION_MILLIS/BLOCK_PROCESSING_VELOCITY`
	pub const BLOCK_PROCESSING_VELOCITY: u32 = 3;
	/// Maximum number of blocks simultaneously accepted by the Runtime, not yet included
	/// into the relay chain.
	pub const UNINCLUDED_SEGMENT_CAPACITY: u32 =
		(3 + RELAY_PARENT_OFFSET) * BLOCK_PROCESSING_VELOCITY;
	/// Relay chain slot duration, in milliseconds.
	pub const RELAY_CHAIN_SLOT_DURATION_MILLIS: u32 = 6000;

	// Currency (Paseo: UNITS = 10_000_000_000, same as DOT)
	pub const UNITS: Balance = 10_000_000_000;
	pub const DOLLARS: Balance = UNITS;
	pub const CENTS: Balance = DOLLARS / 100;
	pub const MILLICENTS: Balance = CENTS / 1_000;

	// Relay chain existential deposit
	pub const RELAY_EXISTENTIAL_DEPOSIT: Balance = CENTS * 100; // 1 DOT

	// People parachain existential deposit (1/10 of relay)
	pub const EXISTENTIAL_DEPOSIT: Balance = RELAY_EXISTENTIAL_DEPOSIT / 10;

	// Treasury pallet ID on Paseo relay
	pub const TREASURY_PALLET_ID: u8 = 19;

	/// Deposit calculation for storage
	pub const fn deposit(items: u32, bytes: u32) -> Balance {
		items as Balance * CENTS * 100 + (bytes as Balance) * CENTS
	}

	/// Weight to fee converter
	pub struct WeightToFee;
	impl frame_support::weights::WeightToFee for WeightToFee {
		type Balance = Balance;
		fn weight_to_fee(weight: &Weight) -> Self::Balance {
			use frame_support::weights::constants::WEIGHT_REF_TIME_PER_SECOND;
			let ref_time = weight.ref_time() as u128;
			CENTS.saturating_mul(ref_time) / (WEIGHT_REF_TIME_PER_SECOND as u128)
		}
	}
}

pub use paseo_constants::WeightToFee;
use paseo_constants::{
	deposit, BLOCK_PROCESSING_VELOCITY, CENTS, EXISTENTIAL_DEPOSIT, MILLICENTS,
	RELAY_CHAIN_SLOT_DURATION_MILLIS, RELAY_PARENT_OFFSET, UNINCLUDED_SEGMENT_CAPACITY, UNITS,
};
use weights::{BlockExecutionWeight, ExtrinsicBaseWeight, RocksDbWeight};
use xcm::prelude::*;
use xcm_config::{
	FellowshipLocation, GovernanceLocation, PriceForSiblingParachainDelivery, XcmConfig,
	XcmOriginToTransactDispatchOrigin,
};
use xcm_runtime_apis::{
	dry_run::{CallDryRunEffects, Error as XcmDryRunApiError, XcmDryRunEffects},
	fees::Error as XcmPaymentApiError,
};

/// Parameters supporting async backing functionality.
pub mod async_backing {
	use frame_support::weights::{constants::WEIGHT_REF_TIME_PER_SECOND, Weight};
	pub use parachains_common::BlockNumber;
	use sp_runtime::Perbill;

	/// The average expected block time that we are targeting.
	pub const MILLISECS_PER_BLOCK: u64 = 2_000;
	pub const SLOT_DURATION: u64 = 12_000;

	// Time is measured by number of blocks.
	pub const MINUTES: BlockNumber = 60_000 / (MILLISECS_PER_BLOCK as BlockNumber);
	pub const HOURS: BlockNumber = MINUTES * 60;
	pub const DAYS: BlockNumber = HOURS * 24;

	/// We assume that ~5% of the block weight is consumed by `on_initialize` handlers. This is
	/// used to limit the maximal weight of a single extrinsic.
	pub const AVERAGE_ON_INITIALIZE_RATIO: Perbill = Perbill::from_percent(5);

	/// We allow `Normal` extrinsics to fill up the block up to 85%, the rest can be used by
	/// Operational  extrinsics.
	pub const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(85);

	/// We allow for 2 seconds of compute with a 2 second average block time.
	pub const MAXIMUM_BLOCK_WEIGHT: Weight = Weight::from_parts(
		WEIGHT_REF_TIME_PER_SECOND.saturating_mul(2),
		cumulus_primitives_core::relay_chain::MAX_POV_SIZE as u64,
	);
}

use async_backing::*;

/// The address format for describing accounts.
pub type Address = MultiAddress<AccountId, ()>;

/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

/// A Block signed with an [`sp_runtime::Justification`].
pub type SignedBlock = generic::SignedBlock<Block>;

/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;

/// The extension to the basic transaction logic.
pub type TransactionExtension = cumulus_pallet_weight_reclaim::StorageWeightReclaim<
	Runtime,
	(
		// Origin modifiers
		(
			(),
			pallet_verify_signature::VerifySignature<Runtime>,
			indiv_pallet_people::extension::AsPerson<Runtime>,
			indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant<Runtime>,
			indiv_pallet_score::ScoreAsParticipant<Runtime>,
			indiv_pallet_game::GameAsInvited<Runtime>,
			indiv_pallet_people_lite::extension::PeopleLiteAuth<Runtime>,
			indiv_pallet_members::extension::AsMember<Runtime>,
			indiv_pallet_coinage::extension::AsCoinage<Runtime>,
			indiv_pallet_resources::extension::AsResources<Runtime>,
			indiv_pallet_honour::extension::VoterAuth<Runtime>,
			frame_system::AuthorizeCall<Runtime>,
		),
		// General checks and operations
		indiv_pallet_origin_restriction::RestrictOrigin<Runtime>,
		frame_system::CheckNonZeroSender<Runtime>,
		frame_system::CheckSpecVersion<Runtime>,
		frame_system::CheckTxVersion<Runtime>,
		frame_system::CheckGenesis<Runtime>,
		frame_system::CheckEra<Runtime>,
		frame_system::CheckNonce<Runtime>,
		frame_system::CheckWeight<Runtime>,
		pallet_skip_feeless_payment::SkipCheckIfFeeless<
			Runtime,
			pallet_asset_tx_payment::ChargeAssetTxPayment<Runtime>,
		>,
	),
>;

/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
	generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TransactionExtension>;

/// Migrations to apply on runtime upgrade.
pub type Migrations = (
	pallet_collator_selection::migration::v2::MigrationToV2<Runtime>,
	cumulus_pallet_xcmp_queue::migration::v6::MigrateV5ToV6<Runtime>,
	cumulus_pallet_xcmp_queue::migration::v7::MigrateV6ToV7<Runtime>,
	// Single use! - remove once the upgrade carrying it is live.
	indiv_pallet_members_notifier::migration::SeedSubscriptionWhitelist<
		Runtime,
		people::AssetHubSubscriptionWhitelist,
	>,
	indiv_pallet_nft_credits::migration::MigrateV0ToV1<Runtime>,
	// permanent
	pallet_xcm::migration::MigrateToLatestXcmVersion<Runtime>,
	// permanent, a no-op once the chunk page hashes are set (via genesis on this runtime)
	migrations::ChunkPageHashesInitialization<Runtime, indiv_support::crypto::BandersnatchSuite>,
	// permanent, a no-op once the people collection exists
	indiv_pallet_people::migration::CreatePeopleCollection<Runtime>,
	// permanent, a no-op once the lite people collection exists
	indiv_pallet_people_lite::migration::CreateLitePeopleCollection<Runtime>,
);

/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
	Runtime,
	Block,
	frame_system::ChainContext<Runtime>,
	Runtime,
	AllPalletsWithSystem,
	Migrations,
>;

impl_opaque_keys! {
	pub struct SessionKeys {
		pub aura: Aura,
	}
}

#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
	spec_name: alloc::borrow::Cow::Borrowed("next-people-paseo"),
	impl_name: alloc::borrow::Cow::Borrowed("next-people-paseo"),
	authoring_version: 1,
	spec_version: 3_000_000,
	impl_version: 0,
	apis: RUNTIME_API_VERSIONS,
	transaction_version: 5,
	system_version: 1,
};

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
	NativeVersion { runtime_version: VERSION, can_author_with: Default::default() }
}

parameter_types! {
	pub const Version: RuntimeVersion = VERSION;
	pub RuntimeBlockLength: BlockLength = BlockLength::builder()
		.max_length(5 * 1024 * 1024)
		.modify_max_length_for_class(DispatchClass::Normal, |v| {
			*v = NORMAL_DISPATCH_RATIO * (5 * 1024 * 1024u32)
		})
		.build();
	pub RuntimeBlockWeights: BlockWeights = BlockWeights::builder()
		.base_block(BlockExecutionWeight::get())
		.for_class(DispatchClass::all(), |weights| {
			weights.base_extrinsic = ExtrinsicBaseWeight::get();
		})
		.for_class(DispatchClass::Normal, |weights| {
			weights.max_total = Some(NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT);
		})
		.for_class(DispatchClass::Operational, |weights| {
			weights.max_total = Some(MAXIMUM_BLOCK_WEIGHT);
			// Operational transactions have some extra reserved space, so that they
			// are included even if block reached `MAXIMUM_BLOCK_WEIGHT`.
			weights.reserved = Some(
				MAXIMUM_BLOCK_WEIGHT - NORMAL_DISPATCH_RATIO * MAXIMUM_BLOCK_WEIGHT
			);
		})
		.avg_block_initialization(AVERAGE_ON_INITIALIZE_RATIO)
		.build_or_panic();
	pub const SS58Prefix: u8 = 42;
}

#[derive_impl(frame_system::config_preludes::ParaChainDefaultConfig)]
impl frame_system::Config for Runtime {
	type BaseCallFilter = frame_support::traits::Everything;
	type AccountId = AccountId;
	type RuntimeCall = RuntimeCall;
	type Lookup = AccountIdLookup<AccountId, ()>;
	type Nonce = Nonce;
	type Hash = Hash;
	type Hashing = BlakeTwo256;
	type Block = Block;
	type RuntimeEvent = RuntimeEvent;
	type RuntimeOrigin = RuntimeOrigin;
	type BlockHashCount = BlockHashCount;
	type Version = Version;
	type PalletInfo = PalletInfo;
	type AccountData = pallet_balances::AccountData<Balance>;
	type OnNewAccount = ();
	type OnKilledAccount = ();
	type DbWeight = RocksDbWeight;
	type SystemWeightInfo = weights::frame_system::WeightInfo<Runtime>;
	type BlockWeights = RuntimeBlockWeights;
	type BlockLength = RuntimeBlockLength;
	type SS58Prefix = SS58Prefix;
	type OnSetCode = cumulus_pallet_parachain_system::ParachainSetCode<Self>;
	type MaxConsumers = ConstU32<16>;
	type MultiBlockMigrator = MultiBlockMigrations;
}

impl pallet_timestamp::Config for Runtime {
	/// A timestamp: milliseconds since the unix epoch.
	type Moment = u64;
	type OnTimestampSet = Aura;
	type MinimumPeriod = ConstU64<0>;
	type WeightInfo = weights::pallet_timestamp::WeightInfo<Runtime>;
}

impl pallet_authorship::Config for Runtime {
	type FindAuthor = pallet_session::FindAccountFromAuthorIndex<Self, Aura>;
	type EventHandler = (CollatorSelection,);
}

parameter_types! {
	pub const ExistentialDeposit: Balance = EXISTENTIAL_DEPOSIT;
}

impl pallet_balances::Config for Runtime {
	type Balance = Balance;
	type DustRemoval = ();
	type RuntimeEvent = RuntimeEvent;
	type ExistentialDeposit = ExistentialDeposit;
	type AccountStore = System;
	type WeightInfo = weights::pallet_balances::WeightInfo<Runtime>;
	type MaxLocks = ConstU32<50>;
	type MaxReserves = ConstU32<50>;
	type ReserveIdentifier = [u8; 8];
	type RuntimeFreezeReason = RuntimeFreezeReason;
	type RuntimeHoldReason = RuntimeHoldReason;
	type FreezeIdentifier = ();
	type MaxFreezes = ConstU32<0>;
	type DoneSlashHandler = ();
}

parameter_types! {
	/// Relay Chain `TransactionByteFee` / 10.
	pub const TransactionByteFee: Balance = MILLICENTS;
}

type LengthToFee = ConstantMultiplier<Balance, TransactionByteFee>;

type OnChargeNativeTransaction =
	pallet_transaction_payment::FungibleAdapter<Balances, DealWithFees<Runtime>>;

/// Handles crediting transaction fees to the staking pot.
pub struct CreditToStakingPot;
impl pallet_asset_tx_payment::HandleCredit<AccountId, Assets> for CreditToStakingPot {
	fn handle_credit(credit: frame_support::traits::fungibles::Credit<AccountId, Assets>) {
		use sp_core::TypedGet;
		let staking_pot = pallet_collator_selection::StakingPotAccountId::<Runtime>::get();
		let _ = Assets::resolve(&staking_pot, credit);
	}
}

type OnChargeExternalAssetTransaction =
	pallet_asset_tx_payment::FungiblesAdapter<AssetRate, CreditToStakingPot>;

#[cfg(feature = "runtime-benchmarks")]
pub struct AssetTxPaymentBenchmarkHelper;
#[cfg(feature = "runtime-benchmarks")]
impl pallet_asset_tx_payment::BenchmarkHelperTrait<AccountId, Location, Location>
	for AssetTxPaymentBenchmarkHelper
{
	fn create_asset_id_parameter(id: u32) -> (Location, Location) {
		assert_eq!(id, 1); // only one asset supported in benchmarks
		let l = people::ExternalAssetLocation::get();
		(l.clone(), l)
	}
	fn setup_balances_and_pool(asset_id: Location, account: AccountId) {
		use alloc::boxed::Box;
		use frame_support::traits::{
			fungible::Mutate as _,
			fungibles::{Inspect as _, Mutate as _},
		};

		AssetRate::create(RuntimeOrigin::root(), Box::new(asset_id.clone()), 1.into()).unwrap();
		if !Assets::asset_exists(asset_id.clone()) {
			Assets::force_create(
				RuntimeOrigin::root(),
				asset_id.clone(),
				account.clone().into(),
				true,
				1,
			)
			.unwrap();
		}
		Assets::mint_into(asset_id, &account, 10_000 * UNITS).unwrap();
		Balances::mint_into(&account, 10_000 * UNITS).unwrap();
	}
}

// This extension still uses AssetRate, we may want to a change to using pools.
impl pallet_asset_tx_payment::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Fungibles = Assets;
	type OnChargeAssetTransaction = OnChargeExternalAssetTransaction;
	type WeightInfo = weights::pallet_asset_tx_payment::WeightInfo<Runtime>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = AssetTxPaymentBenchmarkHelper;
}

impl pallet_transaction_payment::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type OnChargeTransaction = OnChargeNativeTransaction;
	type OperationalFeeMultiplier = ConstU8<5>;
	type WeightToFee = WeightToFee;
	type LengthToFee = LengthToFee;
	type FeeMultiplierUpdate = SlowAdjustingFeeUpdate<Self>;
	type WeightInfo = weights::pallet_transaction_payment::WeightInfo<Runtime>;
}

impl pallet_skip_feeless_payment::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
}

impl pallet_sudo::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type WeightInfo = ();
}

parameter_types! {
	pub const ReservedXcmpWeight: Weight = MAXIMUM_BLOCK_WEIGHT.saturating_div(4);
	pub const ReservedDmpWeight: Weight = MAXIMUM_BLOCK_WEIGHT.saturating_div(4);
	pub const RelayOrigin: AggregateMessageOrigin = AggregateMessageOrigin::Parent;
}

impl cumulus_pallet_parachain_system::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type OnSystemEvent = RelayRandomness;
	type SelfParaId = parachain_info::Pallet<Runtime>;
	type OutboundXcmpMessageSource = XcmpQueue;
	type DmpQueue = frame_support::traits::EnqueueWithOrigin<MessageQueue, RelayOrigin>;
	type ReservedDmpWeight = ReservedDmpWeight;
	type XcmpMessageHandler = XcmpQueue;
	type ReservedXcmpWeight = ReservedXcmpWeight;
	type CheckAssociatedRelayNumber = RelayNumberMonotonicallyIncreases;
	type ConsensusHook = ConsensusHook;
	type WeightInfo = weights::cumulus_pallet_parachain_system::WeightInfo<Runtime>;
	type RelayParentOffset = ConstU32<RELAY_PARENT_OFFSET>;
	type SchedulingSignatureVerifier = ();
}

impl indiv_pallet_relay_randomness::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_relay_randomness::WeightInfo<Runtime>;
}

type ConsensusHook = cumulus_pallet_aura_ext::FixedVelocityConsensusHook<
	Runtime,
	RELAY_CHAIN_SLOT_DURATION_MILLIS,
	BLOCK_PROCESSING_VELOCITY,
	UNINCLUDED_SEGMENT_CAPACITY,
>;

parameter_types! {
	pub MessageQueueServiceWeight: Weight =
		Perbill::from_percent(35) * RuntimeBlockWeights::get().max_block;
}

impl pallet_message_queue::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	#[cfg(feature = "runtime-benchmarks")]
	type MessageProcessor = pallet_message_queue::mock_helpers::NoopMessageProcessor<
		cumulus_primitives_core::AggregateMessageOrigin,
	>;
	#[cfg(not(feature = "runtime-benchmarks"))]
	type MessageProcessor = xcm_builder::ProcessXcmMessage<
		AggregateMessageOrigin,
		xcm_executor::XcmExecutor<XcmConfig>,
		RuntimeCall,
	>;
	type Size = u32;
	// The XCMP queue pallet is only ever able to handle the `Sibling(ParaId)` origin:
	type QueueChangeHandler = NarrowOriginToSibling<XcmpQueue>;
	type QueuePausedQuery = NarrowOriginToSibling<XcmpQueue>;
	type HeapSize = sp_core::ConstU32<{ 103 * 1024 }>;
	type MaxStale = sp_core::ConstU32<8>;
	type ServiceWeight = MessageQueueServiceWeight;
	type IdleMaxServiceWeight = MessageQueueServiceWeight;
	type WeightInfo = weights::pallet_message_queue::WeightInfo<Runtime>;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct VerifySignatureBenchmarkHelper;
#[cfg(feature = "runtime-benchmarks")]
impl pallet_verify_signature::BenchmarkHelper<MultiSignature, AccountId>
	for VerifySignatureBenchmarkHelper
{
	fn create_signature(_entropy: &[u8], msg: &[u8]) -> (MultiSignature, AccountId) {
		use sp_io::crypto::{sr25519_generate, sr25519_sign};
		use sp_runtime::traits::IdentifyAccount;
		let public = sr25519_generate(0.into(), None);
		let who_account: AccountId = MultiSigner::Sr25519(public).into_account();
		let signature = MultiSignature::Sr25519(sr25519_sign(0.into(), &public, msg).unwrap());
		(signature, who_account)
	}
}

impl pallet_verify_signature::Config for Runtime {
	type Signature = MultiSignature;
	type AccountIdentifier = MultiSigner;
	type WeightInfo = ();
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = VerifySignatureBenchmarkHelper;
}

impl parachain_info::Config for Runtime {}

impl cumulus_pallet_aura_ext::Config for Runtime {}

parameter_types! {
	// Fellows pluralistic body.
	pub const FellowsBodyId: BodyId = BodyId::Technical;
}

/// Privileged origin that represents Root or Fellows.
pub type RootOrFellows = EitherOfDiverse<
	EnsureRoot<AccountId>,
	EnsureXcm<IsVoiceOfBody<FellowshipLocation, FellowsBodyId>>,
>;

impl cumulus_pallet_xcmp_queue::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type ChannelInfo = ParachainSystem;
	type VersionWrapper = PolkadotXcm;
	type XcmpQueue = TransformOrigin<MessageQueue, AggregateMessageOrigin, ParaId, ParaIdToSibling>;
	type MaxInboundSuspended = ConstU32<1_000>;
	type MaxActiveOutboundChannels = ConstU32<128>;
	// Most on-chain HRMP channels are configured to use 102400 bytes of max message size, so we
	// need to set the page size larger than that until we reduce the channel size on-chain.
	type MaxPageSize = ConstU32<{ 103 * 1024 }>;
	type ControllerOrigin = RootOrFellows;
	type ControllerOriginConverter = XcmOriginToTransactDispatchOrigin;
	type WeightInfo = weights::cumulus_pallet_xcmp_queue::WeightInfo<Runtime>;
	type PriceForSiblingDelivery = PriceForSiblingParachainDelivery;
}

impl cumulus_pallet_xcmp_queue::migration::v5::V5Config for Runtime {
	// This must be the same as the `ChannelInfo` from the `Config`:
	type ChannelList = ParachainSystem;
}

pub const PERIOD: u32 = 6 * HOURS;
pub const OFFSET: u32 = 0;

impl pallet_session::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type ValidatorId = <Self as frame_system::Config>::AccountId;
	// we don't have stash and controller, thus we don't need the convert as well.
	type ValidatorIdOf = pallet_collator_selection::IdentityCollator;
	type ShouldEndSession = pallet_session::PeriodicSessions<ConstU32<PERIOD>, ConstU32<OFFSET>>;
	type NextSessionRotation = pallet_session::PeriodicSessions<ConstU32<PERIOD>, ConstU32<OFFSET>>;
	type SessionManager = CollatorSelection;
	// Essentially just Aura, but let's be pedantic.
	type SessionHandler = <SessionKeys as sp_runtime::traits::OpaqueKeys>::KeyTypeIdProviders;
	type Keys = SessionKeys;
	type DisablingStrategy = ();
	type WeightInfo = weights::pallet_session::WeightInfo<Runtime>;
	type Currency = Balances;
	type KeyDeposit = ();
}

impl pallet_aura::Config for Runtime {
	type AuthorityId = AuraId;
	type DisabledValidators = ();
	type MaxAuthorities = ConstU32<100_000>;
	type AllowMultipleBlocksPerSlot = ConstBool<true>;
	type SlotDuration = ConstU64<SLOT_DURATION>;
}

parameter_types! {
	pub const PotId: PalletId = PalletId(*b"PotStake");
	pub const SessionLength: BlockNumber = 6 * HOURS;
	// StakingAdmin pluralistic body.
	pub const StakingAdminBodyId: BodyId = BodyId::Defense;
}

/// We allow Root and the `StakingAdmin` to execute privileged collator selection operations.
pub type CollatorSelectionUpdateOrigin = EitherOfDiverse<
	EnsureRoot<AccountId>,
	EnsureXcm<IsVoiceOfBody<GovernanceLocation, StakingAdminBodyId>>,
>;

impl pallet_collator_selection::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type UpdateOrigin = CollatorSelectionUpdateOrigin;
	type PotId = PotId;
	type MaxCandidates = ConstU32<100>;
	type MinEligibleCollators = ConstU32<4>;
	type MaxInvulnerables = ConstU32<20>;
	// should be a multiple of session or things will get inconsistent
	type KickThreshold = ConstU32<PERIOD>;
	type ValidatorId = <Self as frame_system::Config>::AccountId;
	type ValidatorIdOf = pallet_collator_selection::IdentityCollator;
	type ValidatorRegistration = Session;
	type WeightInfo = weights::pallet_collator_selection::WeightInfo<Runtime>;
}

parameter_types! {
	// One storage item; key size is 32; value is size 4+4+16+32 bytes = 56 bytes.
	pub const DepositBase: Balance = deposit(1, 88);
	// Additional storage item size of 32 bytes.
	pub const DepositFactor: Balance = deposit(0, 32);
}

impl pallet_multisig::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type DepositBase = DepositBase;
	type DepositFactor = DepositFactor;
	type MaxSignatories = ConstU32<100>;
	type WeightInfo = weights::pallet_multisig::WeightInfo<Runtime>;
	type BlockNumberProvider = RelaychainDataProvider<Runtime>;
}

impl pallet_utility::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type PalletsOrigin = OriginCaller;
	type WeightInfo = weights::pallet_utility::WeightInfo<Runtime>;
}

/// The type used to represent the kinds of proxying allowed.
#[derive(
	Copy,
	Clone,
	Eq,
	PartialEq,
	Ord,
	PartialOrd,
	Encode,
	Decode,
	DecodeWithMemTracking,
	Debug,
	MaxEncodedLen,
	scale_info::TypeInfo,
	Default,
)]
pub enum ProxyType {
	/// Fully permissioned proxy. Can execute any call on behalf of _proxied_.
	#[default]
	Any,
	/// Can execute any call that does not transfer funds or assets.
	NonTransfer,
	/// Proxy with the ability to reject time-delay proxy announcements.
	CancelProxy,
	/// Legacy proxy type kept to preserve stored encoding after identity pallet removal.
	Identity,
	/// Legacy proxy type kept to preserve stored encoding after identity pallet removal.
	IdentityJudgement,
	/// Collator selection proxy. Can execute calls related to collator selection mechanism.
	Collator,
}

impl InstanceFilter<RuntimeCall> for ProxyType {
	fn filter(&self, c: &RuntimeCall) -> bool {
		match self {
			ProxyType::Any => true,
			ProxyType::NonTransfer => !matches!(
				c,
				RuntimeCall::Balances { .. } |
					RuntimeCall::Assets { .. } |
					RuntimeCall::AssetConversion { .. } |
					RuntimeCall::PoolAssets { .. } |
					RuntimeCall::PolkadotXcm { .. } |
					RuntimeCall::Coinage { .. }
			),
			ProxyType::CancelProxy => matches!(
				c,
				RuntimeCall::Proxy(pallet_proxy::Call::reject_announcement { .. }) |
					RuntimeCall::Utility { .. } |
					RuntimeCall::Multisig { .. }
			),
			ProxyType::Identity => false,
			ProxyType::IdentityJudgement => false,
			ProxyType::Collator => matches!(
				c,
				RuntimeCall::CollatorSelection { .. } |
					RuntimeCall::Utility { .. } |
					RuntimeCall::Multisig { .. }
			),
		}
	}

	fn is_superset(&self, o: &Self) -> bool {
		match (self, o) {
			(x, y) if x == y => true,
			(ProxyType::Any, _) => true,
			(_, ProxyType::Any) => false,
			(ProxyType::Identity, ProxyType::IdentityJudgement) => true,
			(ProxyType::NonTransfer, ProxyType::IdentityJudgement) => true,
			(ProxyType::NonTransfer, ProxyType::Collator) => true,
			_ => false,
		}
	}
}

parameter_types! {
	// One storage item; key size 32, value size 8.
	pub const ProxyDepositBase: Balance = deposit(1, 40);
	// Additional storage item size of 33 bytes.
	pub const ProxyDepositFactor: Balance = deposit(0, 33);
	pub const MaxProxies: u16 = 32;
	// One storage item; key size 32, value size 16.
	pub const AnnouncementDepositBase: Balance = deposit(1, 48);
	pub const AnnouncementDepositFactor: Balance = deposit(0, 66);
	pub const MaxPending: u16 = 32;
}

impl pallet_proxy::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeCall = RuntimeCall;
	type Currency = Balances;
	type ProxyType = ProxyType;
	type ProxyDepositBase = ProxyDepositBase;
	type ProxyDepositFactor = ProxyDepositFactor;
	type MaxProxies = MaxProxies;
	type WeightInfo = weights::pallet_proxy::WeightInfo<Runtime>;
	type MaxPending = MaxPending;
	type CallHasher = BlakeTwo256;
	type AnnouncementDepositBase = AnnouncementDepositBase;
	type AnnouncementDepositFactor = AnnouncementDepositFactor;
	type BlockNumberProvider = RelaychainDataProvider<Runtime>;
}

parameter_types! {
	pub MbmServiceWeight: Weight = Perbill::from_percent(80) * RuntimeBlockWeights::get().max_block;
}

impl pallet_migrations::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	#[cfg(not(feature = "runtime-benchmarks"))]
	type Migrations = (
		ForeignAssetsReservesMigration<Runtime, (), migrations::PeoplePaseoAssetsReservesProvider>,
	);
	// Benchmarks need mocked migrations to guarantee that they succeed.
	#[cfg(feature = "runtime-benchmarks")]
	type Migrations = pallet_migrations::mock_helpers::MockedMigrations;
	type CursorMaxLen = ConstU32<65_536>;
	type IdentifierMaxLen = ConstU32<256>;
	type MigrationStatusHandler = ();
	type FailedMigrationHandler = frame_support::migrations::FreezeChainOnFailedMigration;
	type MaxServiceWeight = MbmServiceWeight;
	type WeightInfo = weights::pallet_migrations::WeightInfo<Runtime>;
}

impl cumulus_pallet_weight_reclaim::Config for Runtime {
	type WeightInfo = weights::cumulus_pallet_weight_reclaim::WeightInfo<Runtime>;
}

// TODO(paritytech/individuality#1124): choose good value.
const PEOPLE_IDENTITY_AND_ALIAS_ALLOWANCE_MAX: Balance = UNITS;
const PEOPLE_IDENTITY_AND_ALIAS_ALLOWANCE_RECOVERY: Balance = CENTS * 3;
const POI_CANDIDATE_RECOVERY: Balance = CENTS * 3;
const ACCOUNT_PARTICIPANT_RECOVERY: Balance = CENTS * 3;
const LITE_PERSON_AND_ALIAS_ALLOWANCE_MAX: Balance = UNITS;
const LITE_PERSON_AND_ALIAS_ALLOWANCE_RECOVERY: Balance = MILLICENTS * 3;

#[derive(
	Clone,
	Encode,
	Decode,
	Debug,
	MaxEncodedLen,
	scale_info::TypeInfo,
	Eq,
	PartialEq,
	DecodeWithMemTracking,
)]
pub enum RestrictedEntity {
	PersonalAlias(Alias),
	PersonalIdentity(u64),
	ReferredCandidate(AccountId),
	AccountParticipant(AccountId),
	InvitedCandidate(AccountId),
	LitePerson(AccountId),
	LiteAlias(Alias),
}

impl indiv_pallet_origin_restriction::RestrictedEntity<OriginCaller, Balance> for RestrictedEntity {
	fn allowance(&self) -> indiv_pallet_origin_restriction::Allowance<Balance> {
		match self {
			RestrictedEntity::PersonalAlias(_) | RestrictedEntity::PersonalIdentity(_) =>
				Allowance {
					max: PEOPLE_IDENTITY_AND_ALIAS_ALLOWANCE_MAX,
					recovery_per_block: PEOPLE_IDENTITY_AND_ALIAS_ALLOWANCE_RECOVERY,
				},
			RestrictedEntity::ReferredCandidate(_) =>
				Allowance { max: 0, recovery_per_block: POI_CANDIDATE_RECOVERY },
			RestrictedEntity::InvitedCandidate(_) =>
				Allowance { max: 0, recovery_per_block: POI_CANDIDATE_RECOVERY },
			RestrictedEntity::AccountParticipant(_) =>
				Allowance { max: 0, recovery_per_block: ACCOUNT_PARTICIPANT_RECOVERY },
			RestrictedEntity::LitePerson(_) | RestrictedEntity::LiteAlias(_) => Allowance {
				max: LITE_PERSON_AND_ALIAS_ALLOWANCE_MAX,
				recovery_per_block: LITE_PERSON_AND_ALIAS_ALLOWANCE_RECOVERY,
			},
		}
	}
	fn restricted_entity(origin_caller: &OriginCaller) -> Option<Self> {
		use indiv_pallet_people::Origin::*;
		use indiv_pallet_people_lite::Origin::*;
		use indiv_pallet_proof_of_ink::Origin::*;
		use indiv_pallet_score::Origin::*;
		use OriginCaller::*;
		match origin_caller {
			People(PersonalIdentity(id)) => Some(RestrictedEntity::PersonalIdentity(*id)),
			People(PersonalAlias(rev_ca)) => Some(RestrictedEntity::PersonalAlias(rev_ca.ca.alias)),
			ProofOfInk(ReferredCandidate(account_id)) =>
				Some(RestrictedEntity::ReferredCandidate(account_id.clone())),
			Score(AccountParticipant(account_id)) =>
				Some(RestrictedEntity::AccountParticipant(account_id.clone())),
			PeopleLite(LitePerson(account_id)) =>
				Some(RestrictedEntity::LitePerson(account_id.clone())),
			PeopleLite(LiteAlias(rev_ca)) => Some(RestrictedEntity::LiteAlias(rev_ca.ca.alias)),
			_ => None,
		}
	}
}

pub struct OperationAllowedOneTimeExcess;
impl ContainsPair<RestrictedEntity, RuntimeCall> for OperationAllowedOneTimeExcess {
	fn contains(entity: &RestrictedEntity, call: &RuntimeCall) -> bool {
		use indiv_pallet_game::Call::*;
		use indiv_pallet_proof_of_ink::Call::*;
		use indiv_pallet_score::Call::*;
		match entity {
			RestrictedEntity::ReferredCandidate(_) => {
				matches!(
					call,
					RuntimeCall::ProofOfInk(submit_evidence { .. }) |
						RuntimeCall::ProofOfInk(commit { .. }) |
						RuntimeCall::ProofOfInk(allocate_full { .. }) |
						RuntimeCall::ProofOfInk(flakeout { .. }) |
						RuntimeCall::ProofOfInk(register_referred { .. })
				)
			},
			RestrictedEntity::InvitedCandidate(_) => {
				matches!(
					call,
					RuntimeCall::ProofOfInk(submit_evidence { .. }) |
						RuntimeCall::ProofOfInk(commit { .. }) |
						RuntimeCall::ProofOfInk(allocate_full { .. }) |
						RuntimeCall::ProofOfInk(flakeout { .. }) |
						RuntimeCall::ProofOfInk(register_non_referred { .. })
				)
			},
			RestrictedEntity::AccountParticipant(_) => {
				matches!(
					call,
					RuntimeCall::Score(cash_out { .. }) |
						RuntimeCall::Score(redeem_credit { .. }) |
						RuntimeCall::Score(register { .. }) |
						RuntimeCall::Game(sign_up_with_account { .. }) |
						RuntimeCall::Game(report { .. }) |
						RuntimeCall::Game(offboard { .. }) |
						RuntimeCall::Game(claim_airdrop { .. })
				)
			},
			RestrictedEntity::PersonalAlias(_) | RestrictedEntity::PersonalIdentity(_) => false,
			RestrictedEntity::LiteAlias(_) => {
				matches!(call, RuntimeCall::Game(sign_up_with_account_lite_invite { .. }))
			},
			RestrictedEntity::LitePerson(_) => false,
		}
	}
}

#[cfg(feature = "runtime-benchmarks")]
pub struct OriginRestrictionBenchmarkHelper;
#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_origin_restriction::BenchmarkHelper<OriginCaller, RuntimeCall>
	for OriginRestrictionBenchmarkHelper
{
	fn excess_pair() -> (OriginCaller, RuntimeCall) {
		(
			OriginCaller::Score(indiv_pallet_score::Origin::AccountParticipant(
				sp_runtime::AccountId32::new([0u8; 32]),
			)),
			RuntimeCall::Score(indiv_pallet_score::Call::cash_out {}),
		)
	}
}

impl indiv_pallet_origin_restriction::Config for Runtime {
	type WeightInfo = weights::indiv_pallet_origin_restriction::WeightInfo<Runtime>;
	type BlockNumberProvider = RelaychainDataProvider<Runtime>;
	type RestrictedEntity = RestrictedEntity;
	type OperationAllowedOneTimeExcess = OperationAllowedOneTimeExcess;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = OriginRestrictionBenchmarkHelper;
}

parameter_types! {
	pub const AssetDeposit: Balance = UNITS / 10;
	pub const AssetAccountDeposit: Balance = deposit(1, 16);
	pub const AssetsStringLimit: u32 = 50;
	pub const MetadataDepositBase: Balance = deposit(1, 68);
	pub const MetadataDepositPerByte: Balance = deposit(0, 1);
}

#[cfg(feature = "runtime-benchmarks")]
pub struct AssetsBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl pallet_assets::BenchmarkHelper<xcm::latest::Location, ForeignAssetReserveData>
	for AssetsBenchmarkHelper
{
	fn create_asset_id_parameter(id: u32) -> xcm::latest::Location {
		xcm::latest::Location::new(
			1,
			[
				xcm::latest::Junction::Parachain(ASSET_HUB_ID),
				xcm::latest::Junction::PalletInstance(50),
				xcm::latest::Junction::GeneralIndex(id as u128),
			],
		)
	}

	fn create_reserve_id_parameter(id: u32) -> ForeignAssetReserveData {
		let reserve = xcm::latest::Location::new(1, [xcm::latest::Junction::Parachain(2000 + id)]);
		(reserve, false).into()
	}
}

/// Assets managed by some foreign location.
impl pallet_assets::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type AssetId = Location;
	type AssetIdParameter = Location;
	type ReserveData = ForeignAssetReserveData;
	type Currency = Balances;
	#[cfg(not(feature = "runtime-benchmarks"))]
	type CreateOrigin = AsEnsureOriginWithArg<NeverEnsureOrigin<AccountId>>;
	#[cfg(feature = "runtime-benchmarks")]
	type CreateOrigin = AsEnsureOriginWithArg<frame_system::EnsureSigned<AccountId>>;
	type ForceOrigin = EnsureRoot<AccountId>;
	type AssetDeposit = AssetDeposit;
	type MetadataDepositBase = MetadataDepositBase;
	type MetadataDepositPerByte = MetadataDepositPerByte;
	type ApprovalDeposit = ExistentialDeposit;
	type StringLimit = AssetsStringLimit;
	type Freezer = ();
	type Holder = AssetsHolder;
	type Extra = ();
	type WeightInfo = weights::pallet_assets_assets::WeightInfo<Runtime>;
	type CallbackHandle = ();
	type AssetAccountDeposit = AssetAccountDeposit;
	type RemoveItemsLimit = frame_support::traits::ConstU32<1000>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = AssetsBenchmarkHelper;
}

impl pallet_assets_holder::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type RuntimeHoldReason = RuntimeHoldReason;
}

/// The liquidity pool tokens of [`AssetConversion`].
///
/// Minted and burned by the asset conversion pallet itself, hence the origin that can create
/// them is the pallet's own account.
pub type PoolAssetsInstance = pallet_assets::Instance1;
impl pallet_assets::Config<PoolAssetsInstance> for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type RemoveItemsLimit = ConstU32<1000>;
	type AssetId = u32;
	type AssetIdParameter = u32;
	type ReserveData = ();
	type Currency = Balances;
	#[cfg(feature = "runtime-benchmarks")]
	type CreateOrigin =
		AsEnsureOriginWithArg<frame_system::EnsureSignedBy<AssetConversionOrigin, AccountId>>;
	#[cfg(not(feature = "runtime-benchmarks"))]
	type CreateOrigin = AsEnsureOriginWithArg<NeverEnsureOrigin<AccountId>>;
	type ForceOrigin = EnsureRoot<AccountId>;
	type AssetDeposit = ConstU128<0>;
	type AssetAccountDeposit = ConstU128<0>;
	type MetadataDepositBase = ConstU128<0>;
	type MetadataDepositPerByte = ConstU128<0>;
	type ApprovalDeposit = ExistentialDeposit;
	type StringLimit = AssetsStringLimit;
	type Freezer = ();
	type Holder = ();
	type Extra = ();
	type CallbackHandle = ();
	type WeightInfo = weights::pallet_assets_pool::WeightInfo<Runtime>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ();
}

pub type NativeAndAssets = fungible::UnionOf<
	Balances,
	people::AssetsWithHolder,
	TargetFromLeft<xcm_config::RelayLocation, Location>,
	Location,
	AccountId,
>;

parameter_types! {
	pub const AssetConversionPalletId: PalletId = PalletId(*b"py/ascon");
	pub const LiquidityWithdrawalFee: Permill = Permill::from_percent(0);
	/// Where the pool setup fee goes, the same destination as the transaction fees.
	pub StakingPotAccount: AccountId =
		<pallet_collator_selection::StakingPotAccountId<Runtime> as frame_support::traits::TypedGet>::get();
	pub LpFee: Permill = Permill::from_rational(3u32, 1_000u32); // 0.3%
	/// Storage deposit for the pool entry and for its liquidity token, plus the deposit an asset
	/// costs to register, so that creating a pool is no cheaper than creating the asset it pairs.
	pub const PoolSetupFee: Balance = deposit(1, 4) + AssetDeposit::get();
}

#[cfg(feature = "runtime-benchmarks")]
parameter_types! {
	/// Index of the `Assets` pallet, used to build the asset locations of benchmark pools.
	pub AssetsPalletIndex: u32 =
		<Assets as frame_support::traits::PalletInfoAccess>::index() as u32;
}

#[cfg(feature = "runtime-benchmarks")]
frame_support::ord_parameter_types! {
	pub const AssetConversionOrigin: AccountId =
		sp_runtime::traits::AccountIdConversion::<AccountId>::into_account_truncating(
			&AssetConversionPalletId::get(),
		);
}

pub type PoolIdToAccountId =
	pallet_asset_conversion::AccountIdConverter<AssetConversionPalletId, (Location, Location)>;

impl pallet_asset_conversion::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type HigherPrecisionBalance = sp_core::U256;
	type AssetKind = Location;
	type Assets = NativeAndAssets;
	type PoolId = (Self::AssetKind, Self::AssetKind);
	// Every pool is paired with the native currency, which is what the fee conversion needs and
	// what keeps a swap path down to a single hop.
	type PoolLocator = pallet_asset_conversion::WithFirstAsset<
		xcm_config::RelayLocation,
		AccountId,
		Self::AssetKind,
		PoolIdToAccountId,
	>;
	type PoolAssetId = u32;
	type PoolAssets = PoolAssets;
	type PoolSetupFee = PoolSetupFee;
	type PoolSetupFeeAsset = xcm_config::RelayLocation;
	type PoolSetupFeeTarget = ResolveAssetTo<StakingPotAccount, Self::Assets>;
	type LiquidityWithdrawalFee = LiquidityWithdrawalFee;
	type LPFee = LpFee;
	type PalletId = AssetConversionPalletId;
	// Every pool holds the native asset on one side (see `PoolLocator`), so swapping one asset for
	// another takes two hops through native. Coinage itself only ever swaps an asset for native,
	// which is a single hop.
	type MaxSwapPathLength = ConstU32<3>;
	type MintMinLiquidity = ConstU128<100>;
	type WeightInfo = weights::pallet_asset_conversion::WeightInfo<Runtime>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = assets_common::benchmarks::AssetPairFactory<
		xcm_config::RelayLocation,
		parachain_info::Pallet<Runtime>,
		AssetsPalletIndex,
		Self::AssetKind,
	>;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct AssetRateBenchmarkHelper;
#[cfg(feature = "runtime-benchmarks")]
impl pallet_asset_rate::AssetKindFactory<Location> for AssetRateBenchmarkHelper {
	fn create_asset_kind(seed: u32) -> Location {
		xcm::latest::Location::new(
			1,
			[
				xcm::latest::Junction::Parachain(ASSET_HUB_ID),
				xcm::latest::Junction::PalletInstance(50),
				xcm::latest::Junction::GeneralIndex(seed as u128),
			],
		)
	}
}

impl pallet_asset_rate::Config for Runtime {
	type WeightInfo = weights::pallet_asset_rate::WeightInfo<Runtime>;
	type RuntimeEvent = RuntimeEvent;
	type CreateOrigin = EnsureRoot<AccountId>;
	type RemoveOrigin = EnsureRoot<AccountId>;
	type UpdateOrigin = EnsureRoot<AccountId>;
	type Currency = Balances;
	type AssetKind = <Runtime as pallet_assets::Config>::AssetId;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = AssetRateBenchmarkHelper;
}

impl<LocalCall> CreateBare<LocalCall> for Runtime
where
	RuntimeCall: From<LocalCall>,
{
	fn create_bare(call: Self::RuntimeCall) -> Self::Extrinsic {
		UncheckedExtrinsic::new_bare(call)
	}
}

impl<LocalCall> CreateTransactionBase<LocalCall> for Runtime
where
	RuntimeCall: From<LocalCall>,
{
	type Extrinsic = UncheckedExtrinsic;
	type RuntimeCall = RuntimeCall;
}

impl<LocalCall> CreateTransaction<LocalCall> for Runtime
where
	RuntimeCall: From<LocalCall>,
{
	type Extension = TransactionExtension;
	fn create_transaction(
		call: <Self as frame_system::offchain::CreateTransactionBase<LocalCall>>::RuntimeCall,
		extension: Self::Extension,
	) -> Self::Extrinsic {
		UncheckedExtrinsic::new_transaction(call, extension)
	}
}

/// Mortality window, in blocks, for authorized transactions the runtime constructs and submits.
/// They are re-submitted every block by their offchain workers while the action stays due. A
/// short window lets stale submissions expire from the pool promptly.
///
/// At the 2s block time this is ~4.3 minutes.
///
/// Note: Must be a power of two for `Era::mortal` and cannot exceed `BlockHashCount`.
pub const TRANSACTION_MORTALITY_PERIOD: BlockNumber = 128;

// A mortal era whose period exceeds the number of retained block hashes can never be validated (its
// birth hash is pruned before the window closes), so guard the invariant at compile time.
const _: () = assert!(TRANSACTION_MORTALITY_PERIOD <= BlockHashCount::get());

impl<LocalCall> CreateAuthorizedTransaction<LocalCall> for Runtime
where
	RuntimeCall: From<LocalCall>,
{
	fn create_extension() -> Self::Extension {
		(
			(
				(),
				pallet_verify_signature::VerifySignature::<Runtime>::Disabled,
				indiv_pallet_people::extension::AsPerson::<Runtime>::new(None),
				indiv_pallet_proof_of_ink::extension::AsProofOfInkParticipant::<Runtime>::new(None),
				indiv_pallet_score::ScoreAsParticipant::<Runtime>::new(None),
				indiv_pallet_game::GameAsInvited::<Runtime>::new(None),
				indiv_pallet_people_lite::extension::PeopleLiteAuth::<Runtime>::new(None),
				indiv_pallet_members::extension::AsMember::<Runtime>::new(None),
				indiv_pallet_coinage::extension::AsCoinage::<Runtime>::new(None),
				indiv_pallet_resources::extension::AsResources::<Runtime>::new(None),
				indiv_pallet_honour::extension::VoterAuth::<Runtime>::new(None),
				frame_system::AuthorizeCall::<Runtime>::new(),
			),
			indiv_pallet_origin_restriction::RestrictOrigin::<Runtime>::new(false),
			frame_system::CheckNonZeroSender::<Runtime>::new(),
			frame_system::CheckSpecVersion::<Runtime>::new(),
			frame_system::CheckTxVersion::<Runtime>::new(),
			frame_system::CheckGenesis::<Runtime>::new(),
			// Anchor the mortal era at `block_number() - 1`: offchain workers build this extension
			// while executing on the current block, whose own hash is not yet in storage, so the
			// birth block must be the parent.
			frame_system::CheckEra::<Runtime>::from(generic::Era::mortal(
				u64::from(TRANSACTION_MORTALITY_PERIOD),
				u64::from(System::block_number()).saturating_sub(1),
			)),
			frame_system::CheckNonce::<Runtime>::from(0),
			frame_system::CheckWeight::<Runtime>::new(),
			pallet_skip_feeless_payment::SkipCheckIfFeeless::<
				Runtime,
				pallet_asset_tx_payment::ChargeAssetTxPayment<Runtime>,
			>::from(pallet_asset_tx_payment::ChargeAssetTxPayment::<Runtime>::from(0u128, None)),
		)
			.into()
	}
}

// Create the runtime by composing the FRAME pallets that were previously configured.
construct_runtime!(
	pub enum Runtime
	{
		// System support stuff.
		System: frame_system = 0,
		ParachainSystem: cumulus_pallet_parachain_system = 1,
		Timestamp: pallet_timestamp = 2,
		ParachainInfo: parachain_info = 3,
		WeightReclaim: cumulus_pallet_weight_reclaim = 4,
		RelayRandomness: indiv_pallet_relay_randomness = 5,
		Parameters: pallet_parameters = 73,
		NetworkSuffix: indiv_pallet_network_suffix = 74,

		// Monetary stuff.
		Balances: pallet_balances = 10,
		TransactionPayment: pallet_transaction_payment = 11,
		SkipFeelessPayment: pallet_skip_feeless_payment = 12,
		OriginRestriction: indiv_pallet_origin_restriction = 13,
		Assets: pallet_assets = 14,
		AssetsHolder: pallet_assets_holder = 15,
		AssetRate: pallet_asset_rate = 16,
		AssetTxPayment: pallet_asset_tx_payment = 17,
		AssetConversion: pallet_asset_conversion = 18,
		PoolAssets: pallet_assets::<Instance1> = 19,

		// Collator support. The order of these 5 are important and shall not change.
		Authorship: pallet_authorship = 20,
		CollatorSelection: pallet_collator_selection = 21,
		Session: pallet_session = 22,
		Aura: pallet_aura = 23,
		AuraExt: cumulus_pallet_aura_ext = 24,

		// XCM helpers.
		XcmpQueue: cumulus_pallet_xcmp_queue = 30,
		PolkadotXcm: pallet_xcm = 31,
		CumulusXcm: cumulus_pallet_xcm = 32,
		MessageQueue: pallet_message_queue = 34,

		// Handy utilities.
		Utility: pallet_utility = 40,
		Multisig: pallet_multisig = 41,
		Sudo: pallet_sudo = 42,
		VerifySignature: pallet_verify_signature = 43,
		Proxy: pallet_proxy = 44,

		// The main stage.
		// Removed: pallet identity at 50 (indiv_pallet_identity)
		People: indiv_pallet_people = 51,
		MobRule: indiv_pallet_mob_rule = 52,
		ProofOfInk: indiv_pallet_proof_of_ink = 53,
		// 54: previously used for privacy voucher
		Game: indiv_pallet_game = 55,
		Score: indiv_pallet_score = 56,
		// The game's NFT claim credits: awarded by playing, committed to per-block roots here and
		// delivered to the claims chain.
		NftCredits: indiv_pallet_nft_credits = 57,
		DummyDim: indiv_pallet_dummy_dim = 59,
		PeopleLite: indiv_pallet_people_lite = 62,
		Resources: indiv_pallet_resources = 63,
		ChunksManager: indiv_pallet_chunks_manager = 64,
		// Removed: 66,
		Members: indiv_pallet_members = 67,
		Coinage: indiv_pallet_coinage = 68,
		MembersNotifier: indiv_pallet_members_notifier = 69,
		Airdrop: indiv_pallet_airdrop = 70,
		Honour: indiv_pallet_honour = 71,
		PeopleAirdrops: indiv_pallet_people_airdrops = 72,

		// Migrations pallet
		MultiBlockMigrations: pallet_migrations = 98,
	}
);

#[cfg(feature = "runtime-benchmarks")]
mod benches {
	frame_benchmarking::define_benchmarks!(
		// Substrate
		[frame_system, SystemBench::<Runtime>]
		[pallet_balances, Balances]
		[pallet_message_queue, MessageQueue]
		[pallet_multisig, Multisig]
		[pallet_session, SessionBench::<Runtime>]
		[pallet_proxy, Proxy]
		[pallet_utility, Utility]
		[pallet_timestamp, Timestamp]
		[pallet_migrations, MultiBlockMigrations]
		[pallet_parameters, Parameters]
		[indiv_pallet_network_suffix, NetworkSuffix]
		[pallet_transaction_payment, TransactionPayment]
		[pallet_assets, Assets]
		[pallet_assets, Pool]
		[pallet_asset_conversion, AssetConversion]
		[pallet_asset_rate, AssetRate]
		[pallet_asset_tx_payment, AssetTxPayment]
		// Cumulus
		[cumulus_pallet_parachain_system, ParachainSystem]
		[cumulus_pallet_xcmp_queue, XcmpQueue]
		[cumulus_pallet_weight_reclaim, WeightReclaim]
		[pallet_collator_selection, CollatorSelection]
		// XCM
		[pallet_xcm, PalletXcmExtrinsicsBenchmark::<Runtime>]
		[pallet_xcm_benchmarks::fungible, XcmBalances]
		[pallet_xcm_benchmarks::generic, XcmGeneric]
		// POP
		[indiv_pallet_origin_restriction, OriginRestriction]
		[indiv_pallet_people, People]
		[indiv_pallet_dummy_dim, DummyDim]
		[indiv_pallet_game, Game]
		[indiv_pallet_nft_credits, NftCredits]
		[indiv_pallet_score, Score]
		[indiv_pallet_proof_of_ink, ProofOfInk]
		[indiv_pallet_mob_rule, MobRule]
		[indiv_pallet_people_lite, PeopleLite]
		[indiv_pallet_resources, Resources]
		[indiv_pallet_chunks_manager, ChunksManager]
		[indiv_pallet_members, Members]
		[indiv_pallet_members_notifier, MembersNotifier]
		[indiv_pallet_coinage, Coinage]
		[indiv_pallet_airdrop, Airdrop]
		[indiv_pallet_relay_randomness, RelayRandomness]
		[indiv_pallet_honour, Honour]
		[indiv_pallet_people_airdrops, PeopleAirdrops]
	);
}

impl_runtime_apis! {
	impl sp_consensus_aura::AuraApi<Block, AuraId> for Runtime {
		fn slot_duration() -> sp_consensus_aura::SlotDuration {
			sp_consensus_aura::SlotDuration::from_millis(SLOT_DURATION)
		}

		fn authorities() -> Vec<AuraId> {
			pallet_aura::Authorities::<Runtime>::get().into_inner()
		}
	}

	impl cumulus_primitives_core::RelayParentOffsetApi<Block> for Runtime {
		fn relay_parent_offset() -> u32 {
			RELAY_PARENT_OFFSET
		}

		fn max_claim_queue_offset() -> u8 {
			cumulus_pallet_parachain_system::Pallet::<Runtime>::max_claim_queue_offset()
		}
	}

	impl cumulus_primitives_core::SchedulingV3EnabledApi<Block> for Runtime {
		fn scheduling_v3_enabled() -> bool {
			<Runtime as cumulus_pallet_parachain_system::Config>::SchedulingSignatureVerifier::V3_SCHEDULING_ENABLED
		}
	}

	impl cumulus_primitives_aura::AuraUnincludedSegmentApi<Block> for Runtime {
		fn can_build_upon(
			included_hash: <Block as BlockT>::Hash,
			slot: cumulus_primitives_aura::Slot,
		) -> bool {
			ConsensusHook::can_build_upon(included_hash, slot)
		}
	}

	impl sp_api::Core<Block> for Runtime {
		fn version() -> RuntimeVersion {
			VERSION
		}

		fn execute_block(block: <Block as BlockT>::LazyBlock) {
			Executive::execute_block(block)
		}

		fn initialize_block(header: &<Block as BlockT>::Header) -> sp_runtime::ExtrinsicInclusionMode {
			Executive::initialize_block(header)
		}
	}

	impl sp_api::Metadata<Block> for Runtime {
		fn metadata() -> OpaqueMetadata {
			OpaqueMetadata::new(Runtime::metadata().into())
		}

		fn metadata_at_version(version: u32) -> Option<OpaqueMetadata> {
			Runtime::metadata_at_version(version)
		}

		fn metadata_versions() -> alloc::vec::Vec<u32> {
			Runtime::metadata_versions()
		}
	}

	impl frame_support::view_functions::runtime_api::RuntimeViewFunction<Block> for Runtime {
		fn execute_view_function(id: frame_support::view_functions::ViewFunctionId, input: Vec<u8>) -> Result<Vec<u8>, frame_support::view_functions::ViewFunctionDispatchError> {
			Runtime::execute_view_function(id, input)
		}
	}

	impl sp_block_builder::BlockBuilder<Block> for Runtime {
		fn apply_extrinsic(extrinsic: <Block as BlockT>::Extrinsic) -> ApplyExtrinsicResult {
			Executive::apply_extrinsic(extrinsic)
		}

		fn finalize_block() -> <Block as BlockT>::Header {
			Executive::finalize_block()
		}

		fn inherent_extrinsics(data: sp_inherents::InherentData) -> Vec<<Block as BlockT>::Extrinsic> {
			data.create_extrinsics()
		}

		fn check_inherents(
			block: <Block as BlockT>::LazyBlock,
			data: sp_inherents::InherentData,
		) -> sp_inherents::CheckInherentsResult {
			data.check_extrinsics(&block)
		}
	}

	impl sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block> for Runtime {
		fn validate_transaction(
			source: TransactionSource,
			tx: <Block as BlockT>::Extrinsic,
			block_hash: <Block as BlockT>::Hash,
		) -> TransactionValidity {
			Executive::validate_transaction(source, tx, block_hash)
		}
	}

	impl sp_offchain::OffchainWorkerApi<Block> for Runtime {
		fn offchain_worker(header: &<Block as BlockT>::Header) {
			Executive::offchain_worker(header)
		}
	}

	impl sp_session::SessionKeys<Block> for Runtime {
		fn generate_session_keys(owner: Vec<u8>, seed: Option<Vec<u8>>) -> sp_session::OpaqueGeneratedSessionKeys {
			SessionKeys::generate(&owner, seed).into()
		}

		fn decode_session_keys(
			encoded: Vec<u8>,
		) -> Option<Vec<(Vec<u8>, KeyTypeId)>> {
			SessionKeys::decode_into_raw_public_keys(&encoded)
		}
	}

	impl frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Nonce> for Runtime {
		fn account_nonce(account: AccountId) -> Nonce {
			System::account_nonce(account)
		}
	}

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance> for Runtime {
		fn query_info(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment_rpc_runtime_api::RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_info(uxt, len)
		}
		fn query_fee_details(
			uxt: <Block as BlockT>::Extrinsic,
			len: u32,
		) -> pallet_transaction_payment::FeeDetails<Balance> {
			TransactionPayment::query_fee_details(uxt, len)
		}
		fn query_weight_to_fee(weight: Weight) -> Balance {
			TransactionPayment::weight_to_fee(weight)
		}
		fn query_length_to_fee(length: u32) -> Balance {
			TransactionPayment::length_to_fee(length)
		}
	}

	impl pallet_transaction_payment_rpc_runtime_api::TransactionPaymentCallApi<Block, Balance, RuntimeCall>
		for Runtime
	{
		fn query_call_info(
			call: RuntimeCall,
			len: u32,
		) -> pallet_transaction_payment::RuntimeDispatchInfo<Balance> {
			TransactionPayment::query_call_info(call, len)
		}
		fn query_call_fee_details(
			call: RuntimeCall,
			len: u32,
		) -> pallet_transaction_payment::FeeDetails<Balance> {
			TransactionPayment::query_call_fee_details(call, len)
		}
		fn query_weight_to_fee(weight: Weight) -> Balance {
			TransactionPayment::weight_to_fee(weight)
		}
		fn query_length_to_fee(length: u32) -> Balance {
			TransactionPayment::length_to_fee(length)
		}
	}

	impl xcm_runtime_apis::fees::XcmPaymentApi<Block> for Runtime {
		fn query_acceptable_payment_assets(xcm_version: xcm::Version) -> Result<Vec<VersionedAssetId>, XcmPaymentApiError> {
			let acceptable_assets = alloc::vec![AssetId(xcm_config::AssetHubLocation::get())];
			PolkadotXcm::query_acceptable_payment_assets(xcm_version, acceptable_assets)
		}

		fn query_weight_to_asset_fee(weight: Weight, asset: VersionedAssetId) -> Result<u128, XcmPaymentApiError> {
			match asset.try_as::<AssetId>() {
				Ok(asset_id) if asset_id.0 == xcm_config::AssetHubLocation::get() => {
					// for native token
					Ok(WeightToFee::weight_to_fee(&weight))
				},
				Ok(asset_id) => {
					log::trace!(target: "xcm::xcm_runtime_apis", "query_weight_to_asset_fee - unhandled asset_id: {asset_id:?}!");
					Err(XcmPaymentApiError::AssetNotFound)
				},
				Err(_) => {
					log::trace!(target: "xcm::xcm_runtime_apis", "query_weight_to_asset_fee - failed to convert asset: {asset:?}!");
					Err(XcmPaymentApiError::VersionedConversionFailed)
				}
			}
		}

		fn query_xcm_weight(message: VersionedXcm<()>) -> Result<Weight, XcmPaymentApiError> {
			PolkadotXcm::query_xcm_weight(message)
		}

		fn query_delivery_fees(
			destination: VersionedLocation,
			message: VersionedXcm<()>,
			asset_id: VersionedAssetId,
		) -> Result<VersionedAssets, XcmPaymentApiError> {
			PolkadotXcm::query_delivery_fees::<()>(destination, message, asset_id)
		}
	}

	impl xcm_runtime_apis::dry_run::DryRunApi<Block, RuntimeCall, RuntimeEvent, OriginCaller> for Runtime {
		fn dry_run_call(origin: OriginCaller, call: RuntimeCall, result_xcms_version: XcmVersion) -> Result<CallDryRunEffects<RuntimeEvent>, XcmDryRunApiError> {
			PolkadotXcm::dry_run_call::<Runtime, xcm_config::XcmRouter, OriginCaller, RuntimeCall>(origin, call, result_xcms_version)
		}

		fn dry_run_xcm(origin_location: VersionedLocation, xcm: VersionedXcm<RuntimeCall>) -> Result<XcmDryRunEffects<RuntimeEvent>, XcmDryRunApiError> {
			PolkadotXcm::dry_run_xcm::<xcm_config::XcmRouter>(origin_location, xcm)
		}
	}

	impl xcm_runtime_apis::conversions::LocationToAccountApi<Block, AccountId> for Runtime {
		fn convert_location(location: VersionedLocation) -> Result<
			AccountId,
			xcm_runtime_apis::conversions::Error
		> {
			xcm_runtime_apis::conversions::LocationToAccountHelper::<
				AccountId,
				xcm_config::LocationToAccountId,
			>::convert_location(location)
		}
	}

	impl indiv_pallet_mob_rule::runtime_api::MobRuleApi<Block, AccountId, Balance> for Runtime {
		fn voted_on(voter: &Alias, done_only: bool) -> Vec<indiv_pallet_mob_rule::CaseIndex> {
			MobRule::voted_on(voter, done_only)
		}
	}

	impl indiv_pallet_proof_of_ink::runtime_api::ProofOfInkApi<Block, Balance> for Runtime {
		fn candidacy_deposit() -> Balance {
			use sp_runtime::traits::Convert;
			let footprint = frame_support::traits::Footprint::from_mel::<(AccountId, indiv_pallet_proof_of_ink::CandidateOf<Runtime>)>();
			frame_support::traits::LinearStoragePrice::<crate::people::ProofOfInkBaseDeposit, crate::people::ProofOfInkByteDeposit, Balance>::convert(footprint)
		}
	}

	impl indiv_pallet_game::runtime_api::PalletGameApi<Block, Balance> for Runtime {
		fn play_deposit() -> Balance {
			indiv_pallet_game::PlayDepositAmount::<Runtime>::get()
		}
	}

	impl indiv_pallet_nft_credits::runtime_api::NftCreditsApi<Block, AccountId, BlockNumber> for Runtime {
		fn nft_claim_credit_roots(
			claimant: indiv_support::identity::AccountOrPerson<AccountId>,
		) -> Vec<(BlockNumber, indiv_support::credit_trees::NftClaimCreditTree)> {
			NftCredits::nft_claim_credit_roots(&claimant)
		}

		fn nft_claim_credit_proofs(
			award_block: BlockNumber,
			claimant: indiv_support::identity::AccountOrPerson<AccountId>,
		) -> Result<Vec<indiv_pallet_nft_credits::NftClaimCreditProof>, indiv_pallet_nft_credits::NftClaimCreditProofError> {
			NftCredits::nft_claim_credit_proofs(award_block, &claimant)
		}

		fn nft_claim_credit_proof_from_awards(
			award_block: BlockNumber,
			awards: Vec<indiv_pallet_nft_credits::NftClaimCreditAward<AccountId>>,
			leaf_index: u32,
		) -> Result<indiv_pallet_nft_credits::NftClaimCreditProof, indiv_pallet_nft_credits::NftClaimCreditProofError> {
			NftCredits::nft_claim_credit_proof_from_awards(award_block, awards, leaf_index)
		}
	}

	impl cumulus_primitives_core::CollectCollationInfo<Block> for Runtime {
		fn collect_collation_info(header: &<Block as BlockT>::Header) -> cumulus_primitives_core::CollationInfo {
			ParachainSystem::collect_collation_info(header)
		}
	}

	#[cfg(feature = "try-runtime")]
	impl frame_try_runtime::TryRuntime<Block> for Runtime {
		fn on_runtime_upgrade(checks: frame_try_runtime::UpgradeCheckSelect) -> (Weight, Weight) {
			let weight = Executive::try_runtime_upgrade(checks).unwrap();
			(weight, RuntimeBlockWeights::get().max_block)
		}

		fn execute_block(
			block: <Block as BlockT>::LazyBlock,
			state_root_check: bool,
			signature_check: bool,
			select: frame_try_runtime::TryStateSelect,
		) -> Weight {
			// NOTE: intentional unwrap: we don't want to propagate the error backwards, and want to
			// have a backtrace here.
			Executive::try_execute_block(block, state_root_check, signature_check, select).unwrap()
		}
	}

	#[cfg(feature = "runtime-benchmarks")]
	impl frame_benchmarking::Benchmark<Block> for Runtime {
		fn benchmark_metadata(extra: bool) -> (
			Vec<frame_benchmarking::BenchmarkList>,
			Vec<frame_support::traits::StorageInfo>,
		) {
			use frame_benchmarking::{BenchmarkList};
			use frame_support::traits::StorageInfoTrait;
			use frame_system_benchmarking::Pallet as SystemBench;
			use cumulus_pallet_session_benchmarking::Pallet as SessionBench;
			use pallet_xcm::benchmarking::Pallet as PalletXcmExtrinsicsBenchmark;

			// This is defined once again in dispatch_benchmark, because list_benchmarks!
			// and add_benchmarks! are macros exported by define_benchmarks! macros and those types
			// are referenced in that call.
			type XcmBalances = pallet_xcm_benchmarks::fungible::Pallet::<Runtime>;
			type XcmGeneric = pallet_xcm_benchmarks::generic::Pallet::<Runtime>;
			// The liquidity-token instance of `pallet_assets`, benchmarked separately from the
			// assets instance because its asset id and storage layout differ.
			type Pool = pallet_assets::Pallet::<Runtime, PoolAssetsInstance>;

			let mut list = Vec::<BenchmarkList>::new();
			list_benchmarks!(list, extra);

			let storage_info = AllPalletsWithSystem::storage_info();
			(list, storage_info)
		}

		fn dispatch_benchmark(
			config: frame_benchmarking::BenchmarkConfig
		) -> Result<Vec<frame_benchmarking::BenchmarkBatch>, alloc::string::String> {
			use frame_benchmarking::{BenchmarkBatch, BenchmarkError};
			use sp_storage::TrackedStorageKey;

			use frame_system_benchmarking::Pallet as SystemBench;
			impl frame_system_benchmarking::Config for Runtime {
				fn setup_set_code_requirements(code: &alloc::vec::Vec<u8>) -> Result<(), BenchmarkError> {
					ParachainSystem::initialize_for_set_code_benchmark(code.len() as u32);
					Ok(())
				}

				fn verify_set_code() {
					System::assert_last_event(cumulus_pallet_parachain_system::Event::<Runtime>::ValidationFunctionStored.into());
				}
			}

			use cumulus_pallet_session_benchmarking::Pallet as SessionBench;
			impl cumulus_pallet_session_benchmarking::Config for Runtime {
				fn generate_session_keys_and_proof(owner: Self::AccountId) -> (Self::Keys, Vec<u8>) {
					use codec::Encode;
					let keys = SessionKeys::generate(&owner.encode(), None);
					(keys.keys, keys.proof.encode())
				}
			}
			impl pallet_transaction_payment::BenchmarkConfig for Runtime {}
			use xcm_config::RelayLocation;
			// Paseo uses the same parachain IDs as other system chains
			use xcm_config::AssetHubLocation;
			use cumulus_primitives_core::ParaId;
			parameter_types! {
				pub AssetHubParaId: ParaId = ParaId::from(1000u32);
			}

			use xcm::latest::prelude::{Assets as XcmAssets, *};
			// the pallet Assets and Assets in xcm::latest::prelude::* are conflicting.
			// Explicitly importing the pallet fix this issue.
			use crate::Assets;

			parameter_types! {
				pub ExistentialDepositAsset: Option<Asset> = Some((
					RelayLocation::get(),
					ExistentialDeposit::get()
				).into());
			}

			use pallet_xcm::benchmarking::Pallet as PalletXcmExtrinsicsBenchmark;
			impl pallet_xcm::benchmarking::Config for Runtime {
				type DeliveryHelper = polkadot_runtime_common::xcm_sender::ToParachainDeliveryHelper<
						xcm_config::XcmConfig,
						ExistentialDepositAsset,
						PriceForSiblingParachainDelivery,
						AssetHubParaId,
						ParachainSystem,
					>;

				fn reachable_dest() -> Option<Location> {
					Some(AssetHubLocation::get())
				}

				fn teleportable_asset_and_dest() -> Option<(Asset, Location)> {
					// Relay/native token can be teleported between People and Relay.
					Some((
						Asset {
							fun: Fungible(ExistentialDeposit::get()),
							id: AssetId(RelayLocation::get())
						},
						AssetHubLocation::get(),
					))
				}

				fn reserve_transferable_asset_and_dest() -> Option<(Asset, Location)> {
					None
				}

				fn set_up_complex_asset_transfer() -> Option<(XcmAssets, u32, Location, alloc::boxed::Box<dyn FnOnce()>)> {
					let native_location = Parent.into();
					let dest = AssetHubLocation::get();

					pallet_xcm::benchmarking::helpers::native_teleport_as_asset_transfer::<Runtime>(
						native_location,
						dest,
					)
				}

				fn get_asset() -> Asset {
					Asset {
						id: AssetId(RelayLocation::get()),
						fun: Fungible(ExistentialDeposit::get()),
					}
				}
			}

			impl pallet_xcm_benchmarks::Config for Runtime {
				type XcmConfig = XcmConfig;
				type AccountIdConverter = xcm_config::LocationToAccountId;
				type DeliveryHelper = polkadot_runtime_common::xcm_sender::ToParachainDeliveryHelper<
						xcm_config::XcmConfig,
						ExistentialDepositAsset,
						PriceForSiblingParachainDelivery,
						AssetHubParaId,
						ParachainSystem,
					>;
				fn valid_destination() -> Result<Location, BenchmarkError> {
					Ok(AssetHubLocation::get())
				}
				fn worst_case_holding(_depositable_count: u32) -> xcm_executor::AssetsInHolding {
					use pallet_xcm_benchmarks::MockCredit;
					// just concrete assets according to relay chain.
					let mut holding = xcm_executor::AssetsInHolding::new();
					holding.fungible.insert(
						AssetId(RelayLocation::get()),
						alloc::boxed::Box::new(MockCredit(1_000_000 * UNITS)),
					);
					holding
				}
			}

			parameter_types! {
				// External assets from Asset Hub are teleport-only (see `ExternalAssetFromAssetHub`
				// in xcm_config.rs); benchmark `receive_teleported_asset` against that path.
				pub TrustedTeleporter: Option<(Location, Asset)> = Some((
					AssetHubLocation::get(),
					Asset { fun: Fungible(UNITS), id: AssetId(crate::people::ExternalAssetLocation::get()) },
				));
				pub const CheckedAccount: Option<(AccountId, xcm_builder::MintLocation)> = None;
				// Reserve example: any AH-issued trust-backed asset other than the external asset, so the
				// `AssetHubReserveAsset` filter accepts it and `reserve_asset_deposited` runs.
				pub TrustedReserve: Option<(Location, Asset)> = Some((
					AssetHubLocation::get(),
					Asset {
						fun: Fungible(UNITS),
						id: AssetId(Location::new(1, [
							Parachain(ASSET_HUB_ID),
							PalletInstance(50),
							GeneralIndex((crate::people::EXTERNAL_ASSET_ID + 1) as u128),
						])),
					},
				));
			}

			impl pallet_xcm_benchmarks::fungible::Config for Runtime {
				type TransactAsset = Balances;

				type CheckedAccount = CheckedAccount;
				type TrustedTeleporter = TrustedTeleporter;
				type TrustedReserve = TrustedReserve;

				fn get_asset() -> Asset {
					Asset {
						id: AssetId(RelayLocation::get()),
						fun: Fungible(UNITS),
					}
				}
			}

			impl pallet_xcm_benchmarks::generic::Config for Runtime {
				type RuntimeCall = RuntimeCall;
				type TransactAsset = Balances;

				fn worst_case_response() -> (u64, Response) {
					(0u64, Response::Version(Default::default()))
				}

				fn worst_case_asset_exchange() -> Result<(XcmAssets, XcmAssets), BenchmarkError> {
					Err(BenchmarkError::Skip)
				}

				fn universal_alias() -> Result<(Location, Junction), BenchmarkError> {
					Err(BenchmarkError::Skip)
				}

				fn transact_origin_and_runtime_call() -> Result<(Location, RuntimeCall), BenchmarkError> {
					Ok((AssetHubLocation::get(), frame_system::Call::remark_with_event { remark: alloc::vec![] }.into()))
				}

				fn subscribe_origin() -> Result<Location, BenchmarkError> {
					Ok(AssetHubLocation::get())
				}

				fn claimable_asset() -> Result<(Location, Location, XcmAssets), BenchmarkError> {
					let origin = AssetHubLocation::get();
					let assets: XcmAssets = (AssetId(RelayLocation::get()), 1_000 * UNITS).into();
					let ticket = Location::new(0, []);
					Ok((origin, ticket, assets))
				}

				fn worst_case_for_trader() -> Result<(Asset, WeightLimit), BenchmarkError> {
					Ok((Asset {
						id: AssetId(RelayLocation::get()),
						fun: Fungible(1_000_000 * UNITS),
					}, WeightLimit::Limited(Weight::from_parts(5000, 5000))))
				}

				fn unlockable_asset() -> Result<(Location, Location, Asset), BenchmarkError> {
					Err(BenchmarkError::Skip)
				}

				fn export_message_origin_and_destination(
				) -> Result<(Location, NetworkId, InteriorLocation), BenchmarkError> {
					Err(BenchmarkError::Skip)
				}

				fn alias_origin() -> Result<(Location, Location), BenchmarkError> {
					Err(BenchmarkError::Skip)
				}
			}

			type XcmBalances = pallet_xcm_benchmarks::fungible::Pallet::<Runtime>;
			type XcmGeneric = pallet_xcm_benchmarks::generic::Pallet::<Runtime>;
			// The liquidity-token instance of `pallet_assets`, benchmarked separately from the
			// assets instance because its asset id and storage layout differ.
			type Pool = pallet_assets::Pallet::<Runtime, PoolAssetsInstance>;

			use frame_support::traits::WhitelistedStorageKeys;
			let whitelist: Vec<TrackedStorageKey> = AllPalletsWithSystem::whitelisted_storage_keys();

			let mut batches = Vec::<BenchmarkBatch>::new();
			let params = (&config, &whitelist);
			add_benchmarks!(params, batches);

			Ok(batches)
		}
	}

	impl sp_genesis_builder::GenesisBuilder<Block> for Runtime {
		fn build_state(config: Vec<u8>) -> sp_genesis_builder::Result {
			build_state::<RuntimeGenesisConfig>(config)
		}

		fn get_preset(id: &Option<sp_genesis_builder::PresetId>) -> Option<Vec<u8>> {
			get_preset::<RuntimeGenesisConfig>(id, &genesis_config_presets::get_preset)
		}

		fn preset_names() -> Vec<sp_genesis_builder::PresetId> {
			genesis_config_presets::preset_names()
		}
	}

}

cumulus_pallet_parachain_system::register_validate_block! {
	Runtime = Runtime,
	BlockExecutor = cumulus_pallet_aura_ext::BlockExecutor::<Runtime, Executive>,
}
