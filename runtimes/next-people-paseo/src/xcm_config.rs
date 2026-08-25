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

extern crate alloc;

use super::{
	AccountId, AllPalletsWithSystem, Balances, ParachainInfo, ParachainSystem, PolkadotXcm,
	Runtime, RuntimeCall, RuntimeEvent, RuntimeOrigin, WeightToFee, XcmpQueue,
};
use crate::{people::ExternalAssetLocation, AssetRate, Assets, Balance, TransactionByteFee, CENTS};
use cumulus_primitives_utility::TakeFirstAssetTrader;
use frame_support::{
	parameter_types,
	traits::{
		tokens::imbalance::{ResolveAssetTo, ResolveTo},
		ConstU32, Contains, ContainsPair, Disabled, Equals, Everything, EverythingBut, Nothing,
		ProcessMessageError,
	},
};
use frame_system::EnsureRoot;
use pallet_collator_selection::StakingPotAccountId;
use pallet_xcm::XcmPassthrough;
use parachains_common::{
	xcm_config::{
		AllSiblingSystemParachains, AssetFeeAsExistentialDepositMultiplier,
		ConcreteAssetFromSystem, ParentRelayOrSiblingParachains, RelayOrOtherSystemParachains,
	},
	TREASURY_PALLET_ID,
};
use paseo_runtime_constants::system_parachain::{ASSET_HUB_ID, COLLECTIVES_ID, NEXT_ASSET_HUB_ID};
use polkadot_parachain_primitives::primitives::Sibling;
use sp_runtime::traits::{AccountIdConversion, TryConvertInto};
use xcm::latest::prelude::*;
use xcm_builder::{
	AccountId32Aliases, AllowExplicitUnpaidExecutionFrom, AllowHrmpNotificationsFromRelayChain,
	AllowKnownQueryResponses, AllowSubscriptionsFrom, AllowTopLevelPaidExecutionFrom,
	DenyRecursively, DenyReserveTransferToRelayChain, DenyThenTry, DescribeTerminus,
	EnsureXcmOrigin, FrameTransactionalProcessor, FungibleAdapter, FungiblesAdapter,
	HashedDescription, IsConcrete, LocationAsSuperuser, MatchedConvertedConcreteId, NoChecking,
	ParentIsPreset, RelayChainAsNative, SendXcmFeeToAccount, SiblingParachainAsNative,
	SiblingParachainConvertsVia, SignedAccountId32AsNative, SignedToAccountId32,
	SovereignSignedViaLocation, StartsWith, StartsWithExplicitGlobalConsensus, TakeWeightCredit,
	TrailingSetTopicAsId, UsingComponents, WeightInfoBounds, WithComputedOrigin,
	WithLatestLocationConverter, WithUniqueTopic, XcmFeeManagerFromComponents,
};
use xcm_executor::{
	traits::{Properties, ShouldExecute},
	XcmExecutor,
};

parameter_types! {
	pub const RootLocation: Location = Location::here();
	pub const RelayLocation: Location = Location::parent();
	pub const RelayNetwork: Option<NetworkId> = Some(NetworkId::Polkadot);
	pub RelayChainOrigin: RuntimeOrigin = cumulus_pallet_xcm::Origin::Relay.into();
	pub UniversalLocation: InteriorLocation =
		[GlobalConsensus(RelayNetwork::get().unwrap()), Parachain(ParachainInfo::parachain_id().into())].into();
	pub const MaxInstructions: u32 = 100;
	pub const MaxAssetsIntoHolding: u32 = 64;
	pub const GovernanceLocation: Location = Location::parent();
	pub FellowshipLocation: Location = Location::new(1, Parachain(COLLECTIVES_ID));
	/// The asset ID for the asset that we use to pay for message delivery fees. Just PAS.
	pub FeeAssetId: AssetId = AssetId(RelayLocation::get());
	/// The base fee for the message delivery fees.
	pub const BaseDeliveryFee: u128 = CENTS.saturating_mul(3);
	pub TreasuryAccount: AccountId = TREASURY_PALLET_ID.into_account_truncating();
	pub RelayTreasuryLocation: Location =
		(Parent, PalletInstance(crate::paseo_constants::TREASURY_PALLET_ID)).into();
	pub LocalLocationPattern: Location = Location::new(0, Here);
	pub ParentLocation: Location = Location::parent();
	pub UniversalLocationNetworkId: NetworkId = UniversalLocation::get().global_consensus().unwrap();
	pub CheckingAccount: AccountId = PolkadotXcm::check_account();
	pub AssetHubLocation: Location = Location::new(1, [Parachain(ASSET_HUB_ID)]);
	pub NextAhLocation: Location = Location::new(1, [Parachain(NEXT_ASSET_HUB_ID)]);
}

