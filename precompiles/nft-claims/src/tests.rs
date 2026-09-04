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
use crate::mock::*;

use indiv_pallet_nft_claims::{CollectionMinter, WeightInfo};
use indiv_precompile_support::PROOF_SIZE_PER_READ;
use pallet_revive::{
	precompiles::{
		alloy::sol_types::{Revert, SolCall, SolError, SolInterface},
		AddressMapper,
	},
	sp_runtime::Weight,
	ExecConfig, TransactionLimits,
};
use sp_runtime::AccountId32;

/// Call the minter precompile with `input` and return the weight the call consumed alongside the
/// raw execution result.
fn call_weighed(caller: &AccountId32, input: Vec<u8>) -> (Weight, pallet_revive::ExecReturnValue) {
	let outcome = pallet_revive::Pallet::<Test>::bare_call(
		RuntimeOrigin::signed(caller.clone()),
		minter_address(),
		0u32.into(),
		TransactionLimits::WeightAndDeposit { weight_limit: Weight::MAX, deposit_limit: u64::MAX },
		input,
		&ExecConfig::new_substrate_tx(),
	);
	(outcome.weight_consumed, outcome.result.expect("precompile call should execute"))
}

/// Call the minter precompile with `input` and return the raw execution result.
fn call_precompile(caller: &AccountId32, input: Vec<u8>) -> pallet_revive::ExecReturnValue {
	call_weighed(caller, input).1
}

/// Call the minter precompile with `input`, attaching `value`, and return the raw execution
/// result.
fn call_with_value(
	caller: &AccountId32,
	input: Vec<u8>,
	value: u64,
) -> pallet_revive::ExecReturnValue {
	pallet_revive::Pallet::<Test>::bare_call(
		RuntimeOrigin::signed(caller.clone()),
		minter_address(),
		value.into(),
		TransactionLimits::WeightAndDeposit { weight_limit: Weight::MAX, deposit_limit: u64::MAX },
		input,
		&ExecConfig::new_substrate_tx(),
	)
	.result
	.expect("precompile call should execute")
}

/// The account the precompile address maps to, where stray value would land.
fn precompile_account() -> AccountId32 {
	<Test as pallet_revive::Config>::AddressMapper::to_fallback_account_id(&minter_address())
}

/// Call the minter precompile with `input`, expecting success, and return the output data.
fn call_ok(caller: &AccountId32, input: Vec<u8>) -> Vec<u8> {
	let output = call_precompile(caller, input);
	assert!(!output.did_revert(), "expected success, got revert: {output:?}");
	output.data
}

/// Call the minter precompile with `input`, expecting a revert whose reason contains `reason`.
fn call_reverted_with(caller: &AccountId32, input: Vec<u8>, reason: &str) {
	let output = call_precompile(caller, input);
	assert!(output.did_revert(), "expected revert, got success: {output:?}");
	let decoded = Revert::abi_decode(&output.data).expect("revert data decodes as Error(string)");
	assert!(
		decoded.reason.contains(reason),
		"revert reason {:?} does not contain {reason:?}",
		decoded.reason
	);
}

fn setup_collection(owner: &AccountId32) -> CollectionId {
	map_account(owner);
	indiv_pallet_scarcity::Pallet::<Test>::do_create_collection(owner.clone()).unwrap()
}

/// The registration `collectionMinter` reports for `collection`.
fn read_minter(collection: CollectionId) -> INftClaimsMinter::collectionMinterReturn {
	let reader = id_to_account(99);
	map_account(&reader);
	let data = call_ok(&reader, INftClaimsMinter::collectionMinterCall { collection }.abi_encode());
	INftClaimsMinter::collectionMinterCall::abi_decode_returns(&data).unwrap()
}

/// Every state-changing minter call, encoded for the precompile. The owner-gate and
/// unknown-collection tests both reject the whole set, so they share this to stay in step when a
/// call is added.
fn mutating_minter_calls(collection: CollectionId) -> [Vec<u8>; 3] {
	[
		INftClaimsMinter::setRandomMinterCall { collection }.abi_encode(),
		INftClaimsMinter::setContractMinterCall { collection, minter: H160([0xCC; 20]).0.into() }
			.abi_encode(),
		INftClaimsMinter::clearMinterCall { collection }.abi_encode(),
	]
}

