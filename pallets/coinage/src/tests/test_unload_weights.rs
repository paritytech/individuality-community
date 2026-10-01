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

//! The unload weight helpers charge each benchmarked alias count its own sample.
//!
//! The interpolation itself is covered in `weight_interpolation`; these tests check that every
//! helper wires the right `WeightInfo` function to each sampled count.

use crate::{mock::*, Config, Pallet, UnloadWeightScope, WeightInfo};
use frame_support::weights::Weight;

type W = <Test as Config>::WeightInfo;

const MAX_ALIASES: u32 = MAX_CONSOLIDATION;
/// Loaded coins and destinations, held fixed across the alias samples.
const D: u32 = 3;

#[test]
fn unload_scope_selects_the_requested_fee_modes_without_deposit_surcharges() {
	new_test_ext().execute_with(|| {
		for outputs in [1, D, MAX_SPLIT_OUTPUTS] {
			let from_output = W::unload_recycler_into_external_asset_from_output_1()
				.max(W::unload_recycler_into_external_asset_and_loaded_coins_from_output_1(outputs))
				.max(W::unload_recycler_into_coins_from_output_1(outputs));
			let any_mode = from_output
				.max(W::unload_recycler_into_coin_1())
				.max(W::unload_recycler_into_external_asset_prepaid_1())
				.max(W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_1(outputs))
				.max(W::unload_recycler_into_coins_prepaid_1(outputs))
				.max(W::unload_recycler_into_external_asset_non_anonymous_1());
			// The fixture must distinguish the scopes, so swapping them fails this test.
			assert_ne!(from_output, any_mode);
			assert_eq!(
				Pallet::<Test>::max_unload_call_weight(
					UnloadWeightScope::FromOutputOnly,
					1,
					outputs
				),
				from_output
			);
			assert_eq!(
				Pallet::<Test>::max_unload_call_weight(UnloadWeightScope::AnyFeeMode, 1, outputs),
				any_mode
			);
		}
	});
}

#[test]
fn output_fee_weight_adds_extension_and_both_deposit_surcharges_once() {
	new_test_ext().execute_with(|| {
		let aliases = MAX_CONSOLIDATION as usize;
		let outputs = MAX_SPLIT_OUTPUTS;
		let call = Pallet::<Test>::unload_recycler_into_external_asset_from_output_weight(aliases)
			.max(Pallet::<Test>::unload_recycler_into_external_asset_and_loaded_coins_from_output_weight(
				aliases, outputs as usize,
			))
			.max(Pallet::<Test>::unload_recycler_into_coins_from_output_weight(aliases, outputs));
		assert!(W::settle_load_deposits().all_gt(Weight::zero()));
		assert!(W::charge_load_deposit().all_gt(Weight::zero()));
		let expected = call
			.saturating_add(W::as_unload_token_from_output_tx_ext())
			.saturating_add(W::validate_unload_calls(1, outputs))
			.saturating_add(W::settle_load_deposits())
			.saturating_add(W::charge_load_deposit());
		assert_eq!(Pallet::<Test>::weight_for_unload_recycler_paying_using_output(), expected);
	});
}

#[test]
fn sample_bounds_follow_the_mock_config() {
	new_test_ext().execute_with(|| {
		assert_eq!(Pallet::<Test>::max_aliases_per_unload(), MAX_ALIASES);
		assert_eq!(Pallet::<Test>::max_aliases_per_coin_unload(), MAX_ALIASES);
	});
}

/// Asserts `helper(count)` returns `sample` at each `(count, sample)`.
fn assert_samples(helper: impl Fn(u32) -> Weight, samples: [(u32, Weight); 7]) {
	for (count, _) in samples {
		let combined = samples
			.iter()
			.filter(|(other_count, _)| *other_count == count)
			.fold(Weight::zero(), |weight, (_, sample)| weight.max(*sample));
		assert_eq!(helper(count), combined, "alias count {count}");
	}
}

