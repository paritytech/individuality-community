// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0
//
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

use core::cell::RefCell;
use frame_support::{
	derive_impl, parameter_types,
	traits::{AsEnsureOriginWithArg, ConstU128, ConstU32, ConstU8},
};
use frame_system::{EnsureRoot, EnsureSigned};
use sp_core::crypto::AccountId32;
use sp_runtime::{
	traits::{AccountIdLookup, ConvertInto},
	BuildStorage,
};
use xcm::latest::{
	Asset as XcmAsset, Assets as XcmAssets, Error as XcmError, InstructionError, InteriorLocation,
	Junction, Location, NetworkId, Outcome, SendError, SendResult, SendXcm, Weight as XcmWeight,
	Xcm, XcmHash,
};
use xcm_builder::{DescribeAllTerminal, DescribeFamily, HashedDescription, SignedToAccountId32};
use xcm_executor::traits::{FeeManager, FeeReason};

pub type Block = frame_system::mocking::MockBlock<Test>;
pub type AccountId = AccountId32;
pub type Balance = u128;

pub const ALICE: AccountId = AccountId32::new([1u8; 32]);
pub const BOB: AccountId = AccountId32::new([2u8; 32]);

/// Instance mirroring the destination chain's `pallet-assets`, used only to decode the calls
/// this pallet sends against the real `pallet-assets` call encoding.
pub type DestAssetsInstance = pallet_assets::Instance2;

frame_support::construct_runtime!(
	pub enum Test {
		System: frame_system,
		Balances: pallet_balances,
		Assets: pallet_assets = 50,
		DestAssets: pallet_assets::<Instance2> = 14,
		Forwarder: crate = 37,
	}
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type AccountId = AccountId;
	type Lookup = AccountIdLookup<AccountId, ()>;
	type Block = Block;
	type AccountData = pallet_balances::AccountData<Balance>;
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type Balance = Balance;
	type AccountStore = System;
	type ExistentialDeposit = ConstU128<1>;
}

impl pallet_assets::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type AssetId = u32;
	type AssetIdParameter = codec::Compact<u32>;
	type Currency = Balances;
	type CreateOrigin = AsEnsureOriginWithArg<EnsureSigned<AccountId>>;
	type ForceOrigin = EnsureRoot<AccountId>;
	type AssetDeposit = ConstU128<1>;
	type AssetAccountDeposit = ConstU128<1>;
	type MetadataDepositBase = ConstU128<1>;
	type MetadataDepositPerByte = ConstU128<1>;
	type ApprovalDeposit = ConstU128<1>;
	type StringLimit = ConstU32<50>;
	type Freezer = ();
	type Holder = ();
	type Extra = ();
	type CallbackHandle = ();
	type WeightInfo = ();
	type RemoveItemsLimit = ConstU32<5>;
	type ReserveData = ();
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ();
}

impl pallet_assets::Config<DestAssetsInstance> for Test {
	type RuntimeEvent = RuntimeEvent;
	type Balance = Balance;
	type AssetId = Location;
	type AssetIdParameter = Location;
	type Currency = Balances;
	type CreateOrigin = AsEnsureOriginWithArg<EnsureSigned<AccountId>>;
	type ForceOrigin = EnsureRoot<AccountId>;
	type AssetDeposit = ConstU128<1>;
	type AssetAccountDeposit = ConstU128<1>;
	type MetadataDepositBase = ConstU128<1>;
	type MetadataDepositPerByte = ConstU128<1>;
	type ApprovalDeposit = ConstU128<1>;
	type StringLimit = ConstU32<50>;
	type Freezer = ();
	type Holder = ();
	type Extra = ();
	type CallbackHandle = ();
	type WeightInfo = ();
	type RemoveItemsLimit = ConstU32<5>;
	type ReserveData = ();
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = LocationBenchmarkHelper;
}

#[cfg(feature = "runtime-benchmarks")]
pub struct LocationBenchmarkHelper;
#[cfg(feature = "runtime-benchmarks")]
impl pallet_assets::BenchmarkHelper<Location, ()> for LocationBenchmarkHelper {
	fn create_asset_id_parameter(id: u32) -> Location {
		Location::new(1, [Junction::Parachain(id)])
	}

	fn create_reserve_id_parameter(_id: u32) {}
}

thread_local! {
	static SENT_XCM: RefCell<Vec<(Location, Xcm<()>)>> = const { RefCell::new(Vec::new()) };
	static CHARGED_FEES: RefCell<Vec<(Location, XcmAssets)>> = const { RefCell::new(Vec::new()) };
}

/// Returns every message delivered through [`MockXcmSender`].
pub fn sent_xcm() -> Vec<(Location, Xcm<()>)> {
	SENT_XCM.with(|q| q.borrow().clone())
}

