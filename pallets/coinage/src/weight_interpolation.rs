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

//! Weights of the unload calls, interpolated between benchmarked alias counts.
//!
//! Ring-VRF batch verification is sublinear in the proof count, so one linear fit over the
//! whole alias range misprices small batches. Each unload call is therefore benchmarked at the
//! fixed alias counts 1, 2, 4, 8, 16, 32 and its maximum, with no `Linear` component for the count,
//! and the weight of any other count is interpolated linearly between the two nearest samples.

use frame_support::weights::Weight;

/// The weight of an unload call for `alias_count` aliases, from its seven fixed benchmarks.
///
/// `max` is the count of the final benchmark and must be at least 8, which the pallet's
/// `integrity_test` checks. Samples above `max` share its coordinate; their component-wise
/// maximum is the charge at that coordinate. Above `max`, the final distinct segment is extended
/// and floored at the combined maximum-coordinate weight.
pub(crate) fn interpolate_unload_weight(
	alias_count: u32,
	max: u32,
	weights: [Weight; 7],
) -> Weight {
	let coordinates = [1, 2, 4, 8, 16.min(max), 32.min(max), max];
	interpolate_weight(alias_count, coordinates, weights)
}

/// Piecewise-linear interpolation of seven ordered sample coordinates and weights.
///
/// Below the first point the first weight is returned. Above the last point the last segment
/// is extended, component-wise never below the combined final weight. Equal coordinates are
/// combined first so every sample at a clamped count contributes to the charge. Each `Weight`
/// component is interpolated on its own and rounded up.
fn interpolate_weight(x: u32, coordinates: [u32; 7], weights: [Weight; 7]) -> Weight {
	let mut distinct_coordinates = [0; 7];
	let mut distinct_weights = [Weight::zero(); 7];
	let mut distinct_len = 0;

	for index in 0..coordinates.len() {
		if distinct_len > 0 && coordinates[index] == distinct_coordinates[distinct_len - 1] {
			distinct_weights[distinct_len - 1] =
				distinct_weights[distinct_len - 1].max(weights[index]);
		} else {
			distinct_coordinates[distinct_len] = coordinates[index];
			distinct_weights[distinct_len] = weights[index];
			distinct_len += 1;
		}
	}

	if x <= distinct_coordinates[0] {
		return distinct_weights[0];
	}

	for index in 1..distinct_len {
		if x <= distinct_coordinates[index] {
			if x == distinct_coordinates[index] {
				return distinct_weights[index];
			}
			return interpolate_between(
				x,
				distinct_coordinates[index - 1],
				distinct_coordinates[index],
				distinct_weights[index - 1],
				distinct_weights[index],
			);
		}
	}

	interpolate_between(
		x,
		distinct_coordinates[distinct_len - 2],
		distinct_coordinates[distinct_len - 1],
		distinct_weights[distinct_len - 2],
		distinct_weights[distinct_len - 1],
	)
	.max(distinct_weights[distinct_len - 1])
}

fn interpolate_between(x: u32, x_lo: u32, x_hi: u32, w_lo: Weight, w_hi: Weight) -> Weight {
	Weight::from_parts(
		interpolate_component(x, x_lo, x_hi, w_lo.ref_time(), w_hi.ref_time()),
		interpolate_component(x, x_lo, x_hi, w_lo.proof_size(), w_hi.proof_size()),
	)
}