#[test]
fn set_random_minter_round_trips() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let collection = setup_collection(&alice);

		call_ok(&alice, INftClaimsMinter::setRandomMinterCall { collection }.abi_encode());

		assert_eq!(
			CollectionMinters::<Test>::get(collection),
			Some(CollectionMinter { owner: alice.clone(), selection: ItemSelection::Random })
		);
		System::assert_has_event(RuntimeEvent::NftClaims(
			indiv_pallet_nft_claims::Event::CollectionMinterSet {
				collection,
				selection: Some(ItemSelection::Random),
			},
		));

		let minter = read_minter(collection);
		assert_eq!(minter.kind, KIND_RANDOM);
		assert_eq!(minter.minter, Address::ZERO);
		assert_eq!(minter.owner, address_of::<Test>(&alice));
	});
}

#[test]
fn set_contract_minter_round_trips() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let collection = setup_collection(&alice);
		let contract = H160([0xCC; 20]);

		call_ok(
			&alice,
			INftClaimsMinter::setContractMinterCall { collection, minter: contract.0.into() }
				.abi_encode(),
		);

		assert_eq!(
			CollectionMinters::<Test>::get(collection),
			Some(CollectionMinter {
				owner: alice.clone(),
				selection: ItemSelection::Contract(contract),
			})
		);

		let minter = read_minter(collection);
		assert_eq!(minter.kind, KIND_CONTRACT);
		assert_eq!(minter.minter.into_array(), contract.0);
		assert_eq!(minter.owner, address_of::<Test>(&alice));
	});
}

#[test]
fn clear_minter_round_trips() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let collection = setup_collection(&alice);

		// A contract selection, so the cleared read has a non-zero minter to lose: cleared from
		// a random one it reports the zero address either way, and the assertion below would
		// only repeat what `collection_minter_answers_none_when_unregistered` already pins.
		let contract = H160([0xCC; 20]);
		call_ok(
			&alice,
			INftClaimsMinter::setContractMinterCall { collection, minter: contract.0.into() }
				.abi_encode(),
		);
		let before = read_minter(collection);
		assert_eq!(before.kind, KIND_CONTRACT);
		assert_eq!(before.minter.into_array(), contract.0);

		call_ok(&alice, INftClaimsMinter::clearMinterCall { collection }.abi_encode());

		assert_eq!(CollectionMinters::<Test>::get(collection), None);
		System::assert_has_event(RuntimeEvent::NftClaims(
			indiv_pallet_nft_claims::Event::CollectionMinterSet { collection, selection: None },
		));

		let minter = read_minter(collection);
		assert_eq!(minter.kind, KIND_NONE);
		assert_eq!(minter.minter, Address::ZERO);
		assert_eq!(minter.owner, Address::ZERO);
	});
}

#[test]
fn non_owner_cannot_register_or_withdraw() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let bob = id_to_account(2);
		let collection = setup_collection(&alice);
		map_account(&bob);

		for input in mutating_minter_calls(collection) {
			call_reverted_with(&bob, input, "caller is not the collection owner");
		}
		assert_eq!(CollectionMinters::<Test>::get(collection), None);
	});
}

#[test]
fn unknown_collection_reverts() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		map_account(&alice);
		let unknown = 7;

		for input in mutating_minter_calls(unknown) {
			call_reverted_with(&alice, input, "unknown collection");
		}
		assert_eq!(CollectionMinters::<Test>::get(unknown), None);
	});
}

#[test]
fn rejected_contract_selection_reverts() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let collection = setup_collection(&alice);
		MinterContractValid::set(&false);

		call_reverted_with(
			&alice,
			INftClaimsMinter::setContractMinterCall {
				collection,
				minter: H160([0xCC; 20]).0.into(),
			}
			.abi_encode(),
			"no contract code at the minter address",
		);
		assert_eq!(CollectionMinters::<Test>::get(collection), None);

		// The selector only checks contract selections, so random registration still works.
		call_ok(&alice, INftClaimsMinter::setRandomMinterCall { collection }.abi_encode());
	});
}

