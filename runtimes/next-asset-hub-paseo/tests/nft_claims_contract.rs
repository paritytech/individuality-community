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

//! End-to-end NFT claim tests running real PolkaVM contracts through the runtime's
//! `NftClaimsCollectionSelector`: Scarcity collection, deployed minter contract, credit tree,
//! claim and mint, all on the production `Runtime`.

use codec::Encode;
use frame_support::{
	assert_ok,
	traits::{
		fungible, fungibles,
		fungibles::Inspect,
		tokens::{Fortitude, Precision, Preservation},
		Currency,
	},
	weights::Weight,
	BoundedVec,
};
use indiv_pallet_nft_claims::{ClaimantKind, CollectionMinters, CreditTrees, ItemSelection};
use indiv_support::{
	credit_trees::{credit_leaf, CreditProofNode, NftClaimCredit, NftClaimCreditTree},
	identity::AccountOrPerson,
};
use next_asset_hub_paseo_runtime::{
	Assets, Balances, NftClaims, NftClaimsCollectionSelector, NftClaimsSelectorDepositLimit,
	PgasAssetId, Runtime, RuntimeOrigin, Scarcity, System,
};
use pallet_revive::{Code, ExecConfig, StorageDeposit, TransactionLimits};
use sp_core::H160;
use sp_runtime::{traits::BlakeTwo256, BuildStorage};

/// The People-chain block the test tree is stored under.
const BLOCK: u32 = 10;
/// The Scarcity collection the tests mint into, the first one created.
const COLLECTION: u32 = 0;

fn account(seed: u8) -> sp_runtime::AccountId32 {
	sp_runtime::AccountId32::new([seed; 32])
}

fn new_test_ext() -> sp_io::TestExternalities {
	let storage = frame_system::GenesisConfig::<Runtime>::default().build_storage().unwrap();
	let mut ext = sp_io::TestExternalities::from(storage);
	ext.execute_with(|| {
		System::set_block_number(1);
		pallet_timestamp::Pallet::<Runtime>::set_timestamp(1_000);
	});
	ext
}

/// The PolkaVM blob of `fixture`.
fn fixture(fixture: &str) -> Vec<u8> {
	let (code, _hash) = pallet_revive_fixtures::compile_module(fixture).unwrap();
	code
}

/// Create the collection with one item, owned by `owner`, funded well past every deposit in
/// both the native token and PGAS, which is what this runtime pays revive storage deposits in.
fn create_collection(owner: &sp_runtime::AccountId32) {
	use frame_support::traits::fungibles::{Create, Mutate};

	let _ = Balances::deposit_creating(owner, 1u128 << 60);
	<Assets as Create<_>>::create(PgasAssetId::get(), owner.clone(), true, 1).expect("pgas asset");
	<Assets as Mutate<_>>::mint_into(PgasAssetId::get(), owner, 1u128 << 60).expect("pgas funds");
	assert_eq!(Scarcity::do_create_collection(owner.clone()).expect("collection"), COLLECTION);
	assert_eq!(
		Scarcity::do_define_item(
			owner.clone(),
			COLLECTION,
			indiv_pallet_scarcity::Transferability::Transferable,
			Vec::new()
		)
		.expect("item"),
		0
	);
}

/// Deploy `code` as `owner`, endowing the contract with `value`, and return its address.
fn instantiate(owner: &sp_runtime::AccountId32, code: Vec<u8>, value: u128) -> H160 {
	let result = pallet_revive::Pallet::<Runtime>::bare_instantiate(
		RuntimeOrigin::signed(owner.clone()),
		value.into(),
		TransactionLimits::WeightAndDeposit {
			weight_limit: Weight::from_parts(500_000_000_000, 5 * 1024 * 1024),
			// Affordability is checked against the limit, so it must stay within the owner's
			// PGAS funding.
			deposit_limit: 1u128 << 50,
		},
		Code::Upload(code),
		Vec::new(),
		None,
		&ExecConfig::new_substrate_tx(),
	);
	result.result.expect("fixture instantiates").addr
}

/// Store a one-leaf credit tree of `claimant` holding `credit` under [`BLOCK`] and return the
/// (empty) inclusion proof of its only leaf.
fn store_tree(
	claimant: &AccountOrPerson<sp_runtime::AccountId32>,
	credit: NftClaimCredit,
) -> BoundedVec<CreditProofNode, frame_support::traits::ConstU32<16>> {
	let leaves = vec![credit_leaf(claimant, &credit)];
	let root = binary_merkle_tree::merkle_root::<BlakeTwo256, _>(leaves.clone());
	CreditTrees::<Runtime>::insert(
		BLOCK,
		NftClaimCreditTree { game_index: 1, root: root.into(), leaf_count: 1, timestamp: 1_000 },
	);
	let proof = binary_merkle_tree::merkle_proof::<BlakeTwo256, _, _>(leaves, 0);
	proof
		.proof
		.into_iter()
		.map(CreditProofNode::from)
		.collect::<Vec<_>>()
		.try_into()
		.expect("a one-leaf proof is empty")
}