/// `y` at `x` on the line through `(x_lo, y_lo)` and `(x_hi, y_hi)`, rounded up.
///
/// `x` may lie past `x_hi`, in which case the line is extended and a falling line saturates at
/// zero. Two points at the same `x` yield the larger `y`.
fn interpolate_component(x: u32, x_lo: u32, x_hi: u32, y_lo: u64, y_hi: u64) -> u64 {
	let span = u128::from(x_hi.saturating_sub(x_lo));
	if span == 0 {
		return y_lo.max(y_hi);
	}
	let offset = u128::from(x.saturating_sub(x_lo));
	if y_hi >= y_lo {
		let rise = u128::from(y_hi - y_lo).saturating_mul(offset).div_ceil(span);
		y_lo.saturating_add(u64::try_from(rise).unwrap_or(u64::MAX))
	} else {
		let fall = u128::from(y_lo - y_hi).saturating_mul(offset) / span;
		y_lo.saturating_sub(u64::try_from(fall).unwrap_or(u64::MAX))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn weight(ref_time: u64, proof_size: u64) -> Weight {
		Weight::from_parts(ref_time, proof_size)
	}

	fn samples() -> [Weight; 7] {
		[
			weight(100, 1_000),
			weight(150, 1_200),
			weight(220, 1_600),
			weight(340, 2_400),
			weight(500, 4_000),
			weight(700, 7_000),
			weight(1_100, 10_000),
		]
	}

	#[test]
	fn sampled_counts_return_their_sample() {
		for (count, expected) in
			[(1, samples()[0]), (2, samples()[1]), (4, samples()[2]), (8, samples()[3])]
		{
			assert_eq!(interpolate_unload_weight(count, 64, samples()), expected);
		}
	}

	#[test]
	fn counts_between_samples_interpolate_each_component() {
		// Halfway between 2 and 4.
		assert_eq!(interpolate_unload_weight(3, 64, samples()), weight(185, 1_400));
		// A quarter of the way from 4 to 8 on ref_time (220 + 120 / 4) and proof_size
		// (1600 + 800 / 4).
		assert_eq!(interpolate_unload_weight(5, 64, samples()), weight(250, 1_800));
		// Halfway between 8 and 16.
		assert_eq!(interpolate_unload_weight(12, 64, samples()), weight(420, 3_200));
	}

	#[test]
	fn interpolation_rounds_up() {
		let points = [1, 2, 4, 8, 16, 32, 64];
		let weights = [
			weight(0, 0),
			weight(0, 0),
			weight(10, 20),
			weight(11, 23),
			weight(11, 23),
			weight(11, 23),
			weight(11, 23),
		];
		// 10 + 1 / 4 and 20 + 3 / 4, both rounded up.
		assert_eq!(interpolate_weight(5, points, weights), weight(11, 21));
	}

	#[test]
	fn zero_aliases_charge_the_first_sample() {
		assert_eq!(interpolate_unload_weight(0, 64, samples()), samples()[0]);
	}

	#[test]
	fn counts_above_max_extend_the_last_segment() {
		// From 32 to 64 ref_time rises by 400 and proof_size by 3_000, so extending by half
		// that segment adds 200 and 1_500 respectively.
		assert_eq!(interpolate_unload_weight(80, 64, samples()), weight(1_300, 11_500));
	}

	#[test]
	fn a_falling_last_segment_never_drops_below_max() {
		let falling = [
			weight(10, 10),
			weight(10, 10),
			weight(10, 10),
			weight(100, 100),
			weight(90, 120),
			weight(90, 120),
			weight(90, 120),
		];
		// Inside the segment the measured fall is followed.
		assert_eq!(interpolate_unload_weight(12, 16, falling), weight(95, 110));
		// Past it, ref_time holds at the `Max` sample while proof_size keeps rising.
		assert_eq!(interpolate_unload_weight(24, 16, falling), weight(90, 140));
		assert_eq!(interpolate_unload_weight(1_000, 16, falling), weight(90, 2_580));
	}

	#[test]
	fn max_equal_to_eight_charges_the_larger_sample_above_it() {
		let at_eight = [
			weight(10, 10),
			weight(10, 10),
			weight(10, 10),
			weight(100, 300),
			weight(120, 250),
			weight(110, 350),
			weight(115, 275),
		];
		assert_eq!(interpolate_unload_weight(8, 8, at_eight), weight(120, 350));
		assert_eq!(interpolate_unload_weight(9, 8, at_eight), weight(148, 435));
	}

	#[test]
	fn interpolation_is_monotonic_over_growing_samples() {
		let mut previous = Weight::zero();
		for count in 0..40 {
			let current = interpolate_unload_weight(count, 64, samples());
			assert!(current.all_gte(previous), "weight fell from {previous:?} to {current:?}");
			previous = current;
		}
	}

	#[test]
	fn extreme_values_saturate() {
		let huge = [
			weight(0, 0),
			weight(0, 0),
			weight(0, 0),
			weight(0, 0),
			weight(u64::MAX, u64::MAX),
			weight(u64::MAX, u64::MAX),
			weight(u64::MAX, u64::MAX),
		];
		assert_eq!(interpolate_unload_weight(u32::MAX, 16, huge), weight(u64::MAX, u64::MAX));
	}

	#[test]
	fn clamped_coordinates_combine_each_weight_component() {
		let weights = [
			weight(1, 1),
			weight(2, 2),
			weight(4, 4),
			weight(80, 800),
			weight(100, 700),
			weight(90, 900),
			weight(95, 750),
		];
		assert_eq!(interpolate_unload_weight(12, 16, weights), weight(90, 850));
		assert_eq!(interpolate_unload_weight(16, 16, weights), weight(100, 900));
		assert_eq!(interpolate_unload_weight(17, 16, weights), weight(103, 913));
	}

	#[test]
	fn all_supported_maxima_have_ordered_effective_samples() {
		for max in [8, 16, 32, 64, 19] {
			assert_eq!(
				interpolate_unload_weight(max, max, samples()),
				match max {
					8 => samples()[3].max(samples()[4]).max(samples()[5]).max(samples()[6]),
					16 => samples()[4].max(samples()[5]).max(samples()[6]),
					32 => samples()[5].max(samples()[6]),
					_ => samples()[6],
				}
			);
		}
	}
}
