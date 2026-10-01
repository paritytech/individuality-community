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
use crate::xcm_config::{AssetHubLocation, RelayLocation, XcmConfig};
use frame_support::weights::WeightToFee as _;
use xcm::latest::prelude::*;
use xcm_runtime_apis::fees::runtime_decl_for_xcm_payment_api::XcmPaymentApi;

#[test]
fn xcm_payment_api_mirrors_the_configured_trader() {
	new_test_ext().execute_with(|| {
		type Trader = <XcmConfig as xcm_executor::Config>::Trader;
		let weight = Weight::from_parts(1_000_000_000, 10_000);
		let native_fee = WeightToFee::weight_to_fee(&weight);

		assert_eq!(
			PolkadotXcm::query_weight_to_asset_fee::<Trader>(
				weight,
				AssetId(RelayLocation::get()).into()
			)
			.unwrap(),
			native_fee,
		);
		assert_eq!(
			PolkadotXcm::query_weight_to_asset_fee::<Trader>(
				weight,
				AssetId(ExternalAssetLocation::get()).into()
			)
			.unwrap(),
			native_fee / Balance::from(EXTERNAL_ASSET_RATE),
		);

		// No trader component buys weight with Asset Hub itself.
		assert!(PolkadotXcm::query_weight_to_asset_fee::<Trader>(
			weight,
			AssetId(AssetHubLocation::get()).into()
		)
		.is_err());

		// The runtime API advertises exactly the assets the trader accepts, in that order.
		assert_eq!(
			Runtime::query_acceptable_payment_assets(XCM_VERSION).unwrap(),
			vec![
				VersionedAssetId::from(AssetId(RelayLocation::get())),
				VersionedAssetId::from(AssetId(ExternalAssetLocation::get())),
			],
		);
	});
}
