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

use super::*;
use frame_support::assert_ok;
use paseo_runtime_constants::ValueTransferAuthorizationPubkey;

#[test]
fn value_transfer_authorization_pubkey_can_be_rotated_via_root_storage() {
	new_test_ext().execute_with(|| {
		let original = ValueTransferAuthorizationPubkey::get();
		let rotated = ed25519::Public::from_raw([0x24; 32]);
		assert_ne!(original, rotated, "test replacement key should differ from the default");

		assert_ok!(frame_system::Pallet::<Runtime>::set_storage(
			RuntimeOrigin::root(),
			vec![(ValueTransferAuthorizationPubkey::key().to_vec(), rotated.encode())],
		));

		assert_eq!(ValueTransferAuthorizationPubkey::get(), rotated);
	});
}
