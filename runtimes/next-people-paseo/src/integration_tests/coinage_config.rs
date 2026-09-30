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

//! Configuration invariants between coinage and the member service.

use super::*;
use frame_support::traits::Get;

/// The mixed-output benchmarks model each denomination group across at most two onboarding pages.
/// A group holds at most `MaxSplitOutputs` coins, so it spans two pages only while it fits one.
#[test]
fn split_outputs_fit_one_onboarding_page() {
	let max_split_outputs: u32 = <Runtime as indiv_pallet_coinage::Config>::MaxSplitOutputs::get();
	let page_size: u32 = <Runtime as indiv_pallet_members::Config>::OnboardingQueuePageSize::get();
	assert!(
		max_split_outputs <= page_size,
		"MaxSplitOutputs {max_split_outputs} exceeds OnboardingQueuePageSize {page_size}",
	);
}
