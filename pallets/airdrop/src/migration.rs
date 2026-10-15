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

//! Storage migrations for the airdrop pallet.

use crate::{
	AccountIdOf, ActiveEvent, AirdropPrize, AssetBalanceOf, AssetIdOf, Config, EventId, EventInfo,
	Pallet, ProductName, Status, WeightInfo,
};
use alloc::vec::Vec;
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, storage_alias,
	traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-airdrop::migration";

/// Adds [`ProductName`] to every stored event where the context is irrelevant and cancels the ones
/// which are either scheduled or still registering participants.
///
/// Events in `Scheduled` or `Registering` status are cancelled in the same way
/// [`crate::Airdrop::cancel`] does, because proofs built for the pre-upgrade context do not verify
/// under [`Pallet::context_for_event`]. Events past registration get an empty product name because
/// at that point, it is irrelevant. They accept no further proofs, so the name is never read.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToProductContexts<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// Pre-upgrade layout of [`EventInfo`].
	#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
	pub struct OldEventInfo<AssetId, AssetBalance> {
		pub prize: AirdropPrize<AssetId, AssetBalance>,
		pub registration_starts: u64,
		pub draw_time: u64,
		pub end_time: u64,
	}

	/// Pre-upgrade layout of [`ActiveEvent`].
	#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
	pub struct OldActiveEvent<AccountId, AssetId, AssetBalance> {
		pub id: EventId,
		pub info: OldEventInfo<AssetId, AssetBalance>,
		pub status: Status,
		pub source: Option<AccountId>,
	}

	pub type OldActiveEventOf<T> = OldActiveEvent<AccountIdOf<T>, AssetIdOf<T>, AssetBalanceOf<T>>;

	/// [`crate::Events`] under its pre-upgrade value layout.
	#[storage_alias]
	pub type Events<T: Config> =
		StorageMap<Pallet<T>, Twox64Concat, EventId, OldActiveEventOf<T>, OptionQuery>;

	/// Use [`MigrateV0ToV1`] rather than this directly.
	pub struct MigrateToProductContexts<T>(PhantomData<T>);

	impl<T: Config> MigrateToProductContexts<T> {
		fn awaits_registrations(status: &Status) -> bool {
			matches!(status, Status::Scheduled | Status::Registering { .. })
		}
	}

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToProductContexts<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut translated = 0u64;
			let mut to_cancel = Vec::new();

			crate::Events::<T>::translate::<OldActiveEventOf<T>, _>(|event_id, old| {
				translated.saturating_inc();
				if Self::awaits_registrations(&old.status) {
					to_cancel.push(event_id);
				}
				Some(ActiveEvent {
					id: old.id,
					info: EventInfo {
						product_name: ProductName::default(),
						prize: old.info.prize,
						registration_starts: old.info.registration_starts,
						draw_time: old.info.draw_time,
						end_time: old.info.end_time,
					},
					status: old.status,
					source: old.source,
				})
			});

			for event_id in &to_cancel {
				// `do_cancel` never returns an error.
				let _ = Pallet::<T>::do_cancel(*event_id);
			}

			log::info!(
				target: LOG_TARGET,
				"Added an empty product name to {translated} events and cancelled {} of them which \
				 were still awaiting registrations",
				to_cancel.len(),
			);

			// One read and one write per translated event. A cancellation costs
			// `remove_scheduled_event`, plus one write for the action schedule entry a registering
			// event gains.
			let cancelled = to_cancel.len() as u64;
			T::DbWeight::get()
				.reads_writes(translated.saturating_add(1), translated)
				.saturating_add(
					T::WeightInfo::remove_scheduled_event()
						.saturating_add(T::DbWeight::get().writes(1))
						.saturating_mul(cancelled),
				)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<Vec<u8>, sp_runtime::TryRuntimeError> {
			let mut total = 0u32;
			let mut scheduled = 0u32;
			let mut registering = 0u32;
			for event in Events::<T>::iter_values() {
				total.saturating_inc();
				match event.status {
					Status::Scheduled => scheduled.saturating_inc(),
					Status::Registering { .. } => registering.saturating_inc(),
					_ => {},
				}
			}
			Ok((total, scheduled, registering).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let (total, scheduled, _registering): (u32, u32, u32) =
				Decode::decode(&mut &state[..]).map_err(|_| "invalid pre-upgrade state")?;

			let mut remaining = 0u32;
			for event in crate::Events::<T>::iter_values() {
				remaining.saturating_inc();
				ensure!(
					!Self::awaits_registrations(&event.status),
					"an event still awaiting registrations survived the migration"
				);
				ensure!(
					event.info.product_name.is_empty(),
					"a migrated event carries a non-empty product name"
				);
			}
			// Scheduled events are dropped by the cancellation; registering ones stay until the
			// offchain clean-up finalizes them.
			ensure!(
				remaining == total.saturating_sub(scheduled),
				"unexpected number of events after the migration"
			);
			Ok(())
		}
	}
}
