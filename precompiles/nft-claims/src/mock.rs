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

pub use super::*;

use frame_support::{
	derive_impl, parameter_types,
	traits::{
		fungible::HoldConsideration, ConstU32, ConstU64, ConstU8, Currency, LinearStoragePrice,
		UnixTime,
	},
};
use frame_system::{
	offchain::{CreateAuthorizedTransaction, CreateTransaction, CreateTransactionBase},
	AuthorizeCall,
};
use indiv_pallet_nft_claims::{CollectionSelector, Selection, SelectionError};
use indiv_support::{credit_trees::NftClaimCredit, identity::AccountOrPerson};
use pallet_revive::precompiles::AddressMapper;
use sp_runtime::{testing::UintAuthorityId, traits::Identity, AccountId32, BuildStorage};
use xcm::latest::{Junction::Parachain, Location};

/// The nft-claims pallet submits authorized calls from its offchain worker. Its `Config` therefore
/// needs a runtime that builds such a transaction, which a `MockBlock` does not.
pub type TransactionExtension = AuthorizeCall<Test>;
pub type Header = sp_runtime::generic::Header<u64, sp_runtime::traits::BlakeTwo256>;
pub type Block = sp_runtime::generic::Block<Header, Extrinsic>;
pub type Extrinsic = sp_runtime::generic::UncheckedExtrinsic<
	AccountId32,
	RuntimeCall,
	UintAuthorityId,
	TransactionExtension,
>;

/// Fixed address index of the minter-registration precompile in this mock.
pub const MINTER_INDEX: u16 = 0x0522;

frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		Balances: pallet_balances,
		Scarcity: pallet_scarcity,
		NftClaims: indiv_pallet_nft_claims,
		Revive: pallet_revive,
	}
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
	type AccountId = AccountId32;
	type Lookup = sp_runtime::traits::IdentityLookup<Self::AccountId>;
	type AccountData = pallet_balances::AccountData<u64>;
}

impl<LocalCall> CreateTransactionBase<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	type Extrinsic = Extrinsic;
	type RuntimeCall = RuntimeCall;
}

impl<LocalCall> CreateTransaction<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	type Extension = TransactionExtension;

	fn create_transaction(
		call: <Self as CreateTransactionBase<LocalCall>>::RuntimeCall,
		extension: Self::Extension,
	) -> Self::Extrinsic {
		Extrinsic::new_transaction(call, extension)
	}
}

impl<LocalCall> CreateAuthorizedTransaction<LocalCall> for Test
where
	RuntimeCall: From<LocalCall>,
{
	fn create_extension() -> Self::Extension {
		AuthorizeCall::new()
	}
}

#[derive_impl(pallet_balances::config_preludes::TestDefaultConfig)]
impl pallet_balances::Config for Test {
	type AccountStore = System;
	type MaxFreezes = frame_support::traits::VariantCountOf<RuntimeFreezeReason>;
}

parameter_types! {
	pub static MockNow: u64 = 0;
}

/// Test-controlled Unix time source.
pub struct MockUnixTime;
impl UnixTime for MockUnixTime {
	fn now() -> core::time::Duration {
		core::time::Duration::from_secs(MockNow::get())
	}
}

parameter_types! {
	pub MockGameChainLocation: Location = Location::new(1, [Parachain(1000)]);
	pub const ScarcityHoldReason: RuntimeHoldReason =
		RuntimeHoldReason::Scarcity(pallet_scarcity::HoldReason::StorageDeposit);
}

type StoragePrice = LinearStoragePrice<ConstU64<1>, ConstU64<1>, u64>;
type Consideration = HoldConsideration<AccountId32, Balances, ScarcityHoldReason, Identity, u64>;

impl pallet_scarcity::Config for Test {
	type RuntimeEvent = RuntimeEvent;
	type WeightInfo = ();
	type UnixTime = MockUnixTime;
	type Balance = u64;
	type Consideration = Consideration;
	type CollectionDeposit = StoragePrice;
	type ItemDeposit = StoragePrice;
	type InstanceDeposit = StoragePrice;
	type MetadataDeposit = StoragePrice;
	type MaxKeyLen = ConstU32<32>;
	type MaxValueLen = ConstU32<256>;
	type MaxInstanceMetadata = ConstU32<3>;
	type LockPeriod = ConstU64<60>;
	type MaxTransferPriority = ConstU64<1_000_000>;
	type OnCollectionDeleted = indiv_pallet_nft_claims::ClearCollectionMinter<Test>;
	// The runtime maps a mint's destination here. These tests cover the registration surface
	// and mint nothing, so the hook would never run.
	type OnPurseOccupied = ();
	// These tests store no metadata, so the runtime's ERC-721 policy would never be consulted.
	type MetadataPolicy = ();
}

parameter_types! {
	/// Whether the mock selector accepts a contract selection as deployed code. Stands in for
	/// the runtime adapter's code check, which this mock cannot perform without deploying
	/// real contracts.
	pub storage MinterContractValid: bool = true;
}