/// Returns every fee charged through [`MockXcmExecutor`].
pub fn charged_fees() -> Vec<(Location, XcmAssets)> {
	CHARGED_FEES.with(|q| q.borrow().clone())
}

parameter_types! {
	pub const DeliveryFeeAmount: u128 = 1_000_000;
	pub storage SendFails: bool = false;
	pub storage FeeChargeFails: bool = false;
	pub storage FeesWaived: bool = false;
}

pub struct MockXcmSender;
impl SendXcm for MockXcmSender {
	type Ticket = (Location, Xcm<()>);

	fn validate(
		destination: &mut Option<Location>,
		message: &mut Option<Xcm<()>>,
	) -> SendResult<Self::Ticket> {
		if SendFails::get() {
			return Err(SendError::Transport("mock send failure"));
		}
		let ticket = (
			destination.take().expect("destination is set"),
			message.take().expect("message is set"),
		);
		let price: XcmAssets =
			XcmAsset::from((Location::parent(), DeliveryFeeAmount::get())).into();
		Ok((ticket, price))
	}

	fn deliver(ticket: Self::Ticket) -> Result<XcmHash, SendError> {
		let hash = sp_io::hashing::blake2_256(&codec::Encode::encode(&ticket.1));
		SENT_XCM.with(|q| q.borrow_mut().push(ticket));
		Ok(hash)
	}
}

pub struct MockXcmExecutor;
impl xcm::latest::ExecuteXcm<RuntimeCall> for MockXcmExecutor {
	type Prepared = MockPrepared;

	fn prepare(
		message: Xcm<RuntimeCall>,
		_weight_limit: XcmWeight,
	) -> Result<Self::Prepared, InstructionError> {
		let _ = message;
		Ok(MockPrepared)
	}

	fn execute(
		_origin: impl Into<Location>,
		_pre: Self::Prepared,
		_id: &mut XcmHash,
		_weight_credit: XcmWeight,
	) -> Outcome {
		Outcome::Complete { used: XcmWeight::zero() }
	}

	fn charge_fees(location: impl Into<Location>, fees: XcmAssets) -> Result<(), XcmError> {
		if FeeChargeFails::get() {
			return Err(XcmError::FeesNotMet);
		}
		CHARGED_FEES.with(|q| q.borrow_mut().push((location.into(), fees)));
		Ok(())
	}
}

pub struct MockPrepared;
impl xcm::latest::PreparedMessage for MockPrepared {
	fn weight_of(&self) -> XcmWeight {
		XcmWeight::zero()
	}
}

impl FeeManager for MockXcmExecutor {
	fn is_waived(_origin: Option<&Location>, _reason: FeeReason) -> bool {
		FeesWaived::get()
	}

	fn handle_fee(
		_fees: xcm_executor::AssetsInHolding,
		_context: Option<&xcm::latest::XcmContext>,
		_reason: FeeReason,
	) {
	}
}

parameter_types! {
	pub Destination: Location = Location::new(1, [Junction::Parachain(1502)]);
	pub AssetsPalletLocation: Location = Location::new(0, [Junction::PalletInstance(50)]);
	pub UniversalLocation: InteriorLocation =
		[Junction::GlobalConsensus(NetworkId::Polkadot), Junction::Parachain(1500)].into();
	pub RelayNetwork: Option<NetworkId> = Some(NetworkId::Polkadot);
	pub const ForwardDeposit: Balance = 100;
}

impl crate::Config for Test {
	type RuntimeHoldReason = RuntimeHoldReason;
	type Currency = Balances;
	type ForwardDeposit = ForwardDeposit;
	type ManagerOrigin = EnsureRoot<AccountId>;
	type Destination = Destination;
	type RemoteAssetsPalletIndex = ConstU8<14>;
	type AssetsPalletLocation = AssetsPalletLocation;
	type UniversalLocation = UniversalLocation;
	type DestinationAccountOf = HashedDescription<AccountId, DescribeFamily<DescribeAllTerminal>>;
	type AssetIdToIndex = ConvertInto;
	type OriginToLocation = SignedToAccountId32<RuntimeOrigin, AccountId, RelayNetwork>;
	type XcmSender = MockXcmSender;
	type XcmExecutor = MockXcmExecutor;
	type WeightInfo = ();
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = ();
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let mut storage = frame_system::GenesisConfig::<Test>::default().build_storage().unwrap();
	pallet_balances::GenesisConfig::<Test> {
		balances: vec![(ALICE, 1_000_000_000), (BOB, 1_000_000_000)],
		..Default::default()
	}
	.assimilate_storage(&mut storage)
	.unwrap();
	let mut ext: sp_io::TestExternalities = storage.into();
	ext.execute_with(|| {
		System::set_block_number(1);
		SENT_XCM.with(|q| q.borrow_mut().clear());
		CHARGED_FEES.with(|q| q.borrow_mut().clear());
	});
	ext
}