/// The happy path: a deployed contract picks the item and the claim mints it.
///
/// `call_data_load` returns the 32-byte calldata word at the offset its first input byte
/// names. The `mint(uint32,bytes32)` selector's first byte (0xb3) is past the 68-byte
/// calldata, which EVM semantics zero-fill, so the contract returns one canonical zero word:
/// item 0. That is exactly the strict ABI surface a real minter must satisfy.
#[test]
fn a_deployed_contract_selects_the_minted_item() {
	let code = fixture("call_data_load");
	new_test_ext().execute_with(|| {
		let owner = account(1);
		let claimant = account(2);
		let purse = account(3);
		create_collection(&owner);
		let contract = instantiate(&owner, code, 0);

		assert_ok!(NftClaims::set_collection_minter(
			RuntimeOrigin::signed(owner),
			COLLECTION,
			Some(ItemSelection::Contract(contract))
		));

		let credit: NftClaimCredit = [7u8; 32];
		let proof = store_tree(&AccountOrPerson::Account(claimant.clone()), credit);
		assert_ok!(NftClaims::claim(
			RuntimeOrigin::signed(claimant),
			ClaimantKind::Account,
			BLOCK,
			credit,
			0,
			proof,
			COLLECTION,
			purse.clone()
		));

		let nft = indiv_pallet_scarcity::NftsByOwner::<Runtime>::get(&purse).expect("minted");
		assert_eq!((nft.collection, nft.item), (COLLECTION, 0));
	});
}

/// A state-writing minter's current collection owner collateralizes the storage it creates.
///
/// `multi_store` accepts two value lengths and writes both values. The adapter result and contract
/// accounting identify the charge and its payer.
#[test]
fn a_state_writing_minter_charges_the_collection_owner() {
	let code = fixture("multi_store");
	new_test_ext().execute_with(|| {
		let owner = account(1);
		create_collection(&owner);
		let contract = instantiate(&owner, code, 0);
		let contract_before = pallet_revive::AccountInfo::<Runtime>::load_contract(&contract)
			.expect("fixture is a contract");
		let contract_deposit_before = contract_before
			.storage_byte_deposit
			.saturating_add(contract_before.storage_item_deposit);
		let owner_pgas_before = <Assets as Inspect<_>>::balance(PgasAssetId::get(), &owner);

		let result =
			NftClaimsCollectionSelector::call(owner.clone(), contract, (256u32, 256u32).encode());
		assert!(result.result.is_ok());
		let StorageDeposit::Charge(charged) = result.storage_deposit else {
			panic!("state growth charges a deposit")
		};

		let contract_after = pallet_revive::AccountInfo::<Runtime>::load_contract(&contract)
			.expect("fixture remains a contract");
		let contract_deposit_after = contract_after
			.storage_byte_deposit
			.saturating_add(contract_after.storage_item_deposit);
		let owner_pgas_after = <Assets as Inspect<_>>::balance(PgasAssetId::get(), &owner);

		assert_eq!(contract_after.storage_bytes.saturating_sub(contract_before.storage_bytes), 608);
		assert!(charged > 0);
		assert!(charged <= NftClaimsSelectorDepositLimit::get());
		assert_eq!(contract_deposit_after.saturating_sub(contract_deposit_before), charged);
		assert_eq!(owner_pgas_before.saturating_sub(owner_pgas_after), charged);
	});
}

/// The counterpart of the charge: an owner holding neither PGAS nor spendable native funds
/// cannot collateralize what a minter writes, so the call fails and takes nothing.
///
/// Driven through the adapter rather than a claim because `multi_store` traps on the
/// `mint(uint32,bytes32)` calldata a claim sends, and the fixtures that survive that calldata
/// write no state to charge for. What a claim does with the failure, charging the weight the
/// call consumed and leaving the credit unspent, is therefore not covered here.
#[test]
fn a_state_writing_minter_fails_when_the_collection_owner_cannot_pay() {
	let code = fixture("multi_store");
	new_test_ext().execute_with(|| {
		let owner = account(1);
		create_collection(&owner);
		let contract = instantiate(&owner, code, 0);
		let bytes_before = pallet_revive::AccountInfo::<Runtime>::load_contract(&contract)
			.expect("fixture is a contract")
			.storage_bytes;

		// Empty both tokens the deposit can come from, in the order `PGasDeposit` tries them.
		// What the collection and contract deposits hold is not reducible, so it stays put.
		let pgas = <Assets as Inspect<_>>::balance(PgasAssetId::get(), &owner);
		assert_ok!(<Assets as fungibles::Mutate<_>>::burn_from(
			PgasAssetId::get(),
			&owner,
			pgas,
			Preservation::Expendable,
			Precision::Exact,
			Fortitude::Polite
		));
		let native = <Balances as fungible::Inspect<_>>::reducible_balance(
			&owner,
			Preservation::Expendable,
			Fortitude::Polite,
		);
		assert_ok!(<Balances as fungible::Mutate<_>>::burn_from(
			&owner,
			native,
			Preservation::Expendable,
			Precision::Exact,
			Fortitude::Polite
		));
		let owner_native_before = <Balances as Currency<_>>::total_balance(&owner);
		// The drain leaves the deposits behind, so the owner is still an account the call can
		// be made as. Asserted here so a change in how deposits are held fails on the setup
		// rather than as a dead-origin error below.
		assert!(System::account_exists(&owner));

		let result =
			NftClaimsCollectionSelector::call(owner.clone(), contract, (256u32, 256u32).encode());

		assert_eq!(
			result.result.expect_err("an owner who can pay neither token fails the call"),
			pallet_revive::Error::<Runtime>::StorageDepositNotEnoughFunds.into()
		);
		assert_eq!(result.storage_deposit, StorageDeposit::Charge(0));
		assert_eq!(<Balances as Currency<_>>::total_balance(&owner), owner_native_before);
		assert_eq!(<Assets as Inspect<_>>::balance(PgasAssetId::get(), &owner), 0);
		// The contract's writes survive the failure here, because `bare_call` wraps execution
		// in the reentrancy guard and not in a storage transaction: the frame itself succeeded
		// and only the deposit settlement after it failed. Discarding them is the caller's job,
		// which for a claim is the dispatch returning the error. Pinned so that a future
		// `bare_call` that does roll back is noticed rather than silently relied upon.
		assert_eq!(
			pallet_revive::AccountInfo::<Runtime>::load_contract(&contract)
				.expect("fixture remains a contract")
				.storage_bytes
				.saturating_sub(bytes_before),
			608
		);
	});
}

