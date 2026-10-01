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

use crate::{mock::*, Call, CryptoOf, WeightInfo};
use codec::Encode;
use frame_support::{
	assert_ok,
	dispatch::{DispatchClass, GetDispatchInfo},
	traits::Hooks,
	weights::Weight,
};
use indiv_support::weight_budget::OcwWeightBudget;
use verifiable::GenerateVerifiable;

/// Verify that the default mock configuration passes all integrity checks.
#[test]
fn integrity_test_passes() {
	new_test_ext().execute_with(|| {
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

#[test]
fn mixed_output_weight_can_use_the_normal_extrinsic_limit() {
	new_test_ext().execute_with(|| {
		let limit = MockBlockWeights::get().get(DispatchClass::Normal).max_extrinsic.unwrap();
		let base = limit.saturating_div(4).saturating_mul(3);
		MockMixedOutputWeightBases::set(&Some((base, base)));
		let loaded_coins = (0..MAX_SPLIT_OUTPUTS)
			.map(|i| {
				let secret = CryptoOf::<Test>::new_secret([i as u8; 32]);
				(-2 + (i % 10) as i8, CryptoOf::<Test>::member_from_secret(&secret))
			})
			.collect::<Vec<_>>();
		let loaded_value = loaded_coins
			.iter()
			.map(|(value, _)| 250 * 2u64.pow((*value + 2) as u32))
			.sum::<u64>();
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
			external_asset_amount: u64::from(MAX_CONSOLIDATION) * 128_000 - loaded_value,
			loaded_coins: loaded_coins.try_into().unwrap(),
			max_fee: 0,
		};
		for extension_weight in [
			TestWeightInfo::as_unload_token_people_tx_ext(),
			TestWeightInfo::as_unload_token_lite_people_tx_ext(),
			TestWeightInfo::as_unload_token_paid_tx_ext(),
			TestWeightInfo::as_unload_token_from_output_tx_ext(),
		] {
			let mut info = call.get_dispatch_info();
			info.extension_weight = extension_weight
				.saturating_add(TestWeightInfo::validate_unload_calls(1, MAX_SPLIT_OUTPUTS));
			assert_eq!(info.class, DispatchClass::Normal);
			assert!(info.total_weight().all_gt(limit.saturating_div(2)));
			assert_ok!(frame_system::CheckWeight::<Test>::do_validate(&info, call.encoded_size()));
		}
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

#[test]
#[should_panic(expected = "exceeds the Normal extrinsic budget")]
fn mixed_output_integrity_rejects_excess_execution_time() {
	new_test_ext().execute_with(|| {
		let limit = MockBlockWeights::get().get(DispatchClass::Normal).max_extrinsic.unwrap();
		let base = Weight::from_parts(limit.ref_time(), 0);
		MockMixedOutputWeightBases::set(&Some((base, base)));
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

#[test]
#[should_panic(expected = "exceeds the Normal extrinsic budget")]
fn mixed_output_integrity_rejects_excess_proof_size() {
	new_test_ext().execute_with(|| {
		let limit = MockBlockWeights::get().get(DispatchClass::Normal).max_extrinsic.unwrap();
		let base = Weight::from_parts(0, limit.proof_size());
		MockMixedOutputWeightBases::set(&Some((base, base)));
		<crate::Pallet<Test> as Hooks<u64>>::integrity_test();
	});
}

#[test]
#[should_panic(expected = "exceeds the OCW budget")]
fn ocw_cleanup_keeps_its_reserved_budget() {
	new_test_ext().execute_with(|| {
		let limit = MockBlockWeights::get().get(DispatchClass::Normal).max_extrinsic.unwrap();
		OcwWeightBudget::from_normal_max::<Test>()
			.assert_fits("cleanup", limit.saturating_div(4).saturating_mul(3));
	});
}