pub type PriceForParentDelivery = polkadot_runtime_common::xcm_sender::ExponentialPrice<
	FeeAssetId,
	BaseDeliveryFee,
	TransactionByteFee,
	ParachainSystem,
>;

pub type PriceForSiblingParachainDelivery = polkadot_runtime_common::xcm_sender::ExponentialPrice<
	FeeAssetId,
	BaseDeliveryFee,
	TransactionByteFee,
	XcmpQueue,
>;

/// Type for specifying how a `Location` can be converted into an `AccountId`. This is used
/// when determining ownership of accounts for asset transacting and when attempting to use XCM
/// `Transact` in order to determine the dispatch Origin.
pub type LocationToAccountId = (
	// The parent (Relay-chain) origin converts to the parent `AccountId`.
	ParentIsPreset<AccountId>,
	// Sibling parachain origins convert to AccountId via the `ParaId::into`.
	SiblingParachainConvertsVia<Sibling, AccountId>,
	// Straight up local `AccountId32` origins just alias directly to `AccountId`.
	AccountId32Aliases<RelayNetwork, AccountId>,
	// Here/local root location to `AccountId`.
	HashedDescription<AccountId, DescribeTerminus>,
);

/// Means for transacting the native currency on this chain.
pub type FungibleTransactor = FungibleAdapter<
	// Use this currency:
	Balances,
	// Use this currency when it is a fungible asset matching the given location or name:
	IsConcrete<RelayLocation>,
	// Do a simple punn to convert an `AccountId32` `Location` into a native chain
	// `AccountId`:
	LocationToAccountId,
	// Our chain's account ID type (we can't get away without mentioning it explicitly):
	AccountId,
	// We don't track any teleports of `Balances`.
	(),
>;

/// `AssetId`/`Balance` converter for `Assets`
pub type AssetsConvertedConcreteId = MatchedConvertedConcreteId<
	Location,
	Balance,
	EverythingBut<(
		// Excludes relay/parent chain currency
		Equals<ParentLocation>,
		// Here we rely on fact that something like this works:
		// assert!(Location::new(1,
		// [Parachain(100)]).starts_with(&Location::parent()));
		// assert!([Parachain(100)].into().starts_with(&Here));
		StartsWith<LocalLocationPattern>,
		// Ignore assets that start explicitly with our `GlobalConsensus(NetworkId)`, means:
		// - foreign assets from our consensus should be: `Location {parents: 1, X*(Parachain(xyz),
		//   ..)}`
		// - foreign assets outside our consensus with the same `GlobalConsensus(NetworkId)` won't
		//   be accepted here
		StartsWithExplicitGlobalConsensus<UniversalLocationNetworkId>,
	)>,
	WithLatestLocationConverter<Location>,
	TryConvertInto,
>;

/// Means for transacting foreign assets from different global consensus.
pub type ForeignFungiblesTransactor = FungiblesAdapter<
	// Use this fungibles implementation:
	Assets,
	// Use this currency when it is a fungible asset matching the given location or name:
	AssetsConvertedConcreteId,
	// Convert an XCM `Location` into a local account ID:
	LocationToAccountId,
	// Our chain's account ID type (we can't get away without mentioning it explicitly):
	AccountId,
	// We dont need to check teleports here.
	NoChecking,
	// The account to use for tracking teleports.
	CheckingAccount,
