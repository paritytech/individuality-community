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

//! Placeholder weights for `indiv_pallet_proof_batch`.
//!
//! These values are estimates from the coinage unload benchmarks, not a benchmark run.
//! Regenerate this file with `frame-omni-bencher` before the pallet goes live.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]
#![allow(dead_code)]

use frame_support::{traits::Get, weights::{Weight, constants::RocksDbWeight}};
use core::marker::PhantomData;

/// Weight functions needed for `indiv_pallet_proof_batch`.
pub trait WeightInfo {
	fn verify_batch_1() -> Weight;
	fn verify_batch_2() -> Weight;
	fn verify_batch_4() -> Weight;
	fn verify_batch_8() -> Weight;
	fn verify_batch_16() -> Weight;
	fn verify_batch_32() -> Weight;
	fn verify_batch_64() -> Weight;
	fn verify_batch_128() -> Weight;
	fn verify_batch_256() -> Weight;
	fn verify_batch_512() -> Weight;
	fn verify_batch_1024() -> Weight;
	fn verify_batch_2048() -> Weight;
	fn verify_batch_2048_512_rings() -> Weight;
	fn first_access() -> Weight;
}

/// Weights for `indiv_pallet_proof_batch` using the Substrate node and recommended hardware.
pub struct SubstrateWeight<T>(PhantomData<T>);
impl<T: frame_system::Config> WeightInfo for SubstrateWeight<T> {
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_1() -> Weight {
		Weight::from_parts(29_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_2() -> Weight {
		Weight::from_parts(41_600_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_4() -> Weight {
		Weight::from_parts(66_800_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_8() -> Weight {
		Weight::from_parts(114_800_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_16() -> Weight {
		Weight::from_parts(200_400_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_32() -> Weight {
		Weight::from_parts(371_600_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_64() -> Weight {
		Weight::from_parts(685_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_128() -> Weight {
		Weight::from_parts(1_270_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_256() -> Weight {
		Weight::from_parts(2_400_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_512() -> Weight {
		Weight::from_parts(4_600_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_1024() -> Weight {
		Weight::from_parts(8_900_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_2048() -> Weight {
		Weight::from_parts(17_600_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_2048_512_rings() -> Weight {
		Weight::from_parts(18_622_000_000_000, 0)
	}
	/// Storage: `ProofBatch::TmpPoolValidation` (r:1 w:0)
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn first_access() -> Weight {
		Weight::from_parts(40_000_000, 2_000)
			.saturating_add(T::DbWeight::get().reads(4_u64))
			.saturating_add(T::DbWeight::get().writes(3_u64))
	}
}

// For backwards compatibility and tests.
impl WeightInfo for () {
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_1() -> Weight {
		Weight::from_parts(29_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_2() -> Weight {
		Weight::from_parts(41_600_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_4() -> Weight {
		Weight::from_parts(66_800_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_8() -> Weight {
		Weight::from_parts(114_800_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_16() -> Weight {
		Weight::from_parts(200_400_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_32() -> Weight {
		Weight::from_parts(371_600_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_64() -> Weight {
		Weight::from_parts(685_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_128() -> Weight {
		Weight::from_parts(1_270_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_256() -> Weight {
		Weight::from_parts(2_400_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_512() -> Weight {
		Weight::from_parts(4_600_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_1024() -> Weight {
		Weight::from_parts(8_900_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_2048() -> Weight {
		Weight::from_parts(17_600_000_000_000, 0)
	}
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn verify_batch_2048_512_rings() -> Weight {
		Weight::from_parts(18_622_000_000_000, 0)
	}
	/// Storage: `ProofBatch::TmpPoolValidation` (r:1 w:0)
	/// Storage: `ProofBatch::BatchProofs` (r:1 w:1)
	/// Storage: `ProofBatch::BatchRingHashes` (r:1 w:1)
	fn first_access() -> Weight {
		Weight::from_parts(40_000_000, 2_000)
			.saturating_add(RocksDbWeight::get().reads(4_u64))
			.saturating_add(RocksDbWeight::get().writes(3_u64))
	}
}
