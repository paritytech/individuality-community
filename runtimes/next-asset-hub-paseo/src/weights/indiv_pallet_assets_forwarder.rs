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

//! Weights for `indiv_pallet_assets_forwarder`.
//!
//! Placeholder values pending a benchmark run by the weights bot; the shapes match the
//! benchmarks in the pallet's `benchmarking.rs`.

#![cfg_attr(rustfmt, rustfmt_skip)]
#![allow(unused_parens)]
#![allow(unused_imports)]
#![allow(missing_docs)]

use frame_support::{traits::Get, weights::Weight};
use core::marker::PhantomData;

/// Weight functions for `indiv_pallet_assets_forwarder`.
pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> indiv_pallet_assets_forwarder::WeightInfo for WeightInfo<T> {
	fn forward_asset() -> Weight {
		Weight::from_parts(150_000_000, 0)
			.saturating_add(Weight::from_parts(0, 6000))
			.saturating_add(T::DbWeight::get().reads(6))
			.saturating_add(T::DbWeight::get().writes(4))
	}
	fn sync_asset_status() -> Weight {
		Weight::from_parts(120_000_000, 0)
			.saturating_add(Weight::from_parts(0, 6000))
			.saturating_add(T::DbWeight::get().reads(5))
			.saturating_add(T::DbWeight::get().writes(2))
	}
	fn remove_forwarded_asset() -> Weight {
		Weight::from_parts(60_000_000, 0)
			.saturating_add(Weight::from_parts(0, 6000))
			.saturating_add(T::DbWeight::get().reads(3))
			.saturating_add(T::DbWeight::get().writes(3))
	}
}
