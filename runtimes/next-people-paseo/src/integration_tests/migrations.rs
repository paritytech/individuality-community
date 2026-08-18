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

//! Tests for the runtime's bootstrap migrations.

use crate::{migrations::ChunkPageHashesInitialization, Runtime, System};
use frame_support::traits::OnRuntimeUpgrade;
use indiv_pallet_chunks_manager::{ChunkPageHashes, Event as ChunksManagerEvent};
use indiv_support::{
	crypto::BandersnatchSuite, genesis::ring_verifier_r2e9_r2e10_builder_params_hashes,
	traits::RingExponent,
};
use sp_io::TestExternalities;
use sp_runtime::BuildStorage;

type Initialization = ChunkPageHashesInitialization<Runtime, BandersnatchSuite>;

/// Externalities from default genesis: no chunk page hashes, no collections. The shared
/// `new_test_ext` cannot be used here since it pre-creates both collections.
fn new_empty_ext() -> TestExternalities {
	let mut ext: TestExternalities = frame_system::GenesisConfig::<Runtime>::default()
		.build_storage()
		.expect("frame system genesis storage builds")
		.into();
	ext.execute_with(|| System::set_block_number(1));
	ext
}

fn page_size() -> u32 {
	<Runtime as indiv_pallet_chunks_manager::Config>::PageSize::get()
}

fn expected_hashes() -> Vec<(RingExponent, Vec<[u8; 32]>)> {
	ring_verifier_r2e9_r2e10_builder_params_hashes::<BandersnatchSuite>(page_size())
		.into_iter()
		.map(|(exponent, hashes)| (RingExponent::new_from_exponent(exponent).unwrap(), hashes))
		.collect::<Vec<_>>()
}

fn stored_hashes() -> Vec<(RingExponent, u32, [u8; 32])> {
	let mut stored = ChunkPageHashes::<Runtime>::iter().collect::<Vec<_>>();
	stored.sort();
	stored
}

#[test]
fn initializes_all_ring_page_hashes_from_empty_state() {
	new_empty_ext().execute_with(|| {
		Initialization::on_runtime_upgrade();

		for (ring_exponent, hashes) in expected_hashes() {
			assert!(!hashes.is_empty());
			for (page_index, hash) in hashes.iter().enumerate() {
				assert_eq!(
					ChunkPageHashes::<Runtime>::get(ring_exponent, page_index as u32),
					Some(*hash)
				);
			}
			System::assert_has_event(
				ChunksManagerEvent::<Runtime>::ChunkPageHashesInitialized {
					ring_exponent,
					total_pages: hashes.len() as u32,
				}
				.into(),
			);
		}
	});
}

#[test]
fn skips_rings_with_existing_hashes() {
	new_empty_ext().execute_with(|| {
		// Simulate a genesis-initialized 2^9 ring with a marker hash.
		let marker = [7u8; 32];
		ChunkPageHashes::<Runtime>::insert(RingExponent::R2e9, 0, marker);

		Initialization::on_runtime_upgrade();

		// The preexisting ring is untouched.
		assert_eq!(ChunkPageHashes::<Runtime>::get(RingExponent::R2e9, 0), Some(marker));
		assert_eq!(ChunkPageHashes::<Runtime>::iter_prefix(RingExponent::R2e9).count(), 1);

		// The empty ring is still initialized.
		let (_, r2e10_hashes) = expected_hashes()
			.into_iter()
			.find(|(ring_exponent, _)| *ring_exponent == RingExponent::R2e10)
			.unwrap();
		assert_eq!(
			ChunkPageHashes::<Runtime>::iter_prefix(RingExponent::R2e10).count(),
			r2e10_hashes.len()
		);
		for (page_index, hash) in r2e10_hashes.iter().enumerate() {
			assert_eq!(
				ChunkPageHashes::<Runtime>::get(RingExponent::R2e10, page_index as u32),
				Some(*hash),
				"page {page_index}"
			);
		}
	});
}