#[test]
fn single_recycler_unloads_charge_their_sample() {
	new_test_ext().execute_with(|| {
		assert_samples(
			|n| Pallet::<Test>::unload_recycler_into_coin_weight(n as usize),
			[
				(1, W::unload_recycler_into_coin_1()),
				(2, W::unload_recycler_into_coin_2()),
				(4, W::unload_recycler_into_coin_4()),
				(8, W::unload_recycler_into_coin_8()),
				(16.min(MAX_ALIASES), W::unload_recycler_into_coin_16()),
				(32.min(MAX_ALIASES), W::unload_recycler_into_coin_32()),
				(MAX_ALIASES, W::unload_recycler_into_coin_max()),
			],
		);
		assert_samples(
			|n| Pallet::<Test>::unload_recycler_into_external_asset_prepaid_weight(n as usize),
			[
				(1, W::unload_recycler_into_external_asset_prepaid_1()),
				(2, W::unload_recycler_into_external_asset_prepaid_2()),
				(4, W::unload_recycler_into_external_asset_prepaid_4()),
				(8, W::unload_recycler_into_external_asset_prepaid_8()),
				(16.min(MAX_ALIASES), W::unload_recycler_into_external_asset_prepaid_16()),
				(32.min(MAX_ALIASES), W::unload_recycler_into_external_asset_prepaid_32()),
				(MAX_ALIASES, W::unload_recycler_into_external_asset_prepaid_max()),
			],
		);
		assert_samples(
			|n| Pallet::<Test>::unload_recycler_into_external_asset_from_output_weight(n as usize),
			[
				(1, W::unload_recycler_into_external_asset_from_output_1()),
				(2, W::unload_recycler_into_external_asset_from_output_2()),
				(4, W::unload_recycler_into_external_asset_from_output_4()),
				(8, W::unload_recycler_into_external_asset_from_output_8()),
				(16.min(MAX_ALIASES), W::unload_recycler_into_external_asset_from_output_16()),
				(32.min(MAX_ALIASES), W::unload_recycler_into_external_asset_from_output_32()),
				(MAX_ALIASES, W::unload_recycler_into_external_asset_from_output_max()),
			],
		);
		assert_samples(
			|n| {
				Pallet::<Test>::unload_recycler_into_external_asset_non_anonymous_weight(n as usize)
			},
			[
				(1, W::unload_recycler_into_external_asset_non_anonymous_1()),
				(2, W::unload_recycler_into_external_asset_non_anonymous_2()),
				(4, W::unload_recycler_into_external_asset_non_anonymous_4()),
				(8, W::unload_recycler_into_external_asset_non_anonymous_8()),
				(16.min(MAX_ALIASES), W::unload_recycler_into_external_asset_non_anonymous_16()),
				(32.min(MAX_ALIASES), W::unload_recycler_into_external_asset_non_anonymous_32()),
				(MAX_ALIASES, W::unload_recycler_into_external_asset_non_anonymous_max()),
			],
		);
	});
}

#[test]
fn unloads_with_outputs_charge_their_sample() {
	new_test_ext().execute_with(|| {
		assert_samples(
			|a| {
				Pallet::<Test>::unload_recycler_into_external_asset_and_loaded_coins_prepaid_weight(
					a as usize, D as usize,
				)
			},
			[
				(1, W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_1(D)),
				(2, W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_2(D)),
				(4, W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_4(D)),
				(8, W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_8(D)),
				(
					16.min(MAX_ALIASES),
					W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_16(D),
				),
				(
					32.min(MAX_ALIASES),
					W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_32(D),
				),
				(
					MAX_ALIASES,
					W::unload_recycler_into_external_asset_and_loaded_coins_prepaid_max(D),
				),
			],
		);
		assert_samples(
			|a| {
				Pallet::<Test>::unload_recycler_into_external_asset_and_loaded_coins_from_output_weight(
					a as usize, D as usize,
				)
			},
			[
				(1, W::unload_recycler_into_external_asset_and_loaded_coins_from_output_1(D)),
				(2, W::unload_recycler_into_external_asset_and_loaded_coins_from_output_2(D)),
				(4, W::unload_recycler_into_external_asset_and_loaded_coins_from_output_4(D)),
				(8, W::unload_recycler_into_external_asset_and_loaded_coins_from_output_8(D)),
				(
					16.min(MAX_ALIASES),
					W::unload_recycler_into_external_asset_and_loaded_coins_from_output_16(D),
				),
				(
					32.min(MAX_ALIASES),
					W::unload_recycler_into_external_asset_and_loaded_coins_from_output_32(D),
				),
				(
					MAX_ALIASES,
					W::unload_recycler_into_external_asset_and_loaded_coins_from_output_max(D),
				),
			],
		);
		assert_samples(
			|a| Pallet::<Test>::unload_recycler_into_coins_prepaid_weight(a as usize, D),
			[
				(1, W::unload_recycler_into_coins_prepaid_1(D)),
				(2, W::unload_recycler_into_coins_prepaid_2(D)),
				(4, W::unload_recycler_into_coins_prepaid_4(D)),
				(8, W::unload_recycler_into_coins_prepaid_8(D)),
				(16.min(MAX_ALIASES), W::unload_recycler_into_coins_prepaid_16(D)),
				(32.min(MAX_ALIASES), W::unload_recycler_into_coins_prepaid_32(D)),
				(MAX_ALIASES, W::unload_recycler_into_coins_prepaid_max(D)),
			],
		);
		assert_samples(
			|a| Pallet::<Test>::unload_recycler_into_coins_from_output_weight(a as usize, D),
			[
				(1, W::unload_recycler_into_coins_from_output_1(D)),
				(2, W::unload_recycler_into_coins_from_output_2(D)),
				(4, W::unload_recycler_into_coins_from_output_4(D)),
				(8, W::unload_recycler_into_coins_from_output_8(D)),
				(16.min(MAX_ALIASES), W::unload_recycler_into_coins_from_output_16(D)),
				(32.min(MAX_ALIASES), W::unload_recycler_into_coins_from_output_32(D)),
				(MAX_ALIASES, W::unload_recycler_into_coins_from_output_max(D)),
			],
		);
	});
}

