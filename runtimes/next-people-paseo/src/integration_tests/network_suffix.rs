// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0
//
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
use frame_support::{assert_ok, traits::Get};
use indiv_support::context::{
	build_product_context, personhood, signing_context, ProductContextNetworkSuffix,
};

#[test]
fn genesis_config_selects_network_suffix() {
	let mut config = RuntimeGenesisConfig::default();
	config.network_suffix.network_suffix = b"test".to_vec().try_into().unwrap();
	let mut ext: TestExternalities =
		config.build_storage().expect("runtime genesis storage builds").into();

	ext.execute_with(|| {
		assert_eq!(<NetworkSuffix as Get<ProductContextNetworkSuffix>>::get().as_slice(), b"test");
		assert_eq!(
			Score::score_context(),
			build_product_context(personhood::PRODUCT_NAME, b"test", personhood::SCORE),
		);
	});
}

#[test]
fn root_suffix_override_updates_all_people_product_contexts() {
	new_test_ext().execute_with(|| {
		assert_ok!(NetworkSuffix::set_network_suffix(
			RuntimeOrigin::root(),
			b"test".to_vec().try_into().unwrap(),
		));

		assert_eq!(<NetworkSuffix as Get<ProductContextNetworkSuffix>>::get().as_slice(), b"test");
		assert_eq!(
			Score::score_context(),
			build_product_context(personhood::PRODUCT_NAME, b"test", personhood::SCORE),
		);
		assert_eq!(
			PeopleLite::auth_context(),
			build_product_context(personhood::PRODUCT_NAME, b"test", personhood::PEOPLE_LITE_AUTH,),
		);
		assert_eq!(
			Resources::resources_context(),
			build_product_context(personhood::PRODUCT_NAME, b"test", personhood::RESOURCES),
		);
		assert_eq!(
			PeopleAirdrops::people_airdrops_context(),
			build_product_context(personhood::PRODUCT_NAME, b"test", personhood::PEOPLE_AIRDROPS,),
		);
		assert_eq!(
			PeopleLite::signing_context(),
			signing_context(personhood::PRODUCT_NAME, b"test")
		);
		assert_eq!(
			Resources::signing_context(),
			signing_context(personhood::PRODUCT_NAME, b"test")
		);
		assert_eq!(
			Game::signing_context(),
			signing_context(indiv_pallet_game::PRODUCT_NAME, b"test")
		);
	});
}

#[test]
fn signing_contexts_are_published_as_pallet_constants() {
	new_test_ext().execute_with(|| {
		let frame_metadata::RuntimeMetadata::V14(metadata) = Runtime::metadata().1 else {
			panic!("runtime metadata is not V14");
		};
		let constant = |pallet: &str| {
			metadata
				.pallets
				.iter()
				.find(|p| p.name == pallet)
				.unwrap_or_else(|| panic!("{pallet} is in the metadata"))
				.constants
				.iter()
				.find(|c| c.name == "SigningContext")
				.unwrap_or_else(|| panic!("{pallet} publishes SigningContext"))
				.value
				.clone()
		};

		// Each verifying pallet publishes the context it verifies, as 32 raw bytes.
		assert_eq!(constant("PeopleLite"), PeopleLite::signing_context().encode());
		assert_eq!(constant("Resources"), Resources::signing_context().encode());
		assert_eq!(constant("Game"), Game::signing_context().encode());
		assert_eq!(constant("PeopleLite").len(), 32);
	});
}