/// A contract that returns anything but one canonical `uint32` word fails the claim and
/// leaves the credit unspent, and switching the collection to `Random` claims it after all.
///
/// `ok_trap_revert` returns empty data for our selector's first byte, which the strict
/// return decoding rejects.
#[test]
fn a_malformed_contract_return_fails_the_claim_and_random_recovers_it() {
	let code = fixture("ok_trap_revert");
	new_test_ext().execute_with(|| {
		let owner = account(1);
		let claimant = account(2);
		let purse = account(3);
		create_collection(&owner);
		let contract = instantiate(&owner, code, 0);

		assert_ok!(NftClaims::set_collection_minter(
			RuntimeOrigin::signed(owner.clone()),
			COLLECTION,
			Some(ItemSelection::Contract(contract))
		));

		let credit: NftClaimCredit = [7u8; 32];
		let proof = store_tree(&AccountOrPerson::Account(claimant.clone()), credit);
		assert!(NftClaims::claim(
			RuntimeOrigin::signed(claimant.clone()),
			ClaimantKind::Account,
			BLOCK,
			credit,
			0,
			proof.clone(),
			COLLECTION,
			purse.clone()
		)
		.is_err());
		assert!(indiv_pallet_scarcity::NftsByOwner::<Runtime>::get(&purse).is_none());

		// The credit was not spent: the owner switches the collection to the random item and
		// the same claim goes through.
		assert_ok!(NftClaims::set_collection_minter(
			RuntimeOrigin::signed(owner),
			COLLECTION,
			Some(ItemSelection::Random)
		));
		assert_ok!(NftClaims::claim(
			RuntimeOrigin::signed(claimant),
			ClaimantKind::Account,
			BLOCK,
			credit,
			0,
			proof,
			COLLECTION,
			purse.clone()
		));
		assert!(indiv_pallet_scarcity::NftsByOwner::<Runtime>::get(&purse).is_some());
	});
}

/// A contract burning all its gas is stopped by the selector's weight ceiling and the claim
/// fails rather than the block.
///
/// `consume_all_gas` picks its behaviour by the value it receives: the endowment at deploy
/// makes the constructor succeed, and the selector's zero-value call burns the whole frame
/// and reverts.
#[test]
fn a_gas_burning_contract_is_stopped_by_the_selector_ceiling() {
	let code = fixture("consume_all_gas");
	new_test_ext().execute_with(|| {
		let owner = account(1);
		let claimant = account(2);
		create_collection(&owner);
		let contract = instantiate(&owner, code, 1u128 << 30);

		assert_ok!(NftClaims::set_collection_minter(
			RuntimeOrigin::signed(owner),
			COLLECTION,
			Some(ItemSelection::Contract(contract))
		));

		let credit: NftClaimCredit = [7u8; 32];
		let proof = store_tree(&AccountOrPerson::Account(claimant.clone()), credit);
		assert!(NftClaims::claim(
			RuntimeOrigin::signed(claimant),
			ClaimantKind::Account,
			BLOCK,
			credit,
			0,
			proof,
			COLLECTION,
			account(3)
		)
		.is_err());
	});
}

/// Registering an address with no code is rejected by the real code lookup, before any claim.
#[test]
fn an_address_without_code_is_rejected_at_registration() {
	new_test_ext().execute_with(|| {
		let owner = account(1);
		create_collection(&owner);

		assert!(NftClaims::set_collection_minter(
			RuntimeOrigin::signed(owner),
			COLLECTION,
			Some(ItemSelection::Contract(H160::repeat_byte(0xEE)))
		)
		.is_err());
		assert_eq!(CollectionMinters::<Runtime>::get(COLLECTION), None);
	});
}
