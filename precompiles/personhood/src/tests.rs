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
use crate::{mock::*, DEFAULT_CONTEXT_ALIAS};

use alloy::sol_types::{Revert, SolCall, SolError};
use indiv_precompile_support::ERR_VALUE_NOT_ACCEPTED;
use indiv_support::traits::{Alias, Context, PEOPLE_IDENTIFIER, PEOPLE_LITE_IDENTIFIER};
use pallet_revive::{precompiles::AddressMapper, ExecConfig, TransactionLimits};
use sp_runtime::Weight;

fn test_context() -> Context {
	let mut ctx = DEFAULT_CONTEXT_ALIAS;
	ctx[..5].copy_from_slice(b"dotns");
	ctx
}

fn call_precompile(
	caller: u64,
	target_account: &sp_runtime::AccountId32,
	context: &Context,
) -> IPersonhood::PersonhoodInfo {
	let caller_account = id_to_account(caller);
	map_account::<Test>(&caller_account);

	let target_address = <Test as pallet_revive::Config>::AddressMapper::to_address(target_account);

	let input = IPersonhood::personhoodStatusCall {
		account: target_address.0.into(),
		context: (*context).into(),
	}
	.abi_encode();

	let data = pallet_revive::Pallet::<Test>::bare_call(
		RuntimeOrigin::signed(caller_account),
		PRECOMPILE_ADDR,
		0u32.into(),
		TransactionLimits::WeightAndDeposit { weight_limit: Weight::MAX, deposit_limit: u64::MAX },
		input,
		&ExecConfig::new_substrate_tx(),
	)
	.result
	.expect("precompile call should succeed")
	.data;

	IPersonhood::personhoodStatusCall::abi_decode_returns(&data).unwrap()
}

#[test]
fn returns_none_for_unknown_account() {
	new_test_ext().execute_with(|| {
		let info = call_precompile(1, &id_to_account(99), &test_context());
		assert_eq!(info.status as u8, NO_STATUS);
		assert_eq!(info.contextAlias.0, DEFAULT_CONTEXT_ALIAS);
	});
}

#[test]
fn returns_lite_with_alias() {
	new_test_ext().execute_with(|| {
		let target = id_to_account(10);
		map_account::<Test>(&target);
		set_personhood(&target, &test_context(), *PEOPLE_LITE_IDENTIFIER, ALICE_ALIAS);

		let info = call_precompile(1, &target, &test_context());
		assert_eq!(info.status as u8, LITE_STATUS);
		assert_eq!(info.contextAlias.0, ALICE_ALIAS);
	});
}

#[test]
fn returns_full_with_alias() {
	new_test_ext().execute_with(|| {
		let target = id_to_account(20);
		map_account::<Test>(&target);
		set_personhood(&target, &test_context(), *PEOPLE_IDENTIFIER, BOB_ALIAS);

		let info = call_precompile(1, &target, &test_context());
		assert_eq!(info.status as u8, FULL_STATUS);
		assert_eq!(info.contextAlias.0, BOB_ALIAS);
	});
}

#[test]
fn wrong_context_returns_none() {
	new_test_ext().execute_with(|| {
		let target = id_to_account(4);
		map_account::<Test>(&target);
		let other_context = [0xFFu8; 32];
		set_personhood(&target, &other_context, *PEOPLE_IDENTIFIER, DAVE_ALIAS);

		let info = call_precompile(1, &target, &test_context());
		assert_eq!(info.status as u8, NO_STATUS);
	});
}

#[test]
fn unknown_collection_returns_none() {
	new_test_ext().execute_with(|| {
		let target = id_to_account(5);
		map_account::<Test>(&target);
		let unknown_collection = [0xABu8; 32];
		set_personhood(&target, &test_context(), unknown_collection, EVE_ALIAS);

		let info = call_precompile(1, &target, &test_context());
		assert_eq!(info.status as u8, NO_STATUS);
		assert_eq!(info.contextAlias.0, DEFAULT_CONTEXT_ALIAS);
	});
}

