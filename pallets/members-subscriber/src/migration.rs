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

//! Storage migrations for the members subscriber pallet.

use crate::{
	Config, Pallet, ProcessingState, RingCollectionState, RingCollectionStates, RingIndex,
	SequenceNumber, UpdatesProcessingState,
};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-members-subscriber::migration";

/// Moves the batch timestamp from the global [`UpdatesProcessingState`] into every
/// [`RingCollectionState`], as `last_batch_received_time`.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToPerCollectionBatchTime<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// The processing state as stored while it carried the batch timestamp.
	#[derive(Decode)]
	pub struct OldUpdatesProcessingState {
		pub last_processed_sequence: SequenceNumber,
		pub last_batch_received_time: u64,
		pub last_replay_request_time: u64,
	}

	/// A collection state as stored before it carried its own batch timestamp.
	#[derive(Decode)]
	pub struct OldRingCollectionState<MaxMissing: Get<u32>, MaxDeleted: Get<u32>> {
		pub ring_count: u32,
		pub next_ring_index: u32,
		pub next_scan_index: u32,
		pub missing_indices: BoundedBTreeMap<RingIndex, u32, MaxMissing>,
		pub deleted_indices: BoundedBTreeSet<RingIndex, MaxDeleted>,
	}

	pub struct MigrateToPerCollectionBatchTime<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToPerCollectionBatchTime<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut global_time = 0u64;
			let result = ProcessingState::<T>::translate::<OldUpdatesProcessingState, _>(|old| {
				let old = old?;
				global_time = old.last_batch_received_time;
				Some(UpdatesProcessingState {
					last_processed_sequence: old.last_processed_sequence,
					last_replay_request_time: old.last_replay_request_time,
				})
			});
			if result.is_err() {
				log::error!(target: LOG_TARGET, "processing state did not decode as the old type");
			}

			let mut translated = 0u64;
			RingCollectionStates::<T>::translate_values(
				|old: OldRingCollectionState<
					T::MaxMissingRootsPerCollection,
					T::MaxDeletedRingsPerCollection,
				>| {
					translated.saturating_inc();
					Some(RingCollectionState {
						ring_count: old.ring_count,
						next_ring_index: old.next_ring_index,
						next_scan_index: old.next_scan_index,
						missing_indices: old.missing_indices,
						deleted_indices: old.deleted_indices,
						last_batch_received_time: global_time,
					})
				},
			);
			log::info!(target: LOG_TARGET, "translated {translated} collection states");

			// The processing state, every collection state and the end of the prefix iteration.
			T::DbWeight::get()
				.reads_writes(translated.saturating_add(2), translated.saturating_add(1))
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// Keys, not entries: the values do not decode under the current type yet, so `iter`
			// would skip every one of them.
			Ok((RingCollectionStates::<T>::iter_keys().count() as u32).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u32::decode(&mut &state[..]).map_err(|_| {
				sp_runtime::TryRuntimeError::Other("pre_upgrade state is not a u32")
			})?;
			// `iter` now, so an entry that failed to translate counts as missing.
			let after = RingCollectionStates::<T>::iter().count() as u32;
			ensure!(before == after, "a collection state did not survive the migration");
			Ok(())
		}
	}
}
