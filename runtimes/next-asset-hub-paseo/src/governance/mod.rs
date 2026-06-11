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

//! Governance configurations for the Asset Hub runtime.

use super::*;
use crate::{
	treasury::{AssetRateWithNative, TreasuryPalletId},
	xcm_config::FellowshipLocation,
};
use frame_support::traits::fungible::HoldConsideration;
use frame_system::EnsureRootWithSuccess;
use pallet_xcm::{EnsureXcm, IsVoiceOfBody};
use parachains_common::pay::{AccountIdToLocalLocation, LocalPay, VersionedLocatableAccount};
use polkadot_runtime_common::impls::VersionedLocatableAsset;
use sp_runtime::traits::IdentityLookup;
use xcm::latest::BodyId;

mod origins;
pub use origins::{
	pallet_custom_origins, AuctionAdmin, FellowshipAdmin, GeneralAdmin, LeaseAdmin,
	ReferendumCanceller, ReferendumKiller, Spender, StakingAdmin, Treasurer, WhitelistedCaller,
};
mod tracks;
pub use tracks::TracksInfo;

parameter_types! {
	pub const VoteLockingPeriod: BlockNumber = prod_or_fast!(7 * RC_DAYS, 1);
}

impl pallet_conviction_voting::Config for Runtime {
	type WeightInfo = weights::pallet_conviction_voting::WeightInfo<Self>;
	type RuntimeEvent = RuntimeEvent;
	type Currency = Balances;
	type VoteLockingPeriod = VoteLockingPeriod;
	type MaxVotes = ConstU32<512>;
	type MaxTurnout =
		frame_support::traits::tokens::currency::ActiveIssuanceOf<Balances, Self::AccountId>;
	type Polls = Referenda;
	type BlockNumberProvider = RelaychainDataProvider<Runtime>;
	type VotingHooks = ();
}

parameter_types! {
	pub const AlarmInterval: BlockNumber = 1;
	pub const SubmissionDeposit: Balance = 10 * DOLLARS;
	pub const UndecidingTimeout: BlockNumber = 3 * RC_DAYS;
}

parameter_types! {
	pub const MaxBalance: Balance = Balance::MAX;
}
pub type TreasurySpender = EitherOf<EnsureRootWithSuccess<AccountId, MaxBalance>, Spender>;

impl origins::pallet_custom_origins::Config for Runtime {}

parameter_types! {
	// Fellows pluralistic body.
	pub const FellowsBodyId: BodyId = BodyId::Technical;
}

impl pallet_whitelist::Config for Runtime {
	type WeightInfo = weights::pallet_whitelist::WeightInfo<Self>;
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type WhitelistOrigin = EitherOfDiverse<
		EnsureRoot<Self::AccountId>,
		EnsureXcm<IsVoiceOfBody<FellowshipLocation, FellowsBodyId>>,
	>;
	type DispatchWhitelistedOrigin = EitherOf<EnsureRoot<Self::AccountId>, WhitelistedCaller>;
	type Preimages = Preimage;
}

impl pallet_referenda::Config for Runtime {
	type WeightInfo = weights::pallet_referenda::WeightInfo<Self>;
	type RuntimeCall = RuntimeCall;
	type RuntimeEvent = RuntimeEvent;
	type Scheduler = Scheduler;
	type Currency = Balances;
	type SubmitOrigin = frame_system::EnsureSigned<AccountId>;
	type CancelOrigin = EitherOf<EnsureRoot<AccountId>, ReferendumCanceller>;
	type KillOrigin = EitherOf<EnsureRoot<AccountId>, ReferendumKiller>;
	type Slash = Treasury;
	type Votes = pallet_conviction_voting::VotesOf<Runtime>;
	type Tally = pallet_conviction_voting::TallyOf<Runtime>;
	type SubmissionDeposit = SubmissionDeposit;
	type MaxQueued = ConstU32<100>;
	type UndecidingTimeout = UndecidingTimeout;
	type AlarmInterval = AlarmInterval;
	type Tracks = TracksInfo;
	type Preimages = Preimage;
	type BlockNumberProvider = RelaychainDataProvider<Runtime>;
}

parameter_types! {
	pub const MultiAssetBountyValueMinimum: Balance = 200 * CENTS;
	pub const MultiAssetChildBountyValueMinimum: Balance = MultiAssetBountyValueMinimum::get() / 10;
	pub const MultiAssetMaxActiveChildBountyCount: u32 = 100;
	pub const MultiAssetCuratorHoldReason: RuntimeHoldReason =
		RuntimeHoldReason::MultiAssetBounties(pallet_multi_asset_bounties::HoldReason::CuratorDeposit);
	pub const MultiAssetCuratorDepositFromValueMultiplier: Permill = Permill::from_percent(10);
	pub const MultiAssetCuratorDepositMin: Balance = 10 * CENTS;
	pub const MultiAssetCuratorDepositMax: Balance = 500 * CENTS;
}

impl pallet_multi_asset_bounties::Config for Runtime {
	type Balance = Balance;
	type RejectOrigin = EitherOfDiverse<EnsureRoot<AccountId>, Treasurer>;
	type SpendOrigin = TreasurySpender;
	type AssetKind = VersionedLocatableAsset;
	type Beneficiary = VersionedLocatableAccount;
	type BeneficiaryLookup = IdentityLookup<Self::Beneficiary>;
	type BountyValueMinimum = MultiAssetBountyValueMinimum;
	type ChildBountyValueMinimum = MultiAssetChildBountyValueMinimum;
	type MaxActiveChildBountyCount = MultiAssetMaxActiveChildBountyCount;
	type WeightInfo = pallet_multi_asset_bounties::weights::SubstrateWeight<Runtime>;
	type FundingSource = pallet_multi_asset_bounties::PalletIdAsFundingSource<
		TreasuryPalletId,
		Runtime,
		AccountIdToLocalLocation,
	>;
	type BountySource = pallet_multi_asset_bounties::BountySourceFromPalletId<
		TreasuryPalletId,
		pallet_multi_asset_bounties::BountyAccountPrefix,
		Runtime,
		AccountIdToLocalLocation,
	>;
	type ChildBountySource = pallet_multi_asset_bounties::ChildBountySourceFromPalletId<
		TreasuryPalletId,
		pallet_multi_asset_bounties::ChildBountyAccountPrefix,
		Runtime,
		AccountIdToLocalLocation,
	>;
	type Paymaster = LocalPay<NativeAndAssets, AccountId, xcm_config::LocationToAccountId>;
	type BalanceConverter = AssetRateWithNative;
	type Preimages = Preimage;
	type Consideration = HoldConsideration<
		AccountId,
		Balances,
		MultiAssetCuratorHoldReason,
		pallet_multi_asset_bounties::CuratorDepositAmount<
			MultiAssetCuratorDepositFromValueMultiplier,
			MultiAssetCuratorDepositMin,
			MultiAssetCuratorDepositMax,
			Balance,
		>,
		Balance,
	>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = parachains_common::pay::benchmarks::LocalPayWithSourceArguments<
		xcm_config::TrustBackedAssetsPalletIndex,
	>;
}
