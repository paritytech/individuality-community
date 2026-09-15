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

//! The configuration invariants of the NFT claim credits, against this runtime's own weights and
//! block limits.

use super::*;

/// The credits pallet weighs a `report` and the block it sits in, which its own mock cannot: a mock
/// declares weights of its own. Nothing else runs these assertions, so a configuration that breaks
/// one would otherwise fail at the runtime upgrade that ships it.
#[test]
fn the_credit_configuration_holds_for_this_runtime() {
	new_test_ext().execute_with(|| {
		<NftCredits as Hooks<BlockNumber>>::integrity_test();
	});
}
