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

//! # People Paseo Runtime genesis config presets

use crate::*;
use alloc::{vec, vec::Vec};
use cumulus_primitives_core::ParaId;
use frame_support::build_struct_json_patch;
use hex_literal::hex;
use parachains_common::{AccountId, AuraId};
use paseo_runtime_constants::system_parachain::PEOPLE_ID;
use sp_core::crypto::UncheckedInto;
use sp_genesis_builder::PresetId;
use sp_keyring::Sr25519Keyring;
use verifiable::ring::ark_vrf::suites::bandersnatch::BandersnatchSha512Ell2;

const LIVE_RUNTIME_PRESET: &str = "live";

// Paseo network uses UNITS = 10_000_000_000
const PAS: Balance = 10_000_000_000;
const SAFE_XCM_VERSION: u32 = 4;

const PEOPLE_PASEO_ED: Balance = ExistentialDeposit::get();

fn people_paseo_genesis(
	invulnerables: Vec<(AccountId, AuraId)>,
	endowed_accounts: Vec<AccountId>,
	endowment: Balance,
	id: ParaId,
	sudo_key: AccountId,
) -> serde_json::Value {
	// Ensure the sudo account is always funded.
	let mut endowed_accounts = endowed_accounts;
	if !endowed_accounts.contains(&sudo_key) {
		endowed_accounts.push(sudo_key.clone());
	}

	build_struct_json_patch!(RuntimeGenesisConfig {
		balances: BalancesConfig {
			balances: endowed_accounts.iter().cloned().map(|k| (k, endowment)).collect(),
		},
		parachain_info: ParachainInfoConfig { parachain_id: id },
		collator_selection: CollatorSelectionConfig {
			invulnerables: invulnerables.iter().cloned().map(|(acc, _)| acc).collect(),
			candidacy_bond: PEOPLE_PASEO_ED * 16,
		},
		session: SessionConfig {
			keys: invulnerables
				.into_iter()
				.map(|(acc, aura)| {
					(
						acc.clone(),          // account id
						acc,                  // validator id
						SessionKeys { aura }, // session keys
					)
				})
				.collect(),
		},
		chunks_manager: ChunksManagerConfig {
			encoded_chunk_page_hashes:
				// We don't include the builder params hashes for r2e14 because it makes the
				// runtime wasm binary too big.
				indiv_support::genesis::ring_verifier_r2e9_r2e10_builder_params_hashes::<BandersnatchSha512Ell2>(
					people::ChunkPageSize::get()
				),
			_phantom: Default::default()
		},
		polkadot_xcm: PolkadotXcmConfig { safe_xcm_version: Some(SAFE_XCM_VERSION) },
		sudo: SudoConfig { key: Some(sudo_key) },
	})
}

fn people_paseo_local_testnet_genesis() -> serde_json::Value {
	people_paseo_genesis(
		// initial collators.
		vec![
			(Sr25519Keyring::Alice.to_account_id(), Sr25519Keyring::Alice.public().into()),
			(Sr25519Keyring::Bob.to_account_id(), Sr25519Keyring::Bob.public().into()),
		],
		Sr25519Keyring::well_known().map(|x| x.to_account_id()).collect(),
		PAS * 1_000_000,
		PEOPLE_ID.into(),
		Sr25519Keyring::Alice.to_account_id(),
	)
}

fn people_paseo_development_genesis() -> serde_json::Value {
	people_paseo_genesis(
		// initial collators.
		vec![(Sr25519Keyring::Alice.to_account_id(), Sr25519Keyring::Alice.public().into())],
		vec![
			Sr25519Keyring::Alice.to_account_id(),
			Sr25519Keyring::Bob.to_account_id(),
			Sr25519Keyring::AliceStash.to_account_id(),
			Sr25519Keyring::BobStash.to_account_id(),
		],
		PAS * 1_000_000,
		PEOPLE_ID.into(),
		Sr25519Keyring::Alice.to_account_id(),
	)
}

fn people_paseo_live_genesis() -> serde_json::Value {
	people_paseo_genesis(
		Vec::from([
			(
				// Collator 1 - Stash & Aura key
				hex!("c4a649d9ddfa50130085a322b9adfe684888df6a6212dab0ef81193011d13119").into(),
				hex!("c4a649d9ddfa50130085a322b9adfe684888df6a6212dab0ef81193011d13119")
					.unchecked_into(),
			),
			(
				// Collator 2 - Stash & Aura key
				hex!("9eb379b09a33013b839ed290a6d73cc31b138b1f6c178ba51406a45503801265").into(),
				hex!("9eb379b09a33013b839ed290a6d73cc31b138b1f6c178ba51406a45503801265")
					.unchecked_into(),
			),
		]),
		Vec::new(),
		PEOPLE_PASEO_ED * 4096 * 4096,
		PEOPLE_ID.into(),
		// Sudo key for live Paseo deployment
		hex!("98384d04c5a3f298f29b027b5581b096b5d8f6a84e34e23a62f23d8ef6afc766").into(),
	)
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
	let patch = match id.as_ref() {
		sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET => people_paseo_local_testnet_genesis(),
		sp_genesis_builder::DEV_RUNTIME_PRESET => people_paseo_development_genesis(),
		LIVE_RUNTIME_PRESET => people_paseo_live_genesis(),
		_ => return None,
	};

	Some(
		serde_json::to_string(&patch)
			.expect("serialization to json is expected to work. qed.")
			.into_bytes(),
	)
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
	vec![
		PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
		PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
		PresetId::from(LIVE_RUNTIME_PRESET),
	]
}