>;

/// Means for transacting assets on this chain.
pub type AssetTransactors = (FungibleTransactor, ForeignFungiblesTransactor);

/// This is the type we use to convert an (incoming) XCM origin into a local `Origin` instance,
/// ready for dispatching a transaction with XCM's `Transact`.
///
/// There is an `OriginKind` that can bias the kind of local `Origin` it will become.
pub type XcmOriginToTransactDispatchOrigin = (
	// Sovereign account converter; this attempts to derive an `AccountId` from the origin location
	// using `LocationToAccountId` and then turn that into the usual `Signed` origin. Useful for
	// foreign chains who want to have a local sovereign account on this chain that they control.
	SovereignSignedViaLocation<LocationToAccountId, RuntimeOrigin>,
	// Native converter for Relay-chain (Parent) location; will convert to a `Relay` origin when
	// recognized.
	RelayChainAsNative<RelayChainOrigin, RuntimeOrigin>,
	// Native converter for sibling Parachains; will convert to a `SiblingPara` origin when
	// recognized.
	SiblingParachainAsNative<cumulus_pallet_xcm::Origin, RuntimeOrigin>,
	// Superuser converter for the AH-next location. This will allow it to issue a transaction from
	// the Root origin.
	LocationAsSuperuser<Equals<NextAhLocation>, RuntimeOrigin>,
	// Native signed account converter; this just converts an `AccountId32` origin into a normal
	// `RuntimeOrigin::Signed` origin of the same 32-byte value.
	SignedAccountId32AsNative<RelayNetwork, RuntimeOrigin>,
	// XCM origins can be represented natively under the XCM pallet's `Xcm` origin.
	XcmPassthrough<RuntimeOrigin>,
);

pub struct LocalPlurality;
impl Contains<Location> for LocalPlurality {
	fn contains(location: &Location) -> bool {
		matches!(location.unpack(), (0, [Plurality { .. }]))
	}
}

pub struct ParentOrParentsPlurality;
impl Contains<Location> for ParentOrParentsPlurality {
	fn contains(location: &Location) -> bool {
		matches!(location.unpack(), (1, []) | (1, [Plurality { .. }]))
	}
}

/// Custom barrier for Asset Hub - allows any execution from Asset Hub
pub struct AllowAssetHubExecution;
impl ShouldExecute for AllowAssetHubExecution {
	fn should_execute<RuntimeCall>(
		origin: &Location,
		_instructions: &mut [Instruction<RuntimeCall>],
		_max_weight: Weight,
		_properties: &mut Properties,
	) -> Result<(), ProcessMessageError> {
		if origin == &AssetHubLocation::get() {
			log::trace!(target: "xcm::barriers", "AllowAssetHubExecution: Asset Hub origin allowed");
			Ok(())
		} else {
			Err(ProcessMessageError::Unsupported)
		}
	}
}

pub type Barrier = TrailingSetTopicAsId<
	DenyThenTry<
		DenyRecursively<DenyReserveTransferToRelayChain>,
		(
			// Allow local users to buy weight credit.
			TakeWeightCredit,
			// Expected responses are OK.
			AllowKnownQueryResponses<PolkadotXcm>,
			AllowAssetHubExecution,
			WithComputedOrigin<
				(
					// If the message is one that immediately attempts to pay for execution, then
					// allow it.
					AllowTopLevelPaidExecutionFrom<Everything>,
					// Parent and its pluralities (i.e. governance bodies) get free execution.
					AllowExplicitUnpaidExecutionFrom<ParentOrParentsPlurality>,
					// Subscriptions for version tracking are OK.
					AllowSubscriptionsFrom<ParentRelayOrSiblingParachains>,
					// HRMP notifications from the relay chain are OK.
					AllowHrmpNotificationsFromRelayChain,
				),
				UniversalLocation,
				ConstU32<8>,
			>,
		),
	>,
