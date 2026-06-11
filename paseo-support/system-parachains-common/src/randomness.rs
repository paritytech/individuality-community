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

use core::marker::PhantomData;
use cumulus_pallet_parachain_system::{RelaychainDataProvider, RelaychainStateProvider};
use cumulus_primitives_core::relay_chain;
use frame_support::traits::Randomness;
use sp_runtime::traits::{BlakeTwo256, Hash};
use sp_state_machine::{Backend, TrieBackendBuilder};

pub const LOG_TARGET: &str = "runtime::randomness";

/// VRF output length for per-slot randomness.
pub const VRF_RANDOMNESS_LENGTH: usize = 32;

// The initial version of the `Randomness` implementation, subject to change later.
/// Provides randomness from the Relay Chain VRF from one epoch ago, but does not include the block
/// number indicating when this randomness was generated or became observable to chain observers.
///
/// WARNING: This implementation does not return the block number associated with the randomness,
/// because this information is not available in the validation data.
pub struct RelayChainOneEpochAgoWithoutBlockNumber<T, BlockNumber>(PhantomData<(T, BlockNumber)>);

impl<T, BlockNumber> Randomness<T::Hash, BlockNumber>
	for RelayChainOneEpochAgoWithoutBlockNumber<T, BlockNumber>
where
	T: cumulus_pallet_parachain_system::Config,
	BlockNumber: From<u32>,
{
	fn random(subject: &[u8]) -> (T::Hash, BlockNumber) {
		// Defensive fallback used if the `well_known_keys::ONE_EPOCH_AGO_RANDOMNESS` key
		// is missing or absent from the validation data. This situation is unexpected,
		// as the key should always be present.
		let defensive_fallback = || {
			let rc_state = RelaychainDataProvider::<T>::current_relay_chain_state();
			let mut subject = subject.to_vec();
			subject.extend_from_slice(&rc_state.state_root.0);

			(T::Hashing::hash(&subject[..]), 0.into())
		};

		let Some(relay_state_proof) = cumulus_pallet_parachain_system::RelayStateProof::<T>::get()
		else {
			log::error!(
				target: LOG_TARGET,
				"No relay state proof in cumulus_pallet_parachain_system; cannot fetch randomness"
			);
			return defensive_fallback();
		};

		let relay_parent_storage_root = if let Some(validation_data) =
			cumulus_pallet_parachain_system::ValidationData::<T>::get()
		{
			validation_data.relay_parent_storage_root
		} else {
			log::error!(
				target: LOG_TARGET,
				"No validation data in cumulus_pallet_parachain_system; cannot fetch randomness"
			);
			return defensive_fallback();
		};

		let db = relay_state_proof.into_memory_db::<BlakeTwo256>();
		let trie_backend = TrieBackendBuilder::new(db, relay_parent_storage_root).build();

		let Ok(Some(random)) = trie_backend
			.storage(relay_chain::well_known_keys::ONE_EPOCH_AGO_RANDOMNESS)
			.inspect_err(|e| {
				log::error!(
					target: LOG_TARGET,
					"Failed to lookup `well_known_keys::ONE_EPOCH_AGO_RANDOMNESS` from trie: {e}"
				);
			})
		else {
			log::error!(
				target: LOG_TARGET,
				"`well_known_keys::ONE_EPOCH_AGO_RANDOMNESS` is none; cannot fetch randomness"
			);
			return defensive_fallback();
		};

		let mut subject = subject.to_vec();
		subject.reserve(VRF_RANDOMNESS_LENGTH);
		subject.extend_from_slice(&random);

		(T::Hashing::hash(&subject[..]), 0.into())
	}
}