/// Both branches charge before they act, so the frame consumes at least what each one declares.
///
/// The mutator charges the pallet's `set_collection_minter` weight and the read one database
/// read. Dropping either charge leaves the work in place and the test is what notices, since a
/// frame that charges nothing still returns the right answer.
#[test]
fn each_method_charges_its_weight() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let collection = setup_collection(&alice);

		let (mutating, output) =
			call_weighed(&alice, INftClaimsMinter::setRandomMinterCall { collection }.abi_encode());
		assert!(!output.did_revert(), "expected success, got revert: {output:?}");
		let declared =
			<Test as indiv_pallet_nft_claims::Config>::WeightInfo::set_collection_minter();
		assert!(
			mutating.all_gte(declared),
			"the mutator consumed {mutating:?}, less than the {declared:?} it charges"
		);

		let (read, output) = call_weighed(
			&alice,
			INftClaimsMinter::collectionMinterCall { collection }.abi_encode(),
		);
		assert!(!output.did_revert(), "expected success, got revert: {output:?}");
		// `DbWeight` is zero in this mock, so a read shows up in the proof size alone.
		assert!(
			read.proof_size() >= PROOF_SIZE_PER_READ,
			"the read consumed {read:?}, less than the one database read it charges"
		);
		// The read reaches no pallet call, which the mutator's execution time covers.
		assert!(
			read.ref_time() < mutating.ref_time(),
			"the read consumed {read:?}, no less than the mutator's {mutating:?}"
		);
	});
}

#[test]
fn collection_minter_answers_none_when_unregistered() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let collection = setup_collection(&alice);

		// A collection that was never registered and an unknown one answer alike.
		for queried in [collection, 999] {
			let minter = read_minter(queried);
			assert_eq!(minter.kind, KIND_NONE);
			assert_eq!(minter.minter, Address::ZERO);
			assert_eq!(minter.owner, Address::ZERO);
		}
	});
}

/// Assert that calling the precompile with `input` and value attached is rejected and costs
/// nothing.
fn assert_rejects_value(caller: &AccountId32, input: Vec<u8>, method: &str) {
	let before = Balances::free_balance(caller);
	let output = call_with_value(caller, input, 1_000);
	assert!(output.did_revert(), "{method}: expected revert, got success: {output:?}");
	let decoded = Revert::abi_decode(&output.data).expect("revert data decodes as Error(string)");
	assert!(
		decoded.reason.contains("this precompile does not accept value"),
		"{method}: revert reason {:?}",
		decoded.reason
	);
	// The frame unwinds the transfer with the rest of its state changes.
	assert_eq!(Balances::free_balance(caller), before, "{method}: caller was charged");
	assert_eq!(
		Balances::free_balance(precompile_account()),
		0,
		"{method}: value stranded at the precompile"
	);
}

/// No function of the interface is payable, so every one of them must reject attached value.
///
/// The cases below are checked against the generated selector set, so a method added to
/// `INftClaimsMinter.sol` fails this test until it is covered here. Arguments are the ones
/// that would otherwise succeed, which is what makes each case prove the rejection wins over
/// the real path rather than over some other revert.
#[test]
fn every_method_rejects_attached_value() {
	new_test_ext().execute_with(|| {
		let alice = id_to_account(1);
		let collection = setup_collection(&alice);
		let contract = H160([0xCC; 20]);

		let calls = alloc::vec![
			INftClaimsMinterCalls::setRandomMinter(INftClaimsMinter::setRandomMinterCall {
				collection,
			}),
			INftClaimsMinterCalls::setContractMinter(INftClaimsMinter::setContractMinterCall {
				collection,
				minter: contract.0.into(),
			}),
			INftClaimsMinterCalls::clearMinter(INftClaimsMinter::clearMinterCall { collection }),
			INftClaimsMinterCalls::collectionMinter(INftClaimsMinter::collectionMinterCall {
				collection,
			}),
		];

		// Exhaustiveness: every generated selector has a case above.
		let covered = calls.iter().map(|call| call.selector()).collect::<Vec<_>>();
		for selector in INftClaimsMinterCalls::selectors() {
			assert!(covered.contains(&selector), "no case for selector {selector:?}");
		}
		assert_eq!(covered.len(), INftClaimsMinterCalls::COUNT);

		for call in &calls {
			let method = alloc::format!("selector {:02x?}", call.selector());
			assert_rejects_value(&alice, call.abi_encode(), &method);
		}

		// None of the mutators above took effect.
		assert_eq!(CollectionMinters::<Test>::get(collection), None);
	});
}