>;

/// Locations that will not be charged fees in the executor, neither for execution nor delivery. We
/// only waive fees for system functions, which these locations represent.
pub type WaivedLocations = (
	RelayOrOtherSystemParachains<AllSiblingSystemParachains, Runtime>,
	Equals<RelayTreasuryLocation>,
	Equals<RootLocation>,
	LocalPlurality,
);

/// Accept an external asset as a teleport only when it comes from Asset Hub.
pub struct ExternalAssetFromAssetHub;
impl ContainsPair<Asset, Location> for ExternalAssetFromAssetHub {
	fn contains(asset: &Asset, origin: &Location) -> bool {
		origin == &AssetHubLocation::get() && asset.id.0 == ExternalAssetLocation::get()
	}
}

/// Cases where a remote origin is accepted as trusted Teleporter for a given asset:
/// - PAS with the parent Relay Chain and sibling parachains; and
/// - an external asset from Asset Hub.
pub type TrustedTeleporters = (ConcreteAssetFromSystem<RelayLocation>, ExternalAssetFromAssetHub);

/// Custom reserve filter for Asset Hub assets coming from Asset Hub perspective.
/// Accepts trust-backed assets from Asset Hub except an external asset,
/// which is teleport-only.
pub struct AssetHubReserveAsset;
impl ContainsPair<Asset, Location> for AssetHubReserveAsset {
	fn contains(asset: &Asset, origin: &Location) -> bool {
		if origin != &AssetHubLocation::get() {
			return false;
		}
		// External assets are teleport-only.
		if asset.id.0 == ExternalAssetLocation::get() {
			return false;
		}
		matches!(
			asset.id.0.unpack(),
			(1, [Parachain(ASSET_HUB_ID), PalletInstance(50), GeneralIndex(_)])
		)
	}
}

pub type WeightToNativeFee = WeightToFee;

/// Prices weight in any asset that governance registered a rate for in `pallet-asset-rate`.
///
/// The weight is first priced in PAS, then converted to the asset at the registered rate. Assets
/// without a rate are rejected, which makes the trader fall through to the next component.
pub type WeightToAssetRateFee =
	AssetFeeAsExistentialDepositMultiplier<Runtime, WeightToNativeFee, AssetRate, ()>;

/// Buys XCM execution weight with any asset governance registered a rate for, taking it in kind
/// at that rate.
///
/// The fee is deposited *in that asset* into the staking pot, and a deposit that fails is burned
/// rather than refunded. `pallet-assets` refuses to open an account for an asset that is not
/// `is_sufficient` in an account with no provider reference, and refuses any deposit below the
/// asset's `min_balance`. So a rated asset must be registered `is_sufficient = true` with a
/// `min_balance` no larger than the smallest fee, or the staking pot must be given the asset, or
/// the existential deposit in PAS, before the rate is registered.
pub type AssetRateTrader = TakeFirstAssetTrader<
	AccountId,
	WeightToAssetRateFee,
	AssetsConvertedConcreteId,
	Assets,
	ResolveAssetTo<StakingPotAccountId<Runtime>, Assets>,
>;

/// All ways of paying for execution fees via XCM: PAS, or any asset with a rate registered in
/// `pallet-asset-rate` (the external asset being the first such asset).
pub type Traders = (
	UsingComponents<
		WeightToNativeFee,
		RelayLocation,
		AccountId,
		Balances,
		ResolveTo<StakingPotAccountId<Runtime>, Balances>,
	>,
	AssetRateTrader,
);