#[test]
fn multi_recycler_unload_charges_its_sample_per_recycler_count() {
	new_test_ext().execute_with(|| {
		assert_samples(
			Pallet::<Test>::unload_recyclers_into_external_asset_non_anonymous_weight,
			[
				(1, W::unload_recyclers_into_external_asset_non_anonymous_1()),
				(2, W::unload_recyclers_into_external_asset_non_anonymous_2()),
				(4, W::unload_recyclers_into_external_asset_non_anonymous_4()),
				(8, W::unload_recyclers_into_external_asset_non_anonymous_8()),
				(
					16.min(MAX_CONSOLIDATION),
					W::unload_recyclers_into_external_asset_non_anonymous_16(),
				),
				(
					32.min(MAX_CONSOLIDATION),
					W::unload_recyclers_into_external_asset_non_anonymous_32(),
				),
				(MAX_CONSOLIDATION, W::unload_recyclers_into_external_asset_non_anonymous_max()),
			],
		);
	});
}

#[test]
fn multi_recycler_maximum_uses_max_consolidation() {
	new_test_ext().execute_with(|| {
		assert_eq!(
			Pallet::<Test>::unload_recyclers_into_external_asset_non_anonymous_weight(
				MAX_CONSOLIDATION
			),
			W::unload_recyclers_into_external_asset_non_anonymous_max(),
		);
	});
}

#[test]
fn counts_between_samples_are_interpolated_not_clamped() {
	new_test_ext().execute_with(|| {
		// Each bound is the helper's own charge at a sampled coordinate. `MAX_ALIASES` is 16, so
		// the `32` and maximum samples clamp onto the `16` coordinate and the charge there
		// combines all three. `assert_samples` pins every coordinate to its `WeightInfo` samples.
		let between = |lo: Weight, mid: Weight, hi: Weight| {
			assert!(mid.all_gt(lo), "{mid:?} is not above {lo:?}");
			assert!(mid.all_lt(hi), "{mid:?} is not below {hi:?}");
		};
		let prepaid =
			|n: u32| Pallet::<Test>::unload_recycler_into_external_asset_prepaid_weight(n as usize);
		let coins =
			|n: u32| Pallet::<Test>::unload_recycler_into_coins_prepaid_weight(n as usize, D);
		let recyclers = Pallet::<Test>::unload_recyclers_into_external_asset_non_anonymous_weight;

		// 12 aliases lie between the `8` and `16` coordinates.
		between(prepaid(8), prepaid(12), prepaid(16));
		between(coins(8), coins(12), coins(16));
		// 6 recyclers lie between the `4` and `8` coordinates.
		between(recyclers(4), recyclers(6), recyclers(8));

		// Halfway between two coordinates is their mean, rounded up.
		let (lo, hi) = (prepaid(8), prepaid(16));
		let mean = Weight::from_parts(
			(lo.ref_time() + hi.ref_time()).div_ceil(2),
			(lo.proof_size() + hi.proof_size()).div_ceil(2),
		);
		assert_eq!(prepaid(12), mean);
	});
}
