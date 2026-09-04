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

use crate::{mock::*, Config, Pallet, WeightInfo};
use frame_support::weights::Weight;

type W = <Test as Config>::WeightInfo;

const MAX_ALIASES: u32 = MAX_CONSOLIDATION;
/// The mock has ten denominations, fewer than `MAX_CONSOLIDATION`.
const MAX_RECYCLERS: u32 = 10;
/// Loaded coins and destinations, held fixed across the alias samples.
const D: u32 = 3;

#[test]
fn sample_bounds_follow_the_mock_config() {
	new_test_ext().execute_with(|| {
		assert_eq!(Pallet::<Test>::max_aliases_per_unload(), MAX_ALIASES);
		assert_eq!(Pallet::<Test>::max_aliases_per_coin_unload(), MAX_ALIASES);
		assert_eq!(Pallet::<Test>::max_recyclers_per_unload(), MAX_RECYCLERS);
	});
}

/// Asserts `helper(count)` returns `sample` at each `(count, sample)`.
fn assert_samples(helper: impl Fn(u32) -> Weight, samples: [(u32, Weight); 5]) {
	for (count, sample) in samples {
		assert_eq!(helper(count), sample, "alias count {count}");
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
				(MAX_RECYCLERS, W::unload_recyclers_into_external_asset_non_anonymous_max()),
			],
		);
	});
}

#[test]
fn more_aliases_than_recyclers_extend_the_last_segment() {
	new_test_ext().execute_with(|| {
		// `MaxConsolidation` aliases over `MAX_RECYCLERS` recyclers is a valid call, so the
		// weight must keep growing past the last sample.
		let at_max = Pallet::<Test>::unload_recyclers_into_external_asset_non_anonymous_weight(
			MAX_RECYCLERS,
		);
		let above = Pallet::<Test>::unload_recyclers_into_external_asset_non_anonymous_weight(
			MAX_CONSOLIDATION,
		);
		assert!(above.all_gte(at_max));
		assert_ne!(above, at_max);
	});
}

#[test]
fn counts_between_samples_are_interpolated_not_clamped() {
	new_test_ext().execute_with(|| {
		// 12 aliases lie between the `8` and `max` (16) samples of the mock.
		let between = |lo: Weight, mid: Weight, hi: Weight| {
			assert!(mid.all_gt(lo), "{mid:?} is not above {lo:?}");
			assert!(mid.all_lt(hi), "{mid:?} is not below {hi:?}");
		};
		between(
			W::unload_recycler_into_external_asset_prepaid_8(),
			Pallet::<Test>::unload_recycler_into_external_asset_prepaid_weight(12),
			W::unload_recycler_into_external_asset_prepaid_max(),
		);
		between(
			W::unload_recycler_into_coins_prepaid_8(D),
			Pallet::<Test>::unload_recycler_into_coins_prepaid_weight(12, D),
			W::unload_recycler_into_coins_prepaid_max(D),
		);
		// 6 recyclers lie between the `4` and `8` samples.
		between(
			W::unload_recyclers_into_external_asset_non_anonymous_4(),
			Pallet::<Test>::unload_recyclers_into_external_asset_non_anonymous_weight(6),
			W::unload_recyclers_into_external_asset_non_anonymous_8(),
		);
		// Halfway between two samples is their mean, rounded up.
		let lo = W::unload_recycler_into_external_asset_prepaid_8();
		let hi = W::unload_recycler_into_external_asset_prepaid_max();
		let mean = Weight::from_parts(
			(lo.ref_time() + hi.ref_time()).div_ceil(2),
			(lo.proof_size() + hi.proof_size()).div_ceil(2),
		);
		assert_eq!(Pallet::<Test>::unload_recycler_into_external_asset_prepaid_weight(12), mean);
	});
}
