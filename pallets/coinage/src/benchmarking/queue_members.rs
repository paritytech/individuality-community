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

//! Filler members for full onboarding pages in benchmark setup.
//!
//! These bytes are not validated as public keys. The measured call only decodes and
//! stores existing queue members; it validates the new output keys separately.
//! Filler members have the same encoding size as valid members and are never onboarded.

use crate::{Config, MemberOf};
use codec::{DecodeAll, Encode};
use sp_crypto_hashing::blake2_256;

/// Derives a filler member from its page index and denomination group.
pub(super) fn member<T: Config>(i: u32, group: u32) -> MemberOf<T> {
	let bytes = blake2_256(&(i, group).encode());
	MemberOf::<T>::decode_all(&mut bytes.as_slice()).expect("benchmark member encoding is 32 bytes")
}

#[cfg(test)]
mod tests {
	use super::member;
	use crate::mock::Test;
	use alloc::collections::BTreeSet;

	#[test]
	fn filler_members_are_unique_across_production_pages() {
		let mut seen = BTreeSet::new();
		for group in 0..15 {
			for i in 0..256 {
				assert!(seen.insert(member::<Test>(i, group)));
			}
		}
		assert_eq!(seen.len(), 15 * 256);
	}
}