fn call_proof_precompile(
	caller: u64,
	expected_status: u8,
	expected_alias: Alias,
	context: &Context,
	proof: Vec<u8>,
) -> pallet_revive::ContractResult<pallet_revive::ExecReturnValue, u64> {
	let caller_account = id_to_account(caller);
	map_account::<Test>(&caller_account);

	let input = IPersonhood::personhoodInfoByProofCall {
		request: IPersonhood::ProofVerificationRequest {
			expectedStatus: expected_status,
			proof: proof.into(),
			expectedAlias: expected_alias.into(),
			ringIndex: 0,
			context: (*context).into(),
			revision: 1,
			message: Vec::new().into(),
		},
	}
	.abi_encode();

	pallet_revive::Pallet::<Test>::bare_call(
		RuntimeOrigin::signed(caller_account),
		PRECOMPILE_ADDR,
		0u32.into(),
		TransactionLimits::WeightAndDeposit { weight_limit: Weight::MAX, deposit_limit: u64::MAX },
		input,
		&ExecConfig::new_substrate_tx(),
	)
}

fn decode_proof_status(
	result: pallet_revive::ContractResult<pallet_revive::ExecReturnValue, u64>,
) -> bool {
	let data = result.result.expect("precompile call should succeed").data;
	IPersonhood::personhoodInfoByProofCall::abi_decode_returns(&data).unwrap()
}

#[test]
fn proof_returns_full_for_people_match() {
	new_test_ext().execute_with(|| {
		set_proof_result(ALICE_ALIAS, test_context(), *PEOPLE_IDENTIFIER);

		let ok = decode_proof_status(call_proof_precompile(
			1,
			FULL_STATUS,
			ALICE_ALIAS,
			&test_context(),
			Vec::new(),
		));
		assert!(ok);
	});
}

#[test]
fn proof_returns_lite_for_people_lite_match() {
	new_test_ext().execute_with(|| {
		set_proof_result(BOB_ALIAS, test_context(), *PEOPLE_LITE_IDENTIFIER);

		let ok = decode_proof_status(call_proof_precompile(
			1,
			LITE_STATUS,
			BOB_ALIAS,
			&test_context(),
			Vec::new(),
		));
		assert!(ok);
	});
}

#[test]
fn proof_returns_none_for_unknown_alias() {
	new_test_ext().execute_with(|| {
		let ok = decode_proof_status(call_proof_precompile(
			1,
			FULL_STATUS,
			CHARLIE_ALIAS,
			&test_context(),
			Vec::new(),
		));
		assert!(!ok);
	});
}

#[test]
fn proof_returns_none_for_proof_belonging_to_other_collection() {
	new_test_ext().execute_with(|| {
		// Proof was issued under People-Lite but caller asks for Full.
		set_proof_result(DAVE_ALIAS, test_context(), *PEOPLE_LITE_IDENTIFIER);

		let ok = decode_proof_status(call_proof_precompile(
			1,
			FULL_STATUS,
			DAVE_ALIAS,
			&test_context(),
			Vec::new(),
		));
		assert!(!ok);
	});
}

#[test]
fn proof_returns_none_for_unsupported_status() {
	new_test_ext().execute_with(|| {
		set_proof_result(ALICE_ALIAS, test_context(), *PEOPLE_IDENTIFIER);

		let ok = decode_proof_status(call_proof_precompile(
			1,
			99,
			ALICE_ALIAS,
			&test_context(),
			Vec::new(),
		));
		assert!(!ok);
	});
}

