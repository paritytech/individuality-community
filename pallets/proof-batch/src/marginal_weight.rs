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

//! The weight charged for each proof pushed into the batch.
//!
//! The batch cost is concave in the proof count, and linear interpolation of a concave cost
//! undercharges. The marginal weight is a step function whose running sum is a piecewise-linear
//! upper bound of the cost: each segment takes the larger secant slope of itself and the previous
//! segment.

use frame_support::weights::Weight;

/// The weight charged for the `k`-th item of a batch, counted from 1.
///
/// `samples[i]` is the weight of a batch of `2^i` items, so `N` samples cover the counts 1, 2, 4,
/// ..., `2^(N-1)`. `N` must be at least 2. The first item pays `samples[0]`. An item in the
/// segment `(2^i, 2^(i+1)]` pays the larger slope of segment `i` and segment `i - 1`, per weight
/// component and rounded up. Past the last sample, the last segment's weight repeats.
///
/// The marginal weight is a step function. For a concave cost its running sum is a
/// piecewise-linear upper bound: the sum for the first `k` items is at least the cost of `k`
/// items, at and between the samples. The previous secant slope bounds every marginal cost in the
/// current segment. Taking the larger slope keeps the bound where noise makes a segment locally
/// convex.
pub fn marginal_weight<const N: usize>(k: u32, samples: [Weight; N]) -> Weight {
	const { assert!(N >= 2, "a marginal weight needs at least two samples") };
	if k <= 1 {
		return samples[0];
	}
	// Segment `i` covers `(2^i, 2^(i+1)]`, so `k - 1` has its highest bit at `i`.
	let segment = ((k - 1).ilog2() as usize).min(N - 2);
	let current = segment_slope(segment, samples);
	match segment.checked_sub(1) {
		Some(previous) => current.max(segment_slope(previous, samples)),
		None => current,
	}
}

/// The marginal weight of one item in the segment `(2^i, 2^(i+1)]`, rounded up per component.
///
/// A falling component has a slope of zero.
fn segment_slope<const N: usize>(i: usize, samples: [Weight; N]) -> Weight {
	let span = 1u64 << i;
	let rise = samples[i + 1].saturating_sub(samples[i]);
	Weight::from_parts(rise.ref_time().div_ceil(span), rise.proof_size().div_ceil(span))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn weight(ref_time: u64, proof_size: u64) -> Weight {
		Weight::from_parts(ref_time, proof_size)
	}

	/// Sum of the marginal weights of the first `n` items.
	fn marginal_total<const N: usize>(n: u32, samples: [Weight; N]) -> Weight {
		(1..=n).fold(Weight::zero(), |total, k| total.saturating_add(marginal_weight(k, samples)))
	}

	/// A concave curve sampled at 1, 2, 4, ..., 1024: a fixed overhead plus a marginal cost that
	/// falls with the batch size.
	fn concave_cost(n: u32) -> Weight {
		let n = u64::from(n);
		Weight::from_parts(5_000 + 12_700 * n - 2 * n * n, 100 + 3 * n)
	}

	fn concave_samples() -> [Weight; 11] {
		core::array::from_fn(|i| concave_cost(1 << i))
	}

	#[test]
	fn marginal_weight_of_the_first_item_is_the_first_sample() {
		assert_eq!(marginal_weight(1, concave_samples()), concave_cost(1));
		assert_eq!(marginal_total(1, concave_samples()), concave_cost(1));
	}

	#[test]
	fn marginal_total_never_falls_below_a_concave_cost() {
		for n in 1..=2_048 {
			let total = marginal_total(n, concave_samples());
			assert!(total.all_gte(concave_cost(n)), "n = {n}: {total:?} < {:?}", concave_cost(n));
		}
	}

	#[test]
	fn marginal_weight_takes_the_larger_of_the_current_and_previous_slope() {
		let samples = [weight(10, 0), weight(30, 4), weight(40, 12), weight(100, 12)];
		// Segment (1, 2]: only its own slope.
		assert_eq!(marginal_weight(2, samples), weight(20, 4));
		// Segment (2, 4]: slope 5 per item on ref_time, 4 on proof_size; previous is 20 and 4.
		assert_eq!(marginal_weight(3, samples), weight(20, 4));
		assert_eq!(marginal_weight(4, samples), weight(20, 4));
		// Segment (4, 8]: a locally convex segment of slope 15 beats the previous slope of 5.
		assert_eq!(marginal_weight(5, samples), weight(15, 4));
		assert_eq!(marginal_weight(8, samples), weight(15, 4));
		// Past the last sample, the last segment's weight repeats.
		assert_eq!(marginal_weight(9, samples), weight(15, 4));
		assert_eq!(marginal_weight(u32::MAX, samples), weight(15, 4));
	}

	#[test]
	fn marginal_weight_slopes_round_up() {
		let samples = [weight(0, 0), weight(0, 0), weight(1, 3)];
		assert_eq!(marginal_weight(3, samples), weight(1, 2));
	}

	#[test]
	fn marginal_weight_ignores_falling_components() {
		let samples = [weight(10, 10), weight(5, 20)];
		assert_eq!(marginal_weight(2, samples), weight(0, 10));
	}

	#[test]
	fn marginal_total_saturates() {
		let samples = [weight(u64::MAX, u64::MAX), weight(u64::MAX, u64::MAX)];
		assert_eq!(marginal_total(3, samples), weight(u64::MAX, u64::MAX));
	}
}
