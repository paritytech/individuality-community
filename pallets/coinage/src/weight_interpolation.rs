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
//! fixed alias counts 1, 2, 4, 8 and its maximum, with no `Linear` component for the count, and
//! the weight of any other count is interpolated linearly between the two nearest samples.

use frame_support::weights::Weight;

/// The alias counts at which an unload call is benchmarked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AliasCountSample {
	One,
	Two,
	Four,
	Eight,
	/// The most aliases the call accepts; each call's weight helper says which bound.
	Max,
}

impl AliasCountSample {
	const ALL: [Self; 5] = [Self::One, Self::Two, Self::Four, Self::Eight, Self::Max];

	fn alias_count(self, max: u32) -> u32 {
		match self {
			Self::One => 1,
			Self::Two => 2,
			Self::Four => 4,
			Self::Eight => 8,
			Self::Max => max,
		}
	}
}

/// The weight of an unload call for `alias_count` aliases, from the call's benchmarks at each
/// [`AliasCountSample`].
///
/// `max` is the count the `Max` sample was benchmarked at and must be at least 8, which the
/// pallet's `integrity_test` checks. Above `max` the last segment is extended and never drops
/// below the `Max` sample, so a call with more aliases than the samples cover is not
/// under-charged: `unload_recyclers_into_external_asset_non_anonymous` is sampled per recycler
/// but charged per alias, and its aliases can outnumber its recyclers.
pub(crate) fn interpolate_unload_weight(
	alias_count: u32,
	max: u32,
	weight_at: impl Fn(AliasCountSample) -> Weight,
) -> Weight {
	let points = AliasCountSample::ALL.map(|sample| (sample.alias_count(max), weight_at(sample)));
	interpolate_weight(alias_count, &points)
}

/// Piecewise-linear interpolation of `points`, given in increasing `x` order.
///
/// Below the first point the first weight is returned. Above the last point the last segment
/// is extended, component-wise never below the last weight. Each `Weight` component is
/// interpolated on its own and rounded up.
fn interpolate_weight(x: u32, points: &[(u32, Weight)]) -> Weight {
	let [(first_x, first_weight), .., (_, last_weight)] = points else {
		frame_support::defensive!("interpolation needs at least two points");
		return points.first().map_or(Weight::zero(), |(_, weight)| *weight);
	};
	if x <= *first_x {
		return *first_weight;
	}
	// The segment `x` falls in, or the last one when `x` is past every point.
	let Some([(x_lo, w_lo), (x_hi, w_hi)]) = points
		.windows(2)
		.find(|pair| x <= pair[1].0)
		.or_else(|| points.windows(2).last())
	else {
		frame_support::defensive!("two points always form a segment");
		return *last_weight;
	};
	let interpolated = Weight::from_parts(
		interpolate_component(x, *x_lo, *x_hi, w_lo.ref_time(), w_hi.ref_time()),
		interpolate_component(x, *x_lo, *x_hi, w_lo.proof_size(), w_hi.proof_size()),
	);
	// Past the last point `interpolate_component` continues the segment's line. Only a falling
	// segment (measurement noise) is floored at the last sample, so more aliases never cost
	// less than the `Max` sample.
	if x > *x_hi {
		interpolated.max(*last_weight)
	} else {
		interpolated
	}
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

	/// Samples growing by a different amount per component, so a mix-up between them shows.
	fn samples(sample: AliasCountSample) -> Weight {
		match sample {
			AliasCountSample::One => weight(100, 1_000),
			AliasCountSample::Two => weight(150, 1_200),
			AliasCountSample::Four => weight(220, 1_600),
			AliasCountSample::Eight => weight(340, 2_400),
			AliasCountSample::Max => weight(500, 4_000),
		}
	}

	#[test]
	fn sampled_counts_return_their_sample() {
		for (count, sample) in [
			(1, AliasCountSample::One),
			(2, AliasCountSample::Two),
			(4, AliasCountSample::Four),
			(8, AliasCountSample::Eight),
			(16, AliasCountSample::Max),
		] {
			assert_eq!(interpolate_unload_weight(count, 16, samples), samples(sample));
		}
	}

	#[test]
	fn counts_between_samples_interpolate_each_component() {
		// Halfway between 2 and 4.
		assert_eq!(interpolate_unload_weight(3, 16, samples), weight(185, 1_400));
		// A quarter of the way from 4 to 8 on ref_time (220 + 120 / 4) and proof_size
		// (1600 + 800 / 4).
		assert_eq!(interpolate_unload_weight(5, 16, samples), weight(250, 1_800));
		// Halfway between 8 and the maximum of 16.
		assert_eq!(interpolate_unload_weight(12, 16, samples), weight(420, 3_200));
	}

	#[test]
	fn interpolation_rounds_up() {
		let points = [(4, weight(10, 20)), (8, weight(11, 23))];
		// 10 + 1 / 4 and 20 + 3 / 4, both rounded up.
		assert_eq!(interpolate_weight(5, &points), weight(11, 21));
	}

	#[test]
	fn zero_aliases_charge_the_first_sample() {
		assert_eq!(interpolate_unload_weight(0, 16, samples), samples(AliasCountSample::One));
	}

	#[test]
	fn counts_above_max_extend_the_last_segment() {
		// From 8 to 16 ref_time rises by 160 and proof_size by 1_600, so by 20 and 200 per
		// alias.
		assert_eq!(interpolate_unload_weight(20, 16, samples), weight(580, 4_800));
	}

	#[test]
	fn a_falling_last_segment_never_drops_below_max() {
		let falling = |sample| match sample {
			AliasCountSample::Eight => weight(100, 100),
			AliasCountSample::Max => weight(90, 120),
			_ => weight(10, 10),
		};
		// Inside the segment the measured fall is followed.
		assert_eq!(interpolate_unload_weight(12, 16, falling), weight(95, 110));
		// Past it, ref_time holds at the `Max` sample while proof_size keeps rising.
		assert_eq!(interpolate_unload_weight(24, 16, falling), weight(90, 140));
		assert_eq!(interpolate_unload_weight(1_000, 16, falling), weight(90, 2_580));
	}

	#[test]
	fn max_equal_to_eight_charges_the_larger_sample_above_it() {
		let at_eight = |sample| match sample {
			AliasCountSample::Eight => weight(100, 300),
			AliasCountSample::Max => weight(120, 250),
			_ => weight(10, 10),
		};
		assert_eq!(interpolate_unload_weight(8, 8, at_eight), weight(100, 300));
		assert_eq!(interpolate_unload_weight(9, 8, at_eight), weight(120, 300));
	}

	#[test]
	fn interpolation_is_monotonic_over_growing_samples() {
		let mut previous = Weight::zero();
		for count in 0..40 {
			let current = interpolate_unload_weight(count, 16, samples);
			assert!(current.all_gte(previous), "weight fell from {previous:?} to {current:?}");
			previous = current;
		}
	}

	#[test]
	fn extreme_values_saturate() {
		let huge = |sample| match sample {
			AliasCountSample::Eight => weight(0, 0),
			AliasCountSample::Max => weight(u64::MAX, u64::MAX),
			_ => weight(0, 0),
		};
		assert_eq!(interpolate_unload_weight(u32::MAX, 16, huge), weight(u64::MAX, u64::MAX));
	}
}