/// Every pallet error variant the precompile can reach must map to a catchable revert.
///
/// The mapping in `revert_nft_claims` is a runtime list, so the compiler cannot flag a
/// variant added to `pallet-nft-claims` later. This test walks the variants from the error
/// type's own metadata and fails on any reachable one that starts trapping instead of
/// reverting.
#[test]
fn mapped_nft_claims_errors_are_exhaustive() {
	// The ABI covers only `set_collection_minter`; the claim and tree-delivery errors cannot
	// surface through it.
	const UNREACHABLE: [&str; 7] = [
		"UnknownAwardBlock",
		"LeafIndexOutOfBounds",
		"AlreadyClaimed",
		"InvalidProof",
		"CollectionNotRegistered",
		"CollectionOwnerChanged",
		"NoItems",
	];

	let pallet_index = match DispatchError::from(NftClaimsError::<Test>::NotCollectionOwner) {
		DispatchError::Module(module) => module.index,
		other => panic!("pallet errors are module errors, got {other:?}"),
	};
	let variants = match <NftClaimsError<Test> as scale_info::TypeInfo>::type_info().type_def {
		scale_info::TypeDef::Variant(def) => def.variants,
		other => panic!("pallet errors are a variant type, got {other:?}"),
	};
	assert!(!variants.is_empty(), "error metadata carries no variants");

	for variant in &variants {
		let error = DispatchError::Module(sp_runtime::ModuleError {
			index: pallet_index,
			error: [variant.index, 0, 0, 0],
			message: None,
		});
		let reverts = matches!(revert_nft_claims::<Test>(error), Error::Revert(_));
		let reachable = !UNREACHABLE.contains(&variant.name);
		assert_eq!(
			reverts,
			reachable,
			"{}: reverts={reverts}, but it is {} through this precompile. Map it in \
			 `revert_nft_claims`, or add it to UNREACHABLE if the ABI cannot reach it.",
			variant.name,
			if reachable { "reachable" } else { "unreachable" }
		);
	}

	for name in UNREACHABLE {
		assert!(
			variants.iter().any(|variant| variant.name == name),
			"UNREACHABLE lists {name}, which no longer exists in the pallet"
		);
	}
}

/// Frame-flag guards, driven through `pallet_revive::precompiles::run`, which executes a
/// precompile inside a frame with controlled read-only and delegate-call flags.
///
/// `pallet-revive` exports that harness only under `runtime-benchmarks`, and enabling the
/// feature unconditionally would grow the benchmark-only methods of the FRAME traits without
/// enabling the feature on the pallets that implement them, breaking any workspace-wide
/// build. The gate keeps it to feature-enabled runs, which is where CI exercises it.
#[cfg(feature = "runtime-benchmarks")]
mod guards {
	use super::*;
	use pallet_revive::precompiles::run::{
		precompile as run_precompile, CallSetup, VmBinaryModule,
	};

	fn read_call() -> INftClaimsMinterCalls {
		INftClaimsMinterCalls::collectionMinter(INftClaimsMinter::collectionMinterCall {
			collection: 0,
		})
	}

	fn mutating_call() -> INftClaimsMinterCalls {
		INftClaimsMinterCalls::setRandomMinter(INftClaimsMinter::setRandomMinterCall {
			collection: 0,
		})
	}

	fn assert_denied_with(result: Result<Vec<u8>, Error>, expected: pallet_revive::Error<Test>) {
		let expected: DispatchError = expected.into();
		match result {
			Err(Error::Error(e)) => assert_eq!(e.error, expected),
			other => panic!("expected {expected:?}, got {other:?}"),
		}
	}

	/// The revert message `Error::try_to_revert` produces for a delegate call.
	const DELEGATE_DENIED_REVERT: &str = "illegal to call this pre-compile via delegate call";

	#[test]
	fn delegate_call_is_denied() {
		new_test_ext().execute_with(|| {
			let mut setup = CallSetup::<Test>::new(VmBinaryModule::dummy());
			setup.set_delegate_call(true);
			let (mut ext, _) = setup.ext();

			// The guard reverts with a reason rather than trapping, so a delegatecaller keeps
			// its gas and can catch the failure.
			for input in [read_call(), mutating_call()] {
				let result = run_precompile::<NftClaimsMinter<Test, MINTER_INDEX>, _>(
					&mut ext,
					&minter_address().0,
					&input,
				);
				assert!(
					matches!(&result, Err(Error::Revert(r)) if r.reason == DELEGATE_DENIED_REVERT),
					"expected a delegate-call revert, got {result:?}"
				);
			}
		});
	}