pub struct XcmConfig;
impl xcm_executor::Config for XcmConfig {
	type XcmEventEmitter = PolkadotXcm;
	type RuntimeCall = RuntimeCall;
	type XcmSender = XcmRouter;
	type AssetTransactor = AssetTransactors;
	type OriginConverter = XcmOriginToTransactDispatchOrigin;
	type IsReserve = AssetHubReserveAsset;
	/// Allow teleportation of PAS and of external assets from Asset Hub.
	type IsTeleporter = TrustedTeleporters;
	type UniversalLocation = UniversalLocation;
	type Barrier = Barrier;
	type Weigher = WeightInfoBounds<
		crate::weights::xcm::PeoplePaseoXcmWeight<RuntimeCall>,
		RuntimeCall,
		MaxInstructions,
	>;
	type Trader = Traders;
	type ResponseHandler = PolkadotXcm;
	type AssetTrap = PolkadotXcm;
	type SubscriptionService = PolkadotXcm;
	type PalletInstancesInfo = AllPalletsWithSystem;
	type MaxAssetsIntoHolding = MaxAssetsIntoHolding;
	type AssetLocker = ();
	type AssetExchanger = ();
	type FeeManager = XcmFeeManagerFromComponents<
		WaivedLocations,
		SendXcmFeeToAccount<Self::AssetTransactor, TreasuryAccount>,
	>;
	type MessageExporter = ();
	type UniversalAliases = Nothing;
	type CallDispatcher = RuntimeCall;
	type SafeCallFilter = Everything;
	type Aliasers = Nothing;
	type TransactionalProcessor = FrameTransactionalProcessor;
	type HrmpNewChannelOpenRequestHandler = ();
	type HrmpChannelAcceptedHandler = ();
	type HrmpChannelClosingHandler = ();
	type XcmRecorder = PolkadotXcm;
}

/// Converts a local signed origin into an XCM location. Forms the basis for local origins
/// sending/executing XCMs.
pub type LocalOriginToLocation = SignedToAccountId32<RuntimeOrigin, AccountId, RelayNetwork>;

/// The means for routing XCM messages which are not for local execution into the right message
/// queues.
pub type XcmRouter = WithUniqueTopic<(
	// Two routers - use UMP to communicate with the relay chain:
	cumulus_primitives_utility::ParentAsUmp<ParachainSystem, PolkadotXcm, ()>,
	// ..and XCMP to communicate with the sibling chains.
	XcmpQueue,
)>;

impl pallet_xcm::Config for Runtime {
	type AuthorizedAliasConsideration = Disabled;
	type RuntimeEvent = RuntimeEvent;
	// We want to disallow users sending (arbitrary) XCM programs from this chain.
	type SendXcmOrigin = EnsureXcmOrigin<RuntimeOrigin, ()>;
	type XcmRouter = XcmRouter;
	// We support local origins dispatching XCM executions.
	type ExecuteXcmOrigin = EnsureXcmOrigin<RuntimeOrigin, LocalOriginToLocation>;
	type XcmExecuteFilter = Everything;
	type XcmExecutor = XcmExecutor<XcmConfig>;
	type XcmTeleportFilter = Everything;
	type XcmReserveTransferFilter = Nothing; // This parachain is not meant as a reserve location.
	type Weigher = WeightInfoBounds<
		crate::weights::xcm::PeoplePaseoXcmWeight<RuntimeCall>,
		RuntimeCall,
		MaxInstructions,
	>;
	type UniversalLocation = UniversalLocation;
	type RuntimeOrigin = RuntimeOrigin;
	type RuntimeCall = RuntimeCall;
	const VERSION_DISCOVERY_QUEUE_SIZE: u32 = 100;
	type AdvertisedXcmVersion = pallet_xcm::CurrentXcmVersion;
	type Currency = Balances;
	type CurrencyMatcher = ();
	type TrustedLockers = ();
	type SovereignAccountOf = LocationToAccountId;
	type MaxLockers = ConstU32<8>;
	type WeightInfo = crate::weights::pallet_xcm::WeightInfo<Runtime>;
	type AdminOrigin = EnsureRoot<AccountId>;
	type MaxRemoteLockConsumers = ConstU32<0>;
	type RemoteLockConsumerIdentifier = ();
}

impl cumulus_pallet_xcm::Config for Runtime {
	type RuntimeEvent = RuntimeEvent;
	type XcmExecutor = XcmExecutor<XcmConfig>;
}