/// Inconsistent state with page 0 absent, but greater present page index is a no-op
/// for the specific ring index
#[test]
fn skips_rings_whose_only_stored_page_is_not_the_first() {
	new_empty_ext().execute_with(|| {
		let (_, r2e9_hashes) = expected_hashes()
			.into_iter()
			.find(|(ring_exponent, _)| *ring_exponent == RingExponent::R2e9)
			.unwrap();
		let last_page = r2e9_hashes.len() as u32 - 1;
		assert!(last_page > 0);

		let marker = [7u8; 32];
		ChunkPageHashes::<Runtime>::insert(RingExponent::R2e9, last_page, marker);

		Initialization::on_runtime_upgrade();

		assert_eq!(ChunkPageHashes::<Runtime>::iter_prefix(RingExponent::R2e9).count(), 1);
		assert_eq!(ChunkPageHashes::<Runtime>::get(RingExponent::R2e9, last_page), Some(marker));
	});
}

#[test]
fn rerun_is_a_no_op() {
	new_empty_ext().execute_with(|| {
		Initialization::on_runtime_upgrade();
		let after_first = stored_hashes();
		let events_after_first = System::events().len();

		Initialization::on_runtime_upgrade();

		assert_eq!(stored_hashes(), after_first);
		assert_eq!(System::events().len(), events_after_first);
	});
}

#[test]
fn genesis_config_creates_both_collections() {
	let mut config = crate::RuntimeGenesisConfig::default();
	config.people.create_collection = true;
	config.people_lite.create_collection = true;

	let mut ext: TestExternalities =
		config.build_storage().expect("runtime genesis storage builds").into();
	ext.execute_with(|| {
		assert!(indiv_pallet_people::PeopleCollectionCreated::<Runtime>::get());
		assert!(indiv_pallet_people_lite::LitePeopleCollectionCreated::<Runtime>::get());
	});
}

/// Guards the wiring in `genesis_config_presets.rs`: the flags above are only useful if the
/// shipped presets actually set them.
#[test]
fn genesis_presets_enable_collection_creation() {
	for preset in [sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET, "live"] {
		let json =
			crate::genesis_config_presets::get_preset(&sp_genesis_builder::PresetId::from(preset))
				.unwrap_or_else(|| panic!("preset {preset} exists"));
		let value: serde_json::Value = serde_json::from_slice(&json).expect("preset is valid json");

		assert_eq!(value["people"]["createCollection"], serde_json::Value::Bool(true), "{preset}");
		assert_eq!(
			value["peopleLite"]["createCollection"],
			serde_json::Value::Bool(true),
			"{preset}"
		);
	}
}

#[test]
fn development_preset_skips_collection_creation() {
	let json = crate::genesis_config_presets::get_preset(&sp_genesis_builder::PresetId::from(
		sp_genesis_builder::DEV_RUNTIME_PRESET,
	))
	.expect("development preset exists");
	let value: serde_json::Value = serde_json::from_slice(&json).expect("preset is valid json");

	assert_eq!(value["people"]["createCollection"], serde_json::Value::Bool(false));
	assert_eq!(value["peopleLite"]["createCollection"], serde_json::Value::Bool(false));
}

#[test]
fn migrations_tuple_initializes_bootstrap_state() {
	new_empty_ext().execute_with(|| {
		<crate::Migrations as OnRuntimeUpgrade>::on_runtime_upgrade();

		assert!(indiv_pallet_people::PeopleCollectionCreated::<Runtime>::get());
		assert!(indiv_pallet_people_lite::LitePeopleCollectionCreated::<Runtime>::get());
		assert!(ChunkPageHashes::<Runtime>::contains_key(RingExponent::R2e9, 0));
		assert!(ChunkPageHashes::<Runtime>::contains_key(RingExponent::R2e10, 0));
	});
}