#[test]
fn proof_unsupported_status_refunds_gas() {
	new_test_ext().execute_with(|| {
		set_proof_result(ALICE_ALIAS, test_context(), *PEOPLE_IDENTIFIER);

		let result_match = call_proof_precompile(
			1,
			FULL_STATUS,
			ALICE_ALIAS,
			&test_context(),
			Vec::new(),
		);
		let result_unsupported = call_proof_precompile(
			2,
			99,
			ALICE_ALIAS,
			&test_context(),
			Vec::new(),
		);

		let weight_match = result_match.weight_consumed;
		let weight_unsupported = result_unsupported.weight_consumed;
		assert!(decode_proof_status(result_match));
		assert!(!decode_proof_status(result_unsupported));

		assert!(
			weight_unsupported.ref_time() < weight_match.ref_time(),
			"unsupported-status path ({weight_unsupported:?}) should refund vs matched path ({weight_match:?})",
		);
	});
}

#[test]
fn proof_oversized_returns_none_and_refunds_gas() {
	new_test_ext().execute_with(|| {
		set_proof_result(ALICE_ALIAS, test_context(), *PEOPLE_IDENTIFIER);

		let result_match =
			call_proof_precompile(1, FULL_STATUS, ALICE_ALIAS, &test_context(), Vec::new());
		let result_oversized = call_proof_precompile(
			2,
			FULL_STATUS,
			ALICE_ALIAS,
			&test_context(),
			alloc::vec![0xAA; 1],
		);

		let weight_match = result_match.weight_consumed;
		let weight_oversized = result_oversized.weight_consumed;
		assert!(decode_proof_status(result_match));
		assert!(!decode_proof_status(result_oversized));

		assert!(
			weight_oversized.ref_time() < weight_match.ref_time(),
			"oversized-proof path ({weight_oversized:?}) should refund vs matched path ({weight_match:?})",
		);
	});
}

/// A call carrying value reverts before the query runs.
///
/// Both selectors are `view`, so the typed interface keeps value out. A raw `call{value: ...}`
/// reaches the precompile regardless, which is what this sends.
#[test]
fn attached_value_is_rejected() {
	new_test_ext().execute_with(|| {
		let caller = id_to_account(1);
		map_account::<Test>(&caller);
		let target = <Test as pallet_revive::Config>::AddressMapper::to_address(&id_to_account(99));
		let input = IPersonhood::personhoodStatusCall {
			account: target.0.into(),
			context: test_context().into(),
		}
		.abi_encode();

		let before = Balances::free_balance(&caller);
		let output = pallet_revive::Pallet::<Test>::bare_call(
			RuntimeOrigin::signed(caller.clone()),
			PRECOMPILE_ADDR,
			1_000u32.into(),
			TransactionLimits::WeightAndDeposit {
				weight_limit: Weight::MAX,
				deposit_limit: u64::MAX,
			},
			input,
			&ExecConfig::new_substrate_tx(),
		)
		.result
		.expect("precompile call should execute");

		assert!(output.did_revert(), "expected revert, got success: {output:?}");
		let decoded =
			Revert::abi_decode(&output.data).expect("revert data decodes as Error(string)");
		assert_eq!(decoded.reason, ERR_VALUE_NOT_ACCEPTED);

		// The frame unwinds the transfer with its other state changes.
		assert_eq!(Balances::free_balance(&caller), before, "caller was charged");
		let stranded = <Test as pallet_revive::Config>::AddressMapper::to_fallback_account_id(
			&PRECOMPILE_ADDR,
		);
		assert_eq!(Balances::free_balance(&stranded), 0, "value stranded at the precompile");
	});
}

/// End-to-end tests driving the precompile from a real compiled EVM contract, which checks the
/// frame guards as a Solidity caller observes them. `indiv-precompile-fixtures` compiles the
/// contract on demand and panics if `solc` is missing, so a passing run always exercised real
/// bytecode.
mod evm_fixture {
	use super::*;
	use frame_support::traits::Currency;
	use indiv_precompile_fixtures::fixture_code;
	use pallet_revive::{precompiles::alloy::primitives::Address, Code};
	use sp_runtime::AccountId32;

	/// Register `account`'s address mapping with only the funds its deposit needs.
	///
	/// `map_account` endows `u64::MAX / 2`, and two such accounts plus a contract's deposits
	/// overflow the mock's total issuance.
	fn map_modestly(account: &AccountId32) {
		Balances::make_free_balance_be(account, 1u64 << 40);
		let _ = <Test as pallet_revive::Config>::AddressMapper::map(account);
	}

