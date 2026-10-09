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

use crate::{mock::*, Call, Config, CryptoOf, Pallet, UnloadWeightScope, WeightInfo};
use codec::Encode;
use frame_support::{
	assert_ok,
	dispatch::{DispatchClass, GetDispatchInfo},
	traits::Hooks,
	weights::Weight,
};
use verifiable::GenerateVerifiable;

/// Verify that the default mock configuration passes all integrity checks.
#[test]
fn integrity_test_passes() {
	new_test_ext().execute_with(|| {
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

type W = <Test as Config>::WeightInfo;

/// Sets the Normal `max_extrinsic` limit that `integrity_test` checks unloads against.
fn set_normal_max_extrinsic(limit: Weight) {
	let mut block_weights = MockBlockWeights::get();
	block_weights.per_class.get_mut(DispatchClass::Normal).max_extrinsic = Some(limit);
	MockBlockWeights::set(&block_weights);
}

/// Worst-case unload weight over `scope`, as `integrity_test` checks it.
fn max_unload_weight(scope: UnloadWeightScope) -> Weight {
	Pallet::<Test>::max_unload_call_weight(scope, MAX_CONSOLIDATION as usize, MAX_SPLIT_OUTPUTS)
		.saturating_add(Pallet::<Test>::unload_surcharge_weight(scope))
}

/// Worst-case `clean_recycler` weight, as `integrity_test` checks it against the OCW budget.
fn max_clean_recycler_weight() -> Weight {
	let capacity = TestRecyclerRingExponent::get().ring_capacity();
	W::clean_recycler(capacity, capacity)
		.saturating_add(W::authorize_clean_recycler())
		.saturating_add(W::settle_load_deposits())
}

#[test]
fn mixed_output_weight_can_use_the_normal_extrinsic_limit() {
	new_test_ext().execute_with(|| {
		set_normal_max_extrinsic(max_unload_weight(UnloadWeightScope::AnyFeeMode));
		let loaded_coins = (0..MAX_SPLIT_OUTPUTS)
			.map(|i| {
				let secret = CryptoOf::<Test>::new_secret([i as u8; 32]);
				(-2 + (i % DENOMINATION_COUNT) as i8, CryptoOf::<Test>::member_from_secret(&secret))
			})
			.collect::<Vec<_>>();
		let call = Call::<Test>::unload_recycler_into_external_asset_and_loaded_coins {
			instance_id: TEST_INSTANCE_ID,
			aliases: (0..MAX_CONSOLIDATION)
				.map(|i| [i as u8; 32])
				.collect::<Vec<_>>()
				.try_into()
				.unwrap(),
			value: 7,
			index: 0,
			revision: 0,
			to: BOB,
			external_asset_amount: 0,
			loaded_coins: loaded_coins.try_into().unwrap(),
			max_fee: 0,
		};
		for extension_weight in [
			W::as_unload_token_people_tx_ext(),
			W::as_unload_token_lite_people_tx_ext(),
			W::as_unload_token_paid_tx_ext(),
			W::as_unload_token_from_output_tx_ext(),
		] {
			let mut info = call.get_dispatch_info();
			info.extension_weight =
				extension_weight.saturating_add(W::validate_unload_calls(1, MAX_SPLIT_OUTPUTS));
			assert_eq!(info.class, DispatchClass::Normal);
			assert_ok!(frame_system::CheckWeight::<Test>::do_validate(&info, call.encoded_size()));
		}
	});
}

/// Only the prepaid path exceeds the limit, so checking the `FromOutput` paths alone passes.
#[test]
#[should_panic(expected = "exceeds the Normal extrinsic budget")]
fn mixed_output_integrity_rejects_excess_execution_time() {
	new_test_ext().execute_with(|| {
		let any_fee_mode = max_unload_weight(UnloadWeightScope::AnyFeeMode);
		let from_output = max_unload_weight(UnloadWeightScope::FromOutputOnly);
		assert!(from_output.ref_time() < any_fee_mode.ref_time());
		set_normal_max_extrinsic(Weight::from_parts(
			from_output.ref_time(),
			any_fee_mode.proof_size(),
		));
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

#[test]
#[should_panic(expected = "exceeds the Normal extrinsic budget")]
fn mixed_output_integrity_rejects_excess_proof_size() {
	new_test_ext().execute_with(|| {
		let weight = max_unload_weight(UnloadWeightScope::AnyFeeMode);
		set_normal_max_extrinsic(Weight::from_parts(weight.ref_time(), weight.proof_size() - 1));
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

/// Only the most diverse split exceeds the limit.
#[test]
#[should_panic(expected = "exceeds the Normal extrinsic budget")]
fn mixed_output_integrity_checks_every_denomination() {
	new_test_ext().execute_with(|| {
		set_normal_max_extrinsic(
			Pallet::<Test>::unload_recycler_into_external_asset_and_loaded_coins_max_weight(
				MAX_CONSOLIDATION as usize,
				DENOMINATION_COUNT - 1,
				MAX_SPLIT_OUTPUTS,
			)
			.saturating_add(Pallet::<Test>::unload_surcharge_weight(UnloadWeightScope::AnyFeeMode)),
		);
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

#[test]
#[should_panic(expected = "MaxSplitOutputs must not exceed the onboarding queue page size")]
fn integrity_rejects_split_outputs_above_the_page_size() {
	new_test_ext().execute_with(|| {
		OnboardingQueuePageSize::set(&(MAX_SPLIT_OUTPUTS - 1));
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

/// The unload weight equals the limit in `ref_time`. The `proof_size` limit stays at the mock
/// default, so the OCW cleanups fit half of it.
#[test]
fn mixed_output_integrity_accepts_the_exact_limit() {
	new_test_ext().execute_with(|| {
		let weight = max_unload_weight(UnloadWeightScope::AnyFeeMode);
		let default = MockBlockWeights::get().get(DispatchClass::Normal).max_extrinsic.unwrap();
		assert!(weight.ref_time() < default.ref_time());
		set_normal_max_extrinsic(Weight::from_parts(weight.ref_time(), default.proof_size()));
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

/// The unload fits the limit, but `clean_recycler` exceeds half of it.
#[test]
#[should_panic(expected = "`clean_recycler` worst-case weight")]
fn ocw_cleanup_keeps_its_reserved_budget() {
	new_test_ext().execute_with(|| {
		let limit = max_unload_weight(UnloadWeightScope::AnyFeeMode);
		assert!(!max_clean_recycler_weight().all_lte(limit.saturating_div(2)));
		set_normal_max_extrinsic(limit);
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}