	#[test]
	fn read_only_frame_denies_mutations_and_serves_reads() {
		new_test_ext().execute_with(|| {
			let mut setup = CallSetup::<Test>::new(VmBinaryModule::dummy());
			setup.set_read_only(true);
			let (mut ext, _) = setup.ext();

			let mutation = run_precompile::<NftClaimsMinter<Test, MINTER_INDEX>, _>(
				&mut ext,
				&minter_address().0,
				&mutating_call(),
			);
			assert_denied_with(mutation, pallet_revive::Error::<Test>::StateChangeDenied);

			// Views keep answering inside a STATICCALL frame.
			let read = run_precompile::<NftClaimsMinter<Test, MINTER_INDEX>, _>(
				&mut ext,
				&minter_address().0,
				&read_call(),
			)
			.expect("reads must succeed in a read-only frame");
			let minter = INftClaimsMinter::collectionMinterCall::abi_decode_returns(&read).unwrap();
			assert_eq!(minter.kind, KIND_NONE);
		});
	}
}

/// End-to-end tests driving the minter precompile from real compiled EVM contracts: a contract
/// that owns a collection and registers itself as the minter, and a standalone minter contract a
/// collection owner registers. Compiled on demand by `indiv-precompile-fixtures`, which panics if
/// `solc` or `resolc` is missing, so a passing run always exercised real bytecode.
mod evm_fixture {
	use super::*;
	use frame_support::traits::Currency;
	use indiv_precompile_fixtures::fixture_code;
	use indiv_precompile_support::test_helpers::alloy_address;
	use pallet_revive::{precompiles::AddressMapper, Code};

	alloy::sol! {
		interface ISelfMinter {
			struct BootstrapConfig {
				address factory;
				uint16 prefix;
				address claims;
			}
			function bootstrap(BootstrapConfig config) external returns (uint32 collection);
		}

		interface IClaimsMinterCaller {
			function readMinterInStaticFrame(address claims, uint32 collection)
				external
				view
				returns (bool ok, bytes returnData);
			function registerInStaticFrame(address claims, uint32 collection)
				external
				view
				returns (bool ok, bytes returnData);
			function register(address claims, uint32 collection)
				external
				returns (bool ok, bytes returnData);
		}
	}

	fn contract_account(address: H160) -> AccountId32 {
		<Test as pallet_revive::Config>::AddressMapper::to_account_id(&address)
	}

	fn deploy(owner: &AccountId32, code: Vec<u8>) -> H160 {
		pallet_revive::Pallet::<Test>::bare_instantiate(
			RuntimeOrigin::signed(owner.clone()),
			0u32.into(),
			TransactionLimits::WeightAndDeposit {
				weight_limit: Weight::MAX,
				deposit_limit: 1u64 << 50,
			},
			Code::Upload(code),
			Vec::new(),
			None,
			&ExecConfig::new_substrate_tx(),
		)
		.result
		.expect("contract instantiates")
		.addr
	}

	/// Panics if the call reverts.
	fn call_contract(caller: &AccountId32, contract: H160, input: Vec<u8>) -> Vec<u8> {
		let output = pallet_revive::Pallet::<Test>::bare_call(
			RuntimeOrigin::signed(caller.clone()),
			contract,
			0u32.into(),
			TransactionLimits::WeightAndDeposit {
				weight_limit: Weight::MAX,
				deposit_limit: u64::MAX,
			},
			input,
			&ExecConfig::new_substrate_tx(),
		)
		.result
		.expect("contract call executes");
		assert!(!output.did_revert(), "expected success, got revert: {output:?}");
		output.data
	}

