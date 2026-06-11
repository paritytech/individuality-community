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

//! Sanity tests for coinage fee calculations.

use super::*;

type Balance = <Runtime as pallet_balances::Config>::Balance;

#[test]
fn paid_unload_token_fee_in_native_is_reasonable() {
	new_test_ext().execute_with(|| {
		let fee: Balance = Coinage::get_paid_unload_token_fee_in_native();

		// Print the actual value for debugging
		println!("paid_unload_token_fee_in_native = {fee}");

		// The fee should be within reasonable bounds.
		// We allow a range of value/2 to value*2 to tolerate weight changes.
		const EXPECTED: Balance = 33_000_000; // ~0.0033 PAS
		let lower_bound = EXPECTED / 2;
		let upper_bound = EXPECTED * 2;

		assert!(fee >= lower_bound, "Fee {fee} is below minimum expected {lower_bound}");
		assert!(fee <= upper_bound, "Fee {fee} is above maximum expected {upper_bound}");
	});
}