	// Mirrors the `ProofVerificationRequest` struct in `sol/IPersonhood.sol`, which `lib.rs`
	// compiles with `alloy::sol!`. Keep the field order in step with that file, or this test
	// encodes the wrong calldata and still passes.
	alloy::sol! {
		struct ProofVerificationRequest {
			uint8 expectedStatus;
			bytes proof;
			bytes32 expectedAlias;
			uint32 ringIndex;
			bytes32 context;
			uint32 revision;
			bytes message;
		}

		interface IPersonhoodCaller {
			function readInStaticFrame(address personhood, address account, bytes32 context) external view returns (bool ok, bytes returnData);
			function verifyProofInStaticFrame(address personhood, ProofVerificationRequest request) external view returns (bool ok, bytes returnData);
			function readViaDelegateCall(address personhood, address account, bytes32 context) external returns (bool ok, bytes returnData);
			function readWithValue(address personhood, address account, bytes32 context) external payable returns (bool ok, bytes returnData);
		}
	}

	/// The revert reason `ensure_not_delegate` produces, read from `pallet-revive` rather than
	/// copied, so an upstream wording change does not fail a test on the literal.
	fn delegate_denied_reason() -> String {
		match Error::try_to_revert::<Test>(
			pallet_revive::Error::<Test>::PrecompileDelegateDenied.into(),
		) {
			Error::Revert(revert) => revert.reason,
			other => panic!("a delegate call must revert, got {other:?}"),
		}
	}

	fn deploy(owner: &AccountId32) -> H160 {
		pallet_revive::Pallet::<Test>::bare_instantiate(
			RuntimeOrigin::signed(owner.clone()),
			0u32.into(),
			TransactionLimits::WeightAndDeposit {
				weight_limit: Weight::MAX,
				deposit_limit: 1u64 << 50,
			},
			Code::Upload(fixture_code("PersonhoodCaller")),
			Vec::new(),
			None,
			&ExecConfig::new_substrate_tx(),
		)
		.result
		.expect("contract instantiates")
		.addr
	}