	#[test]
	fn contract_owns_collection_and_self_registers() {
		let code = fixture_code("SelfMinter");
		new_test_ext().execute_with(|| {
			let alice = id_to_account(1);
			map_account(&alice);
			let self_minter = deploy(&alice, code);
			// The contract, not the caller, pays the collection and item deposits it incurs.
			Balances::make_free_balance_be(&contract_account(self_minter), u64::MAX / 2);

			let bootstrapped = call_contract(
				&alice,
				self_minter,
				ISelfMinter::bootstrapCall {
					config: ISelfMinter::BootstrapConfig {
						factory: alloy_address(factory_address()),
						prefix: COLLECTION_PREFIX,
						claims: alloy_address(minter_address()),
					},
				}
				.abi_encode(),
			);
			let collection = ISelfMinter::bootstrapCall::abi_decode_returns(&bootstrapped).unwrap();

			// The registration names the contract as both the collection owner and the minter.
			let registration = read_minter(collection);
			assert_eq!(registration.kind, crate::KIND_CONTRACT);
			assert_eq!(registration.minter, alloy_address(self_minter));
			assert_eq!(registration.owner, alloy_address(self_minter));
		});
	}

	#[test]
	fn deployed_contract_is_registered_as_minter() {
		let code = fixture_code("ClaimsMinter");
		new_test_ext().execute_with(|| {
			let alice = id_to_account(1);
			let collection = setup_collection(&alice);
			let minter_contract = deploy(&alice, code);

			// The collection owner registers the deployed contract as its claims minter.
			call_ok(
				&alice,
				INftClaimsMinter::setContractMinterCall {
					collection,
					minter: alloy_address(minter_contract),
				}
				.abi_encode(),
			);

			let registration = read_minter(collection);
			assert_eq!(registration.kind, crate::KIND_CONTRACT);
			assert_eq!(registration.minter, alloy_address(minter_contract));
			assert_eq!(registration.owner, address_of::<Test>(&alice));
		});
	}

	#[test]
	fn a_read_only_frame_serves_the_read_and_denies_the_registration() {
		let code = fixture_code("ClaimsMinterCaller");
		new_test_ext().execute_with(|| {
			let alice = id_to_account(1);
			map_account(&alice);
			let caller = deploy(&alice, code);
			let owner = contract_account(caller);
			// The contract owns the collection, so it pays the deposit that creating one incurs.
			Balances::make_free_balance_be(&owner, u64::MAX / 2);
			let collection =
				indiv_pallet_scarcity::Pallet::<Test>::do_create_collection(owner.clone())
					.expect("collection is created");
			let claims = alloy_address(minter_address());

			let read = |collection| {
				let data = call_contract(
					&alice,
					caller,
					IClaimsMinterCaller::readMinterInStaticFrameCall { claims, collection }
						.abi_encode(),
				);
				let outcome =
					IClaimsMinterCaller::readMinterInStaticFrameCall::abi_decode_returns(&data)
						.unwrap();
				assert!(outcome.ok, "a read must be served in a read-only frame");
				INftClaimsMinter::collectionMinterCall::abi_decode_returns(&outcome.returnData)
					.expect("the read answers with an encoded registration")
			};

			assert_eq!(read(collection).kind, crate::KIND_NONE);

			// The registration is denied and the frame traps rather than reverting, so nothing
			// comes back. An empty return rules out the mapped reverts, which each carry a reason,
			// but names no error: a trapped frame reports none to its caller.
			let data = call_contract(
				&alice,
				caller,
				IClaimsMinterCaller::registerInStaticFrameCall { claims, collection }.abi_encode(),
			);
			let denied =
				IClaimsMinterCaller::registerInStaticFrameCall::abi_decode_returns(&data).unwrap();
			assert!(!denied.ok, "a registration must be denied in a read-only frame");
			assert!(
				denied.returnData.is_empty(),
				"expected a trapped frame, got {:?}",
				denied.returnData
			);
			assert_eq!(CollectionMinters::<Test>::get(collection), None);

			// The same call outside a read-only frame goes through, which leaves the frame's
			// read-only flag as the reason for the denial above.
			let data = call_contract(
				&alice,
				caller,
				IClaimsMinterCaller::registerCall { claims, collection }.abi_encode(),
			);
			let registered = IClaimsMinterCaller::registerCall::abi_decode_returns(&data).unwrap();
			assert!(
				registered.ok,
				"the owner registers outside a read-only frame, got {:?}",
				registered.returnData
			);
			assert_eq!(
				CollectionMinters::<Test>::get(collection),
				Some(CollectionMinter { owner, selection: ItemSelection::Random })
			);

			// The read-only frame reports the registration the writable one made.
			assert_eq!(read(collection).kind, crate::KIND_RANDOM);
		});
	}
}
