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

//! Runtime-local migrations and migration helpers.

#[cfg(not(feature = "runtime-benchmarks"))]
use alloc::vec::Vec;
#[cfg(not(feature = "runtime-benchmarks"))]
use assets_common::{
	local_and_foreign_assets::ForeignAssetReserveData,
	migrations::foreign_assets_reserves::ForeignAssetsReservesProvider,
};
use core::marker::PhantomData;
use frame_support::{
	traits::{Get, OnRuntimeUpgrade},
	weights::Weight,
	BoundedVec,
};
use frame_system::RawOrigin;
use indiv_pallet_chunks_manager::{ChunkPageHashes, WeightInfo};
use indiv_support::{
	genesis::ring_verifier_r2e9_r2e10_builder_params_hashes, traits::RingExponent,
};
use verifiable::ring::RingSuiteExt;
#[cfg(not(feature = "runtime-benchmarks"))]
use xcm::v5::{Junction::Parachain, Location};

const LOG_TARGET: &str = "runtime::next-people-paseo::migrations";

/// Writes the expected chunk page hashes for the 2^9 and 2^10 rings on runtime upgrade.
///
/// A ring is written only when none of its page hashes are stored yet. Chunks are
/// write-once, so overwriting could leave page hashes that no longer match the chunks on chain.
/// Hashes come from the ring verifier builder params of the pinned `verifiable` crate, same as the
/// genesis config. `S` selects the ring suite.
pub struct ChunkPageHashesInitialization<T, S>(PhantomData<(T, S)>);

impl<T, S> OnRuntimeUpgrade for ChunkPageHashesInitialization<T, S>
where
	T: indiv_pallet_chunks_manager::Config,
	S: RingSuiteExt,
{
	fn on_runtime_upgrade() -> Weight {
		let mut weight = Weight::zero();
		let page_size = T::PageSize::get();

		for (exponent, hashes) in ring_verifier_r2e9_r2e10_builder_params_hashes::<S>(page_size) {
			let ring_exponent = match RingExponent::new_from_exponent(exponent) {
				Ok(ring_exponent) => ring_exponent,
				Err(exponent) => {
					log::error!(
						target: LOG_TARGET,
						"chunk page hashes computed for unsupported ring exponent {exponent}"
					);
					continue;
				},
			};

			// A single stored page is enough to treat the ring as owned by whoever wrote it.
			weight.saturating_accrue(T::DbWeight::get().reads(1));
			if ChunkPageHashes::<T>::iter_key_prefix(ring_exponent).next().is_some() {
				log::info!(
					target: LOG_TARGET,
					"chunk page hashes for ring {ring_exponent:?} already set, skipping"
				);
				continue;
			}

			let page_count = hashes.len() as u32;
			let Ok(page_hashes) = BoundedVec::try_from(hashes) else {
				log::error!(
					target: LOG_TARGET,
					"chunk page hashes for ring {ring_exponent:?} exceed the page count limit"
				);
				continue;
			};

			match indiv_pallet_chunks_manager::Pallet::<T>::set_chunk_page_hashes(
				RawOrigin::Root.into(),
				ring_exponent,
				page_hashes,
			) {
				Ok(_) => {
					log::info!(
						target: LOG_TARGET,
						"chunk page hashes for ring {ring_exponent:?} initialized"
					);
					weight.saturating_accrue(
						<T as indiv_pallet_chunks_manager::Config>::WeightInfo::set_chunk_page_hashes(page_count)
					);
				},
				Err(e) => {
					log::error!(
						target: LOG_TARGET,
						"failed to set chunk page hashes for ring {ring_exponent:?}: {e:?}"
					);
				},
			}
		}

		weight
	}

	#[cfg(feature = "try-runtime")]
	fn post_upgrade(_state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
		use frame_support::ensure;

		for (exponent, hashes) in
			ring_verifier_r2e9_r2e10_builder_params_hashes::<S>(T::PageSize::get())
		{
			let ring_exponent = RingExponent::new_from_exponent(exponent)
				.map_err(|_| "unsupported ring exponent")?;
			for page_index in 0..hashes.len() as u32 {
				ensure!(
					ChunkPageHashes::<T>::contains_key(ring_exponent, page_index),
					"chunk page hash missing after initialization"
				);
			}
		}

		Ok(())
	}
}

#[cfg(not(feature = "runtime-benchmarks"))]
fn reserve_data_for(asset_id: &Location) -> Option<ForeignAssetReserveData> {
	let (parents, interior) = asset_id.unpack();
	if parents != 1 {
		return None;
	}
	let reserve = match interior.first() {
		Some(Parachain(id)) => Location::new(1, [Parachain(*id)]),
		_ => return None,
	};
	Some((reserve, false).into())
}

#[cfg(not(feature = "runtime-benchmarks"))]
pub struct PeoplePaseoAssetsReservesProvider;
#[cfg(not(feature = "runtime-benchmarks"))]
impl ForeignAssetsReservesProvider for PeoplePaseoAssetsReservesProvider {
	type ReserveData = ForeignAssetReserveData;

	fn reserves_for(asset_id: &Location) -> Vec<Self::ReserveData> {
		reserve_data_for(asset_id).into_iter().collect()
	}

	#[cfg(feature = "try-runtime")]
	fn check_reserves_for(asset_id: &Location, reserves: Vec<Self::ReserveData>) -> bool {
		reserves == Self::reserves_for(asset_id)
	}
}