/// Stands in for the runtime's contract adapter. Registration only calls `validate`;
/// claims are outside this crate's surface, so `select` is never asked.
pub struct MockSelector;
impl CollectionSelector<AccountId32> for MockSelector {
	fn max_weight() -> Weight {
		Weight::from_parts(1_000_000, 5_000)
	}

	fn validate(_contract: H160) -> Result<(), DispatchError> {
		if !MinterContractValid::get() {
			return Err(DispatchError::Other("no contract code at the minter address"));
		}
		Ok(())
	}

	fn select(
		_owner: AccountId32,
		_contract: H160,
		_collection: CollectionId,
		_entropy: NftClaimCredit,
	) -> Result<Selection, SelectionError> {
		Err(SelectionError {
			error: DispatchError::Other("no claims are dispatched in this mock"),
			weight_consumed: Weight::zero(),
		})
	}
}

#[cfg(feature = "runtime-benchmarks")]
pub struct MockBenchmarkHelper;
#[cfg(feature = "runtime-benchmarks")]
impl indiv_pallet_nft_claims::BenchmarkHelper<AccountId32> for MockBenchmarkHelper {
	fn prepare_collection(
		owner: &AccountId32,
		collection: CollectionId,
		item: pallet_scarcity::ItemIndex,
	) {
		map_account(owner);
		while pallet_scarcity::NextCollectionId::<Test>::get() <= collection {
			pallet_scarcity::Pallet::<Test>::do_create_collection(owner.clone())
				.expect("collection is created; qed");
		}
		while pallet_scarcity::Collections::<Test>::get(collection)
			.expect("the collection was just created; qed")
			.next_item_index <=
			item
		{
			pallet_scarcity::Pallet::<Test>::do_define_item(
				owner.clone(),
				collection,
				pallet_scarcity::Transferability::Transferable,
				Vec::new(),
			)
			.expect("item is defined; qed");
		}
	}

	fn prepare_contract(_owner: &AccountId32) -> H160 {
		H160::repeat_byte(1)
	}

	fn set_unix_time(secs: u64) {
		MockNow::set(secs);
	}

	fn open_game_chain_channel(_max_message_size: u32) {
		// Nothing in this crate removes a tree, so the pallet sends no deletion message.
	}
}

impl indiv_pallet_nft_claims::Config for Test {
	type WeightInfo = ();
	// Tree delivery and claims are outside this crate's surface, so their origins never
	// resolve in this mock.
	type EnsureGameChainOrigin = frame_system::EnsureNever<()>;
	type MaxTreesPerMessage = ConstU32<4>;
	type EnsureClaimant = frame_system::EnsureNever<AccountOrPerson<AccountId32>>;
	type Nfts = Scarcity;
	type CollectionSelector = MockSelector;
	type MaxProofNodes = ConstU32<16>;
	// No tree reaches this mock, so the bitmap this sizes is never written.
	type MaxCreditsPerAwardBlock = ConstU32<12>;
	type UnixTime = MockUnixTime;
	type TreeTtl = ConstU64<{ 30 * 24 * 60 * 60 }>;
	type MaxQueuedTreeDeletions = ConstU32<8>;
	type MaxTreeDeletionsPerMessage = ConstU32<4>;
	// Nothing in this crate removes a tree, so the pallet sends no deletion.
	type XcmRouter = ();
	type GameChainLocation = MockGameChainLocation;
	type GameChainPalletIndex = ConstU8<42>;
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = MockBenchmarkHelper;
}

#[derive_impl(pallet_revive::config_preludes::TestDefaultConfig)]
impl pallet_revive::Config for Test {
	type AddressMapper = pallet_revive::AccountId32Mapper<Self>;
	type Balance = u64;
	type Currency = Balances;
	type Precompiles = (NftClaimsMinter<Self, MINTER_INDEX>,);
	type UploadOrigin = frame_system::EnsureSigned<AccountId32>;
	type InstantiateOrigin = frame_system::EnsureSigned<AccountId32>;
}

/// The minter-registration precompile's fixed address under [`MINTER_INDEX`].
pub fn minter_address() -> H160 {
	let mut address = [0u8; 20];
	address[16..18].copy_from_slice(&MINTER_INDEX.to_be_bytes());
	H160(address)
}

/// Fund `account` and register its H160↔AccountId32 mapping with `pallet-revive`.
pub fn map_account(account: &AccountId32) {
	Balances::make_free_balance_be(account, u64::MAX / 2);
	let _ = <Test as pallet_revive::Config>::AddressMapper::map(account);
}

pub fn id_to_account(id: u64) -> AccountId32 {
	let mut bytes = [0u8; 32];
	bytes[..8].copy_from_slice(&id.to_le_bytes());
	AccountId32::new(bytes)
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	let t = RuntimeGenesisConfig {
		system: Default::default(),
		balances: Default::default(),
		revive: Default::default(),
	}
	.build_storage()
	.unwrap();

	let mut ext: sp_io::TestExternalities = t.into();
	ext.execute_with(|| {
		System::set_block_number(1);
		MockNow::set(1_700_000_000);
	});
	ext
}