	/// Call `contract` with `input` and `value`, and assert the outer call succeeds.
	fn call_contract(caller: &AccountId32, contract: H160, input: Vec<u8>, value: u64) -> Vec<u8> {
		let output = pallet_revive::Pallet::<Test>::bare_call(
			RuntimeOrigin::signed(caller.clone()),
			contract,
			value.into(),
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

	/// The deployer, the deployed contract, a registered person's address and the precompile's
	/// address.
	fn setup() -> (AccountId32, H160, Address, Address) {
		let alice = id_to_account(1);
		map_account::<Test>(&alice);
		let target = id_to_account(42);
		// The precompile maps the address argument back to an account, so the target needs a
		// mapping for the lookup to find what `set_personhood` stored.
		map_modestly(&target);
		set_personhood(&target, &test_context(), *PEOPLE_IDENTIFIER, BOB_ALIAS);
		let contract = deploy(&alice);
		let account = <Test as pallet_revive::Config>::AddressMapper::to_address(&target);
		(alice, contract, Address::from(account.0), Address::from(PRECOMPILE_ADDR.0))
	}

	/// A read-only frame serves the query.
	///
	/// Fails if a blanket read-only denial is ever added, which would break every `STATICCALL` a
	/// Solidity `view` caller makes.
	#[test]
	fn a_read_only_frame_serves_the_query() {
		let context = test_context();
		new_test_ext().execute_with(|| {
			let (alice, contract, account, personhood) = setup();

			let data = call_contract(
				&alice,
				contract,
				IPersonhoodCaller::readInStaticFrameCall {
					personhood,
					account,
					context: context.into(),
				}
				.abi_encode(),
				0,
			);
			let outcome =
				IPersonhoodCaller::readInStaticFrameCall::abi_decode_returns(&data).unwrap();
			assert!(outcome.ok, "a view must be served in a read-only frame");

			// The frame reports the live status, not an empty default.
			let info = IPersonhood::personhoodStatusCall::abi_decode_returns(&outcome.returnData)
				.expect("the read answers with an encoded PersonhoodInfo");
			assert_eq!(info.status as u8, FULL_STATUS);
			assert_eq!(info.contextAlias.0, BOB_ALIAS);
		});
	}

	/// A delegate call reverts, so the delegator keeps its gas and can catch the failure.
	#[test]
	fn a_delegate_call_is_refused() {
		let context = test_context();
		new_test_ext().execute_with(|| {
			let (alice, contract, account, personhood) = setup();

			let data = call_contract(
				&alice,
				contract,
				IPersonhoodCaller::readViaDelegateCallCall {
					personhood,
					account,
					context: context.into(),
				}
				.abi_encode(),
				0,
			);
			let outcome =
				IPersonhoodCaller::readViaDelegateCallCall::abi_decode_returns(&data).unwrap();
			assert!(!outcome.ok, "a delegate call must be refused");
			let revert = Revert::abi_decode(&outcome.returnData)
				.expect("the refusal reverts with a reason rather than trapping");
			assert_eq!(revert.reason, delegate_denied_reason());
		});
	}

	/// Value attached by a contract reverts, and the revert unwinds the transfer.
	#[test]
	fn a_call_carrying_value_is_refused() {
		let context = test_context();
		new_test_ext().execute_with(|| {
			let (alice, contract, account, personhood) = setup();
			let stranded = <Test as pallet_revive::Config>::AddressMapper::to_fallback_account_id(
				&PRECOMPILE_ADDR,
			);

			let data = call_contract(
				&alice,
				contract,
				IPersonhoodCaller::readWithValueCall {
					personhood,
					account,
					context: context.into(),
				}
				.abi_encode(),
				1_000,
			);
			let outcome = IPersonhoodCaller::readWithValueCall::abi_decode_returns(&data).unwrap();
			assert!(!outcome.ok, "a call carrying value must be refused");
			let revert = Revert::abi_decode(&outcome.returnData)
				.expect("the refusal reverts with a reason rather than trapping");
			assert_eq!(revert.reason, ERR_VALUE_NOT_ACCEPTED);
			assert_eq!(Balances::free_balance(&stranded), 0, "value stranded at the precompile");
		});
	}

	/// A read-only frame serves a proof verification, the interface's other view.
	///
	/// `a_read_only_frame_serves_the_query` covers `personhoodStatus`. A selector-gated read-only
	/// denial could deny this selector while leaving that one served, so it needs its own case.
	#[test]
	fn a_read_only_frame_serves_a_proof_verification() {
		let context = test_context();
		new_test_ext().execute_with(|| {
			let (alice, contract, _account, personhood) = setup();
			set_proof_result(ALICE_ALIAS, context, *PEOPLE_IDENTIFIER);

			let data = call_contract(
				&alice,
				contract,
				IPersonhoodCaller::verifyProofInStaticFrameCall {
					personhood,
					request: ProofVerificationRequest {
						expectedStatus: FULL_STATUS,
						proof: Vec::new().into(),
						expectedAlias: ALICE_ALIAS.into(),
						ringIndex: 0,
						context: context.into(),
						revision: 1,
						message: Vec::new().into(),
					},
				}
				.abi_encode(),
				0,
			);
			let outcome =
				IPersonhoodCaller::verifyProofInStaticFrameCall::abi_decode_returns(&data).unwrap();
			assert!(outcome.ok, "a read-only frame must serve a view");

			// The frame answers with the verification result, not an empty default.
			let verified =
				IPersonhood::personhoodInfoByProofCall::abi_decode_returns(&outcome.returnData)
					.expect("the verification answers with an encoded bool");
			assert!(verified, "the proof attests to the claimed tier");
		});
	}
}
