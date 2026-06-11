// Copyright (C) Parity Technologies and the various Polkadot contributors, see Contributions.md
// for a list of specific contributors.
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

#[cfg(not(feature = "std"))]
use alloc::format;
use alloc::vec::Vec;
use parachains_common::AuraId;
use polkadot_primitives::{AccountId, AccountPublic};
use sp_core::{sr25519, Pair, Public};
use sp_runtime::traits::IdentifyAccount;

/// Invulnerable Collators
pub fn invulnerables() -> Vec<(parachains_common::AccountId, AuraId)> {
	Vec::from([
		(get_account_id_from_seed::<sr25519::Public>("Alice"), get_from_seed::<AuraId>("Alice")),
		(get_account_id_from_seed::<sr25519::Public>("Bob"), get_from_seed::<AuraId>("Bob")),
	])
}

/// Invulnerable Collators
pub fn invulnerables_tot() -> Vec<(parachains_common::AccountId, AuraId)> {
	Vec::from([(
		get_account_id_from_seed::<sr25519::Public>("Alice"),
		get_from_seed::<AuraId>("Alice"),
	)])
}

/// Test accounts
pub fn testnet_accounts() -> Vec<AccountId> {
	Vec::from([
		get_account_id_from_seed::<sr25519::Public>("Alice"),
		get_account_id_from_seed::<sr25519::Public>("Bob"),
		get_account_id_from_seed::<sr25519::Public>("Charlie"),
		get_account_id_from_seed::<sr25519::Public>("Dave"),
		get_account_id_from_seed::<sr25519::Public>("Eve"),
		get_account_id_from_seed::<sr25519::Public>("Ferdie"),
		get_account_id_from_seed::<sr25519::Public>("Alice//stash"),
		get_account_id_from_seed::<sr25519::Public>("Bob//stash"),
		get_account_id_from_seed::<sr25519::Public>("Charlie//stash"),
		get_account_id_from_seed::<sr25519::Public>("Dave//stash"),
		get_account_id_from_seed::<sr25519::Public>("Eve//stash"),
		get_account_id_from_seed::<sr25519::Public>("Ferdie//stash"),
		// The following are ethereum derived addresses.
		// Import into your wallet by deriving from this seed:
		// "bottom drive obey lake curtain smoke basket hold race lonely fit walk"
		// BIP44 Serial [1, 5]
		// subxt_signer::eth::dev::alith()
		array_bytes::hex_n_into::<_, _, 32>(
			"f24ff3a9cf04c71dbc94d0b566f7a27b94566caceeeeeeeeeeeeeeeeeeeeeeee",
		)
		.unwrap(),
		// subxt_signer::eth::dev::baltathar()
		array_bytes::hex_n_into::<_, _, 32>(
			"3cd0a705a2dc65e5b1e1205896baa2be8a07c6e0eeeeeeeeeeeeeeeeeeeeeeee",
		)
		.unwrap(),
		// subxt_signer::eth::dev::charleth()
		array_bytes::hex_n_into::<_, _, 32>(
			"798d4ba9baf0064ec19eb4f0a1a45785ae9d6dfceeeeeeeeeeeeeeeeeeeeeeee",
		)
		.unwrap(),
		// subxt_signer::eth::dev::dorothy()
		array_bytes::hex_n_into::<_, _, 32>(
			"773539d4ac0e786233d90a233654ccee26a613d9eeeeeeeeeeeeeeeeeeeeeeee",
		)
		.unwrap(),
		// subxt_signer::eth::dev::ethan()
		array_bytes::hex_n_into::<_, _, 32>(
			"ff64d3f6efe2317ee2807d223a0bdc4c0c49dfdbeeeeeeeeeeeeeeeeeeeeeeee",
		)
		.unwrap(),
	])
}

/// Test accounts extended `with`.
pub fn testnet_accounts_with(extra: impl IntoIterator<Item = AccountId>) -> Vec<AccountId> {
	let mut accounts = testnet_accounts();
	accounts.extend(extra);
	accounts
}

/// Helper function to generate a crypto pair from seed
pub fn get_from_seed<TPublic: Public>(seed: &str) -> <TPublic::Pair as Pair>::Public {
	TPublic::Pair::from_string(&format!("//{seed}"), None)
		.expect("static values are valid; qed")
		.public()
}

/// Helper function to generate an account ID from seed
pub fn get_account_id_from_seed<TPublic: Public>(seed: &str) -> AccountId
where
	AccountPublic: From<<TPublic::Pair as Pair>::Public>,
{
	AccountPublic::from(get_from_seed::<TPublic>(seed)).into_account()
}

/// The default XCM version to set in genesis config.
pub const SAFE_XCM_VERSION: u32 = xcm::prelude::XCM_VERSION;
