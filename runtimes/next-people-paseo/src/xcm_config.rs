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
	AccountId, AllPalletsWithSystem, AssetConversion, Balances, ParachainInfo, ParachainSystem,
	PolkadotXcm, Runtime, RuntimeCall, RuntimeEvent, RuntimeOrigin, WeightToFee, XcmpQueue,
};
use crate::{
	people::{AssetsWithHolder, ExternalAssetLocation},
	AssetRate, Assets as AssetsPallet, Balance, NativeAndAssets, TransactionByteFee, CENTS,
};
use assets_common::matching::RemoteAssetFromLocation;
use core::marker::PhantomData;
use cumulus_primitives_utility::{ChargeWeightInFungibles, TakeFirstAssetTrader};
use frame_support::{
	parameter_types,
	traits::{
		tokens::{
			imbalance::{ResolveAssetTo, ResolveTo},
			ConversionFromAssetBalance, ConversionToAssetBalance,
		},
		ConstU32, Contains, ContainsPair, Disabled, Equals, Everything, EverythingBut, Nothing,
		ProcessMessageError,
	},
};
use frame_system::EnsureRoot;
use pallet_collator_selection::StakingPotAccountId;
use pallet_xcm::XcmPassthrough;
use parachains_common::{
	xcm_config::{
		AllSiblingSystemParachains, ConcreteAssetFromSystem, ParentRelayOrSiblingParachains,
		RelayOrOtherSystemParachains,
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
	SingleAssetExchangeAdapter, SovereignSignedViaLocation, StartsWith,
	StartsWithExplicitGlobalConsensus, TakeWeightCredit, TrailingSetTopicAsId, UsingComponents,
	WeightInfoBounds, WithComputedOrigin, WithLatestLocationConverter, WithUniqueTopic,
	XcmFeeManagerFromComponents,
};
use xcm_executor::{
	traits::{AssetExchange, Properties, ShouldExecute, WeightTrader},
	AssetsInHolding, XcmExecutor,
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
	AssetsPallet,
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

/// Reserve transfers this chain accepts: assets *native to Asset Hub*, the ones its trust backed
/// `Assets` and pool instances issue, sent from Asset Hub. The external asset is left out because
/// it is teleport-only here.
///
/// The rule is deliberately restricted to Asset Hub's own assets rather than everything Asset Hub
/// happens to custody. Trusting a chain as the reserve for an asset it did not issue gives that
/// asset two reserves, and `ReserveAssetDeposited` *mints* locally, so the second reserve can
/// credit this chain with holdings the real reserve is not backing. A blanket rule would let Asset
/// Hub mint PAS here, since `FungibleTransactor` has no checking account.
///
/// Broad reserve trust is safe to pair with a narrow registry: `pallet-assets` here has
/// `CreateOrigin = NeverEnsureOrigin`, so an incoming asset is only ever credited if root already
/// registered it locally.
pub struct TrustedReserves;
impl ContainsPair<Asset, Location> for TrustedReserves {
	fn contains(asset: &Asset, origin: &Location) -> bool {
		// The external asset arrives by teleport, so it must not gain a reserve here as well.
		if asset.id.0 == ExternalAssetLocation::get() {
			return false;
		}
		RemoteAssetFromLocation::<StartsWith<AssetHubLocation>, AssetHubLocation>::contains(
			asset, origin,
		)
	}
}

pub type WeightToNativeFee = WeightToFee;

/// Matches PAS on top of everything [`AssetsConvertedConcreteId`] matches.
///
/// `AssetsConvertedConcreteId` deliberately excludes the relay chain's own token, but the assets
/// the executor asks *for*, delivery fees, are priced in PAS, so the exchanger has to recognise it.
pub type NativeAndForeignAssetsMatcher = (
	AssetsConvertedConcreteId,
	MatchedConvertedConcreteId<
		Location,
		Balance,
		Equals<RelayLocation>,
		WithLatestLocationConverter<Location>,
		TryConvertInto,
	>,
);

/// Prices weight in any asset that governance registered a rate for in `pallet-asset-rate`.
///
/// The weight is first priced in PAS, then converted to the asset at the registered rate. Assets
/// without a rate are rejected, which makes the trader fall through to the next component.
pub struct WeightToAssetRateFee;
impl ChargeWeightInFungibles<AccountId, AssetsWithHolder> for WeightToAssetRateFee {
	fn charge_weight_in_fungibles(asset_id: Location, weight: Weight) -> Result<Balance, XcmError> {
		let native_fee =
			<WeightToNativeFee as frame_support::weights::WeightToFee>::weight_to_fee(&weight);

		AssetRate::to_asset_balance(native_fee, asset_id).map_err(|_| XcmError::AssetNotFound)
	}
}

/// The fungible amount of `asset`, or [`Balance::MAX`] if it is not fungible, so that a
/// nonsensical quote never wins a price comparison.
fn fungible_amount(asset: &Asset) -> Balance {
	match asset.fun {
		Fungible(amount) => amount,
		_ => Balance::MAX,
	}
}

/// A [`WeightTrader`] that charges through whichever of `A` and `B` asks the payer for less.
///
/// The plain tuple `WeightTrader` takes the first component that succeeds, which would make the
/// price a payer gets depend on declaration order: a pool too thin to price a fee sanely would win
/// simply for being listed first. Here both halves quote in the asset the payer offered, so the
/// quotes are directly comparable, and the cheaper one charges. If it cannot settle, the other is
/// tried. On a tie `A` wins.
pub struct CheaperTrader<A, B> {
	first: A,
	second: B,
	/// Which half bought the weight, so a refund goes back through the same one.
	used: Option<bool>,
}

impl<A: WeightTrader, B: WeightTrader> CheaperTrader<A, B> {
	/// Whether `A` is the half to charge through for `given`.
	fn first_is_cheaper(&mut self, weight: Weight, given: AssetId, context: &XcmContext) -> bool {
		let a = self.first.quote_weight(weight, given.clone(), context).ok();
		let b = self.second.quote_weight(weight, given, context).ok();
		match (a, b) {
			(Some(a), Some(b)) => fungible_amount(&a) <= fungible_amount(&b),
			(Some(_), None) => true,
			(None, Some(_)) => false,
			// Neither can price it; the order they refuse in does not matter.
			(None, None) => true,
		}
	}
}

impl<A: WeightTrader, B: WeightTrader> WeightTrader for CheaperTrader<A, B> {
	fn new() -> Self {
		Self { first: A::new(), second: B::new(), used: None }
	}

	fn buy_weight(
		&mut self,
		weight: Weight,
		payment: AssetsInHolding,
		context: &XcmContext,
	) -> Result<AssetsInHolding, (AssetsInHolding, XcmError)> {
		// Once a half has bought weight, stay with it: `refund_weight` can only give back what
		// that half is holding.
		if let Some(used) = self.used {
			return if used {
				self.first.buy_weight(weight, payment, context)
			} else {
				self.second.buy_weight(weight, payment, context)
			};
		}

		let first_is_cheaper = match payment.fungible.first_key_value() {
			Some((given, _)) => self.first_is_cheaper(weight, given.clone(), context),
			// Nothing to price; let the first half produce the error.
			None => true,
		};

		let payment = if first_is_cheaper {
			match self.first.buy_weight(weight, payment, context) {
				Ok(unspent) => {
					self.used = Some(true);
					return Ok(unspent);
				},
				Err((payment, _)) => payment,
			}
		} else {
			match self.second.buy_weight(weight, payment, context) {
				Ok(unspent) => {
					self.used = Some(false);
					return Ok(unspent);
				},
				Err((payment, _)) => payment,
			}
		};

		// The cheaper half could not settle the fee after all, so try the other one.
		let bought = if first_is_cheaper {
			self.second.buy_weight(weight, payment, context)
		} else {
			self.first.buy_weight(weight, payment, context)
		};
		if bought.is_ok() {
			self.used = Some(!first_is_cheaper);
		}
		bought
	}

	fn refund_weight(&mut self, weight: Weight, context: &XcmContext) -> Option<AssetsInHolding> {
		match self.used {
			Some(true) => self.first.refund_weight(weight, context),
			Some(false) => self.second.refund_weight(weight, context),
			None => None,
		}
	}

	fn quote_weight(
		&mut self,
		weight: Weight,
		given: AssetId,
		context: &XcmContext,
	) -> Result<Asset, XcmError> {
		let a = self.first.quote_weight(weight, given.clone(), context);
		let b = self.second.quote_weight(weight, given, context);
		match (a, b) {
			(Ok(a), Ok(b)) => Ok(if fungible_amount(&a) <= fungible_amount(&b) { a } else { b }),
			(Ok(a), Err(_)) => Ok(a),
			(Err(_), Ok(b)) => Ok(b),
			(Err(error), Err(_)) => Err(error),
		}
	}
}

/// Buys XCM execution weight with any asset that has an [`AssetConversion`] pool against PAS, by
/// swapping exactly enough of it for the PAS the weight costs. The PAS lands in the staking pot.
pub type PoolTrader = cumulus_primitives_utility::SwapFirstAssetTrader<
	RelayLocation,
	AssetConversion,
	WeightToNativeFee,
	NativeAndAssets,
	AssetsConvertedConcreteId,
	ResolveAssetTo<StakingPotAccountId<Runtime>, NativeAndAssets>,
	AccountId,
>;

/// Buys XCM execution weight with any asset governance registered a rate for, taking it in kind
/// at that rate.
pub type AssetRateTrader = TakeFirstAssetTrader<
	AccountId,
	WeightToAssetRateFee,
	AssetsConvertedConcreteId,
	AssetsWithHolder,
	ResolveAssetTo<StakingPotAccountId<Runtime>, AssetsWithHolder>,
>;

/// All ways of paying for XCM execution fees: PAS itself, or whichever of the [`AssetConversion`]
/// pool and the governance-registered rate asks the payer for less.
pub type Traders = (
	UsingComponents<
		WeightToNativeFee,
		RelayLocation,
		AccountId,
		Balances,
		ResolveTo<StakingPotAccountId<Runtime>, Balances>,
	>,
	CheaperTrader<PoolTrader, AssetRateTrader>,
);

/// Swaps the asset offered for fees against the asset the executor prices them in, PAS, through an
/// [`AssetConversion`] pool.
///
/// This is what lets delivery fees, which the routers always quote in PAS, be paid in another
/// asset: the executor asks this exchanger for PAS, and it sells just enough of the offered asset
/// to get it.
pub type PoolAssetsExchanger = SingleAssetExchangeAdapter<
	AssetConversion,
	NativeAndAssets,
	NativeAndForeignAssetsMatcher,
	AccountId,
>;

/// The single fungible asset of `assets`, if that is all it holds.
fn single_fungible(assets: &Assets) -> Option<(Location, Balance)> {
	match assets.inner().as_slice() {
		[Asset { id: AssetId(location), fun: Fungible(amount) }] =>
			Some((location.clone(), *amount)),
		_ => None,
	}
}

/// An [`AssetExchange`] that settles through whichever of `A` and `B` asks the payer for less.
///
/// The plain tuple `AssetExchange` takes the first component that answers, which would let a pool
/// too thin to price a fee sanely win over a governance rate simply for being listed first.
///
/// Both `maximal` modes are compared the same way, on the smaller quote. That is correct here
/// because this exchanger only ever prices fees: the executor asks for PAS and the payer parts
/// with the other asset, so in both directions the quote is denominated in what the payer gives
/// up. The `ExchangeAsset` instruction, where `maximal` would mean "get me as much as possible",
/// is not weighed on this chain and so is unreachable.
pub struct CheaperExchanger<A, B>(PhantomData<(A, B)>);

impl<A: AssetExchange, B: AssetExchange> CheaperExchanger<A, B> {
	fn quoted_amount<E: AssetExchange>(
		give: &Assets,
		want: &Assets,
		maximal: bool,
	) -> Option<Balance> {
		E::quote_exchange_price(give, want, maximal)
			.and_then(|quote| single_fungible(&quote))
			.map(|(_, amount)| amount)
	}

	/// Whether `A` is the half to settle through.
	fn first_is_cheaper(give: &Assets, want: &Assets, maximal: bool) -> bool {
		match (
			Self::quoted_amount::<A>(give, want, maximal),
			Self::quoted_amount::<B>(give, want, maximal),
		) {
			(Some(a), Some(b)) => a <= b,
			(Some(_), None) => true,
			(None, Some(_)) => false,
			// Neither can price it; the order they refuse in does not matter.
			(None, None) => true,
		}
	}
}

impl<A: AssetExchange, B: AssetExchange> AssetExchange for CheaperExchanger<A, B> {
	fn exchange_asset(
		origin: Option<&Location>,
		give: AssetsInHolding,
		want: &Assets,
		maximal: bool,
	) -> Result<AssetsInHolding, AssetsInHolding> {
		// `give` was sized by a prior `quote_exchange_price`, so settle through the half that
		// produced that quote. Falling back to the other keeps a half that quoted but then could
		// not settle from failing the whole payment.
		let give_view = match give.fungible.first_key_value() {
			Some((AssetId(location), accounting))
				if give.fungible.len() == 1 && give.non_fungible.is_empty() =>
				Some((location.clone(), accounting.amount()).into()),
			_ => None,
		};
		let first_is_cheaper = give_view
			.map(|view: Assets| Self::first_is_cheaper(&view, want, maximal))
			.unwrap_or(true);

		let give = if first_is_cheaper {
			match A::exchange_asset(origin, give, want, maximal) {
				Ok(got) => return Ok(got),
				Err(give) => give,
			}
		} else {
			match B::exchange_asset(origin, give, want, maximal) {
				Ok(got) => return Ok(got),
				Err(give) => give,
			}
		};
		if first_is_cheaper {
			B::exchange_asset(origin, give, want, maximal)
		} else {
			A::exchange_asset(origin, give, want, maximal)
		}
	}

	fn quote_exchange_price(give: &Assets, want: &Assets, maximal: bool) -> Option<Assets> {
		let a = A::quote_exchange_price(give, want, maximal);
		let b = B::quote_exchange_price(give, want, maximal);
		match (a, b) {
			(Some(a), Some(b)) => {
				let cheaper = match (single_fungible(&a), single_fungible(&b)) {
					(Some((_, x)), Some((_, y))) => x <= y,
					(Some(_), None) => true,
					_ => false,
				};
				Some(if cheaper { a } else { b })
			},
			(Some(a), None) => Some(a),
			(None, b) => b,
		}
	}
}

/// Lets fees that the executor prices in PAS, delivery fees, be settled in any asset that
/// governance registered a rate for in `pallet-asset-rate`. The fallback behind
/// [`PoolAssetsExchanger`], for assets that have a rate but no pool.
///
/// No swap happens, since there is no pool to swap against: the asset offered for fees is priced
/// against the PAS amount the executor asks for using the registered rate, and, if it covers it,
/// is handed straight back so that the `FeeManager` deposits it *in kind* into the fee receiver's
/// account. This is the same deal the [`Traders`] above offer for execution fees.
///
/// The `ExchangeAsset` instruction is not weighed on this chain (its weight is `Weight::MAX`), so
/// this is only ever reachable through fee payment in the XCM executor.
pub struct FeesAtAssetRate;

impl FeesAtAssetRate {
	/// What `amount` of `from` is worth in `to`, at the rates registered in `pallet-asset-rate`.
	///
	/// Only pairs including PAS are priced, which is all fees ever need: rates are registered
	/// against PAS.
	fn convert(amount: Balance, from: &Location, to: &Location) -> Option<Balance> {
		let native = RelayLocation::get();
		match (from == &native, to == &native) {
			(true, true) => Some(amount),
			(true, false) => AssetRate::to_asset_balance(amount, to.clone()).ok(),
			(false, true) => AssetRate::from_asset_balance(amount, from.clone()).ok(),
			(false, false) => None,
		}
	}
}

impl AssetExchange for FeesAtAssetRate {
	fn exchange_asset(
		_origin: Option<&Location>,
		give: AssetsInHolding,
		want: &Assets,
		_maximal: bool,
	) -> Result<AssetsInHolding, AssetsInHolding> {
		// Only the assets set aside for fee payment, a single fungible, are ever offered here.
		let given = match give.fungible.iter().next() {
			Some((AssetId(location), accounting))
				if give.fungible.len() == 1 && give.non_fungible.is_empty() =>
				Some((location.clone(), accounting.amount())),
			_ => None,
		};
		let (Some((given_asset, given_amount)), Some((wanted_asset, wanted_amount))) =
			(given, single_fungible(want))
		else {
			return Err(give);
		};

		match Self::convert(wanted_amount, &wanted_asset, &given_asset) {
			// What is offered is worth what was asked for, so it settles the fee as it is.
			Some(required) if required <= given_amount => Ok(give),
			_ => Err(give),
		}
	}

	fn quote_exchange_price(give: &Assets, want: &Assets, maximal: bool) -> Option<Assets> {
		let (given_asset, given_amount) = single_fungible(give)?;
		let (wanted_asset, wanted_amount) = single_fungible(want)?;
		if maximal {
			// How much of `want`'s asset is `give` worth?
			let obtained = Self::convert(given_amount, &given_asset, &wanted_asset)?;
			Some((wanted_asset, obtained).into())
		} else {
			// How much of `give`'s asset does it take to cover `want`?
			let required = Self::convert(wanted_amount, &wanted_asset, &given_asset)?;
			Some((given_asset, required).into())
		}
	}
}

/// All ways of settling a fee the executor priced in PAS, delivery fees, in another asset:
/// whichever of the [`AssetConversion`] pool and the governance-registered rate asks the payer for
/// less.
pub type AssetExchangers = CheaperExchanger<PoolAssetsExchanger, FeesAtAssetRate>;

pub struct XcmConfig;
impl xcm_executor::Config for XcmConfig {
	type XcmEventEmitter = PolkadotXcm;
	type RuntimeCall = RuntimeCall;
	type XcmSender = XcmRouter;
	type AssetTransactor = AssetTransactors;
	type OriginConverter = XcmOriginToTransactDispatchOrigin;
	type IsReserve = TrustedReserves;
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
	// Delivery fees are priced in PAS but can be settled in any asset with a pool, or failing
	// that a registered rate.
	type AssetExchanger = AssetExchangers;
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
