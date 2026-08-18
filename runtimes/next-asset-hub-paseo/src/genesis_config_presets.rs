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

//! Genesis configs presets for the AssetHubPaseo runtime

use crate::{xcm_config::UniversalLocation, *};
use alloc::vec::Vec;
use paseo_runtime_constants::system_parachain::ASSET_HUB_ID;
use sp_core::sr25519;
use sp_genesis_builder::PresetId;
use system_parachains_constants::genesis_presets::*;
use xcm::latest::prelude::*;
use xcm_builder::GlobalConsensusConvertsFor;
use xcm_executor::traits::ConvertLocation;
use AuraId;

const ASSET_HUB_POLKADOT_ED: Balance = ExistentialDeposit::get();

/// External asset id on Asset Hub. Mirrors `ExternalAssetLocation` in `next-people-paseo`.
pub const EXTERNAL_ASSET_ID: u32 = 50_000_413;

/// Invulnerable Collators for the particular case of AssetHubPaseo
pub fn invulnerables_asset_hub_paseo() -> Vec<(AccountId, AuraId)> {
	vec![
		(get_account_id_from_seed::<sr25519::Public>("Alice"), get_from_seed::<AuraId>("Alice")),
		(get_account_id_from_seed::<sr25519::Public>("Bob"), get_from_seed::<AuraId>("Bob")),
	]
}

fn asset_hub_paseo_genesis(
	invulnerables: Vec<(AccountId, AuraId)>,
	endowed_accounts: Vec<AccountId>,
	id: ParaId,
	foreign_assets: Vec<(Location, AccountId, Balance)>,
	foreign_assets_endowed_accounts: Vec<(Location, AccountId, Balance)>,
) -> serde_json::Value {
	let dev_stakers =
		if cfg!(feature = "runtime-benchmarks") { Some((2_000, 25_000)) } else { None };
	serde_json::json!({
		"balances": BalancesConfig {
			balances: endowed_accounts
				.iter()
				.cloned()
				.map(|k| (k, ASSET_HUB_POLKADOT_ED * 4096 * 4096))
				.collect(),
			dev_accounts: None,
		},
		"parachainInfo": ParachainInfoConfig {
			parachain_id: id,
			..Default::default()
		},
		"collatorSelection": CollatorSelectionConfig {
			invulnerables: invulnerables.iter().cloned().map(|(acc, _)| acc).collect(),
			candidacy_bond: ASSET_HUB_POLKADOT_ED * 16,
			..Default::default()
		},
		"session": SessionConfig {
			keys: invulnerables
				.into_iter()
				.map(|(acc, aura)| {
					(
						acc.clone(),                           // account id
						acc,                                   // validator id
						SessionKeys { aura }, 	// session keys
					)
				})
				.collect(),
			..Default::default()
		},
		"sudo": {
			"key": Some(get_account_id_from_seed::<sr25519::Public>("Alice"))
		},
		"polkadotXcm": {
			"safeXcmVersion": Some(SAFE_XCM_VERSION),
		},
	"staking": {
		"validatorCount": 100,
		"devStakers": dev_stakers
	},
		"foreignAssets": ForeignAssetsConfig {
			assets: foreign_assets
				.into_iter()
				.map(|asset| (asset.0, asset.1, false, asset.2))
				.collect(),
			accounts: foreign_assets_endowed_accounts
				.into_iter()
				.map(|asset| (asset.0, asset.1, asset.2))
				.collect(),
			..Default::default()
		},
		"revive": ReviveConfig {
			// `AutoMap = true` already maps endowed native accounts when balances genesis creates
			// them, so pre-seeding the same accounts here only produces `AccountAlreadyMapped`
			// benchmark noise.
			mapped_accounts: Vec::new(),
			accounts: Vec::new(),
			debug_settings: None,
		},
		// no need to pass anything to aura, in fact it will panic if we do. Session will take care
		// of this. `aura: Default::default()`
	})
}

pub fn asset_hub_paseo_local_testnet_genesis(para_id: ParaId) -> serde_json::Value {
	asset_hub_paseo_genesis(
		invulnerables_asset_hub_paseo(),
		testnet_accounts(),
		para_id,
		vec![
			// bridged KSM
			(
				Location::new(2, [GlobalConsensus(Kusama)]),
				GlobalConsensusConvertsFor::<UniversalLocation, AccountId>::convert_location(
					&Location { parents: 2, interior: [GlobalConsensus(Kusama)].into() },
				)
				.unwrap(),
				10000000,
			),
		],
		vec![
			// bridged KSM to Bob
			(
				Location::new(2, [GlobalConsensus(Kusama)]),
				get_account_id_from_seed::<sp_core::sr25519::Public>("Bob"),
				10000000 * 4096 * 4096,
			),
		],
	)
}

fn asset_hub_paseo_development_genesis(para_id: ParaId) -> serde_json::Value {
	asset_hub_paseo_genesis(
		invulnerables_asset_hub_paseo(),
		testnet_accounts_with([
			// Make sure `StakingPot` is funded for benchmarking purposes.
			StakingPot::get(),
			// Slashes land in the staging account first; funding it above ED keeps benchmark-time
			// `OnUnbalanced` deposits from tripping DAP's defensive path.
			Dap::staging_account(),
		]),
		para_id,
		vec![],
		vec![],
	)
}

/// Provides the names of the predefined genesis configs for this runtime.
pub fn preset_names() -> Vec<PresetId> {
	vec![
		PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
		PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
	]
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::DEV_RUNTIME_PRESET =>
			asset_hub_paseo_development_genesis(ASSET_HUB_ID.into()),
		sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET =>
			asset_hub_paseo_local_testnet_genesis(ASSET_HUB_ID.into()),
		_ => return None,
	};
	Some(
		serde_json::to_string(&patch)
			.expect("serialization to json is expected to work. qed.")
			.into_bytes(),
	)
}
