// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Storage migrations for the Scarcity pallet.

use crate::{
	BalanceOf, CollectionId, Config, ContractNfts, InstanceId, Instances, ItemDefinition, ItemDefs,
	ItemIndex, Location, Nft, NftsByOwner, Pallet, Transferability,
};
use frame_support::{
	migrations::VersionedMigration, pallet_prelude::*, traits::UncheckedOnRuntimeUpgrade,
};
use sp_runtime::Saturating;

const LOG_TARGET: &str = "runtime::indiv-pallet-scarcity::migration";

/// Adds [`Transferability`] to every stored item definition, as [`Transferability::Transferable`].
///
/// `transferability` is a trailing field, so a definition written before it existed decodes to
/// `None` under the current type rather than failing loudly. Every read of that item then reports
/// `UnknownItem`, which takes minting, both transfer paths, burning and deletion with it, and a
/// collection whose items cannot be deleted cannot be deleted either, so its deposit stays held.
pub type MigrateV0ToV1<T> = VersionedMigration<
	0,
	1,
	v1::MigrateToTransferability<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v1 {
	use super::*;

	/// An item definition as stored before item transferability existed.
	#[derive(Decode)]
	pub struct OldItemDefinition<Balance> {
		pub supply: u32,
		pub live_supply: u32,
		pub metadata_count: u32,
		pub deposit: Balance,
	}

	/// Use [`MigrateV0ToV1`] rather than this directly.
	///
	/// Running this twice would reset a soulbound item to transferable: the new encoding is the
	/// old one plus a trailing byte, and `Decode` reads a prefix, so a migrated definition still
	/// decodes as [`OldItemDefinition`] with the flag left over and ignored. The version gate is
	/// what makes that unreachable.
	pub struct MigrateToTransferability<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToTransferability<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut translated = 0u64;
			ItemDefs::<T>::translate_values(|old: OldItemDefinition<BalanceOf<T>>| {
				translated.saturating_inc();
				Some(ItemDefinition {
					supply: old.supply,
					live_supply: old.live_supply,
					metadata_count: old.metadata_count,
					// Deliberately the stored value, not a recomputed one. The new field widens
					// the definition's `MaxEncodedLen` by a byte, but deposits are released from
					// what was charged, and the collection's aggregate is checked against the
					// same figures, so recomputing here would desync both.
					deposit: old.deposit,
					// The only value that preserves behaviour: nothing was bindable before.
					transferability: Transferability::Transferable,
				})
			});
			log::info!(target: LOG_TARGET, "translated {translated} item definitions");
			T::DbWeight::get().reads_writes(translated.saturating_add(1), translated)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// Keys, not entries: the values do not decode under the current type yet, and
			// `iter` would skip every one of them and report an empty map.
			Ok((ItemDefs::<T>::iter_keys().count() as u32).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u32::decode(&mut &state[..]).map_err(|_| {
				sp_runtime::TryRuntimeError::Other("pre_upgrade state is not a u32")
			})?;
			// `iter` now, so an entry that failed to translate is counted as missing rather than
			// silently skipped.
			let after = ItemDefs::<T>::iter().count() as u32;
			ensure!(before == after, "an item definition did not survive the migration");
			Ok(())
		}
	}
}

/// Adds the feeless move counter to every stored NFT, as a full budget.
///
/// `moves` is a trailing field, so an NFT written before it existed no longer decodes under the
/// current type. [`NftsByOwner`] then reports an empty purse key, which takes transfer, burn and
/// every authorization with it.
pub type MigrateV1ToV2<T> = VersionedMigration<
	1,
	2,
	v2::MigrateToMoves<T>,
	Pallet<T>,
	<T as frame_system::Config>::DbWeight,
>;

pub mod v2 {
	use super::*;

	/// An NFT as stored before the feeless move budget existed.
	#[derive(Decode)]
	pub struct OldNft {
		pub instance: InstanceId,
		pub collection: CollectionId,
		pub item: ItemIndex,
		pub minted_at: u64,
		pub last_moved: u64,
		pub state_nonce: u64,
	}

	/// Use [`MigrateV1ToV2`] rather than this directly.
	///
	/// Running this twice would refill the budget of every instance: the new encoding is the old
	/// one plus two trailing bytes, and `Decode` reads a prefix, so a migrated NFT still decodes
	/// as [`OldNft`] with the counter left over and ignored. The version gate is what makes that
	/// unreachable.
	pub struct MigrateToMoves<T>(PhantomData<T>);

	impl<T: Config> UncheckedOnRuntimeUpgrade for MigrateToMoves<T> {
		fn on_runtime_upgrade() -> Weight {
			let mut translated = 0u64;
			NftsByOwner::<T>::translate_values(|old: OldNft| {
				translated.saturating_inc();
				Some(Nft {
					instance: old.instance,
					collection: old.collection,
					item: old.item,
					minted_at: old.minted_at,
					last_moved: old.last_moved,
					state_nonce: old.state_nonce,
					// A full budget. Moves made before this runtime were unbounded, so counting
					// them now would strand an instance already past the new limit.
					moves: 0,
					pledge: None,
				})
			});
			log::info!(target: LOG_TARGET, "translated {translated} NFTs");
			T::DbWeight::get().reads_writes(translated.saturating_add(1), translated)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// Keys, not entries: the values do not decode under the current type yet, and
			// `iter` would skip every one of them and report an empty map.
			Ok((NftsByOwner::<T>::iter_keys().count() as u32).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u32::decode(&mut &state[..]).map_err(|_| {
				sp_runtime::TryRuntimeError::Other("pre_upgrade state is not a u32")
			})?;
			// `iter` now, so an entry that failed to translate is counted as missing rather than
			// silently skipped.
			let after = NftsByOwner::<T>::iter().count() as u32;
			ensure!(before == after, "an NFT did not survive the migration");
			Ok(())
		}
	}
}

/// Changes every [`Instances`] entry from a holder to a [`Location`].
///
/// A [`Location`] names the map that holds the instance and, for a contract, the collection that
/// keys [`ContractNfts`](crate::ContractNfts). An entry written before the change holds only the
/// holder, and it no longer decodes under the current type.
///
/// The migration runs in `pallet-migrations` and translates one entry per step, so it spans as
/// many blocks as the instance count needs. Schedule it in the runtime's multi-block migrations,
/// not in its single-block ones.
pub mod v3 {
	use super::*;
	use crate::WeightInfo;
	use frame_support::{
		migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
		storage_alias,
		weights::WeightMeter,
	};

	const PALLET_MIGRATIONS_ID: &[u8; 21] = b"indiv-pallet-scarcity";

	/// Storage as it was before version 3.
	pub mod old {
		use super::*;

		/// [`Instances`](crate::Instances) with the holder as the value.
		///
		/// The alias takes its storage name from its type name, so it keeps the name `Instances`.
		#[storage_alias]
		pub type Instances<T: Config> =
			StorageMap<Pallet<T>, Twox64Concat, InstanceId, <T as frame_system::Config>::AccountId>;

		/// [`NftsByOwner`](crate::NftsByOwner) read as its leading `instance` field only.
		///
		/// The read decodes a prefix, so it reads an NFT stored before or after version 4 added
		/// the pledge.
		#[storage_alias]
		pub type NftsByOwner<T: Config> = StorageMap<
			Pallet<T>,
			Blake2_128Concat,
			<T as frame_system::Config>::AccountId,
			InstanceId,
		>;
	}

	/// The multi-block migration from version 2 to version 3.
	///
	/// Before version 3 no contract holds an instance, so every holder is a purse key. The
	/// migration removes an entry whose holder does not hold the instance in [`NftsByOwner`].
	/// The cursor is the last instance that a step translated.
	///
	/// Until the migration completes, an entry that it has not reached does not decode. Executive
	/// admits no extrinsic while a multi-block migration runs, so only a runtime API reads it.
	pub struct MigrateToLocations<T>(PhantomData<T>);

	impl<T: Config> MigrateToLocations<T> {
		/// Translate the first entry after `cursor` and return its instance.
		///
		/// Returns `None` and sets the storage version to 3 when no entry is left.
		pub(crate) fn translate_next(cursor: Option<InstanceId>) -> Option<InstanceId> {
			let mut entries = match cursor {
				Some(last) =>
					old::Instances::<T>::iter_from(old::Instances::<T>::hashed_key_for(last)),
				None => old::Instances::<T>::iter(),
			};
			let Some((instance, holder)) = entries.next() else {
				StorageVersion::new(Self::id().version_to as u16).put::<Pallet<T>>();
				log::info!(target: LOG_TARGET, "translated every instance location");
				return None;
			};
			if old::NftsByOwner::<T>::get(&holder) == Some(instance) {
				Instances::<T>::insert(instance, Location::Purse(holder));
			} else {
				log::error!(
					target: LOG_TARGET,
					"Instances entry {instance} has no matching NftsByOwner entry; removed"
				);
				Instances::<T>::remove(instance);
			}
			Some(instance)
		}
	}

	impl<T: Config> SteppedMigration for MigrateToLocations<T> {
		type Cursor = InstanceId;
		type Identifier = MigrationId<21>;

		fn id() -> Self::Identifier {
			MigrationId { pallet_id: *PALLET_MIGRATIONS_ID, version_from: 2, version_to: 3 }
		}

		fn step(
			mut cursor: Option<Self::Cursor>,
			meter: &mut WeightMeter,
		) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
			// A chain built from this code starts at version 3 and has nothing to translate.
			if Pallet::<T>::on_chain_storage_version() != Self::id().version_from as u16 {
				return Ok(None);
			}
			let required = T::WeightInfo::migrate_location();
			if meter.remaining().any_lt(required) {
				return Err(SteppedMigrationError::InsufficientWeight { required });
			}
			while meter.try_consume(required).is_ok() {
				cursor = Self::translate_next(cursor);
				if cursor.is_none() {
					return Ok(None);
				}
			}
			Ok(cursor)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// Keys, not entries: the values do not decode under the current type yet.
			Ok((Instances::<T>::iter_keys().count() as u32).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let before = u32::decode(&mut &state[..]).map_err(|_| {
				sp_runtime::TryRuntimeError::Other("pre_upgrade state is not a u32")
			})?;
			let after = Instances::<T>::iter().count() as u32;
			if before != after {
				log::error!(
					target: LOG_TARGET,
					"{before} instance locations before the migration, {after} after"
				);
				return Err("an instance location did not survive the migration".into());
			}
			Ok(())
		}
	}
}

/// Adds an empty pledge to every stored NFT, in [`NftsByOwner`] and in [`ContractNfts`].
///
/// `pledge` is a trailing field, so an NFT written before it existed no longer decodes under the
/// current type. A purse key then reads as empty and a contract loses the instance from its
/// balance, which takes every move, burn and authorization of the instance with it.
///
/// The migration runs in `pallet-migrations` and translates one NFT per step. Schedule it after
/// [`v3::MigrateToLocations`] in the runtime's multi-block migrations, so a chain at version 2
/// reaches version 4 in one upgrade.
pub mod v4 {
	use super::*;
	use crate::WeightInfo;
	use frame_support::{
		migrations::{MigrationId, SteppedMigration, SteppedMigrationError},
		storage_alias,
		weights::WeightMeter,
	};

	const PALLET_MIGRATIONS_ID: &[u8; 21] = b"indiv-pallet-scarcity";

	/// An NFT as stored before pledges existed.
	#[derive(Encode, Decode)]
	pub struct OldNft {
		pub instance: InstanceId,
		pub collection: CollectionId,
		pub item: ItemIndex,
		pub minted_at: u64,
		pub last_moved: u64,
		pub state_nonce: u64,
		pub moves: u16,
	}

	impl OldNft {
		fn into_nft<AccountId>(self) -> Nft<AccountId> {
			Nft {
				instance: self.instance,
				collection: self.collection,
				item: self.item,
				minted_at: self.minted_at,
				last_moved: self.last_moved,
				state_nonce: self.state_nonce,
				moves: self.moves,
				pledge: None,
			}
		}
	}

	/// Storage as it was before version 4.
	pub mod old {
		use super::*;

		/// [`NftsByOwner`](crate::NftsByOwner) with the NFT before pledges.
		#[storage_alias]
		pub type NftsByOwner<T: Config> =
			StorageMap<Pallet<T>, Blake2_128Concat, <T as frame_system::Config>::AccountId, OldNft>;

		/// [`ContractNfts`](crate::ContractNfts) with the NFT before pledges.
		#[storage_alias]
		pub type ContractNfts<T: Config> = StorageNMap<
			Pallet<T>,
			(
				NMapKey<Blake2_128Concat, <T as frame_system::Config>::AccountId>,
				NMapKey<Twox64Concat, CollectionId>,
				NMapKey<Twox64Concat, InstanceId>,
			),
			OldNft,
		>;
	}

	/// The key of the last NFT that a step translated.
	#[derive(Encode, Decode, MaxEncodedLen, Clone, PartialEq, Eq, Debug)]
	pub enum Cursor<AccountId> {
		/// A purse key in [`NftsByOwner`]. The migration translates every purse key first.
		Purse(AccountId),
		/// A key in [`ContractNfts`].
		Contract(AccountId, CollectionId, InstanceId),
	}

	/// The multi-block migration from version 3 to version 4.
	///
	/// The migration translates [`NftsByOwner`] first and then [`ContractNfts`]. An NFT already
	/// in the current shape decodes as [`OldNft`] with its pledge ignored, so the version gate is
	/// what keeps a second run from clearing a pledge.
	///
	/// Until the migration completes, an NFT that it has not reached does not decode. Executive
	/// admits no extrinsic while a multi-block migration runs, so only a runtime API reads it.
	pub struct MigrateToPledges<T>(PhantomData<T>);

	impl<T: Config> MigrateToPledges<T> {
		/// Translate the first NFT after `cursor` and return its key.
		///
		/// Returns `None` and sets the storage version to 4 when no NFT is left.
		pub(crate) fn translate_next(
			cursor: Option<Cursor<T::AccountId>>,
		) -> Option<Cursor<T::AccountId>> {
			let contract_cursor = match cursor {
				Some(Cursor::Contract(holder, collection, instance)) =>
					Some((holder, collection, instance)),
				purse_cursor => {
					let mut purses = match purse_cursor {
						Some(Cursor::Purse(last)) => old::NftsByOwner::<T>::iter_from(
							old::NftsByOwner::<T>::hashed_key_for(last),
						),
						_ => old::NftsByOwner::<T>::iter(),
					};
					if let Some((holder, nft)) = purses.next() {
						NftsByOwner::<T>::insert(&holder, nft.into_nft::<T::AccountId>());
						return Some(Cursor::Purse(holder));
					}
					None
				},
			};
			let mut contracts = match contract_cursor {
				Some(last) =>
					old::ContractNfts::<T>::iter_from(old::ContractNfts::<T>::hashed_key_for(last)),
				None => old::ContractNfts::<T>::iter(),
			};
			let Some(((holder, collection, instance), nft)) = contracts.next() else {
				StorageVersion::new(Self::id().version_to as u16).put::<Pallet<T>>();
				log::info!(target: LOG_TARGET, "translated every NFT to carry a pledge");
				return None;
			};
			ContractNfts::<T>::insert(
				(&holder, collection, instance),
				nft.into_nft::<T::AccountId>(),
			);
			Some(Cursor::Contract(holder, collection, instance))
		}
	}

	impl<T: Config> SteppedMigration for MigrateToPledges<T> {
		type Cursor = Cursor<T::AccountId>;
		type Identifier = MigrationId<21>;

		fn id() -> Self::Identifier {
			MigrationId { pallet_id: *PALLET_MIGRATIONS_ID, version_from: 3, version_to: 4 }
		}

		fn step(
			mut cursor: Option<Self::Cursor>,
			meter: &mut WeightMeter,
		) -> Result<Option<Self::Cursor>, SteppedMigrationError> {
			// A chain built from this code starts at version 4 and has nothing to translate.
			if Pallet::<T>::on_chain_storage_version() != Self::id().version_from as u16 {
				return Ok(None);
			}
			let required = T::WeightInfo::migrate_pledge();
			if meter.remaining().any_lt(required) {
				return Err(SteppedMigrationError::InsufficientWeight { required });
			}
			while meter.try_consume(required).is_ok() {
				cursor = Self::translate_next(cursor);
				if cursor.is_none() {
					return Ok(None);
				}
			}
			Ok(cursor)
		}

		#[cfg(feature = "try-runtime")]
		fn pre_upgrade() -> Result<alloc::vec::Vec<u8>, sp_runtime::TryRuntimeError> {
			// Keys, not entries: the values do not decode under the current type yet.
			let purses = NftsByOwner::<T>::iter_keys().count() as u32;
			let contracts = ContractNfts::<T>::iter_keys().count() as u32;
			Ok((purses, contracts).encode())
		}

		#[cfg(feature = "try-runtime")]
		fn post_upgrade(state: alloc::vec::Vec<u8>) -> Result<(), sp_runtime::TryRuntimeError> {
			let (purses, contracts) = <(u32, u32)>::decode(&mut &state[..]).map_err(|_| {
				sp_runtime::TryRuntimeError::Other("pre_upgrade state is not two u32s")
			})?;
			// `iter` now, so an NFT that failed to translate is counted as missing rather than
			// silently skipped.
			let purses_after = NftsByOwner::<T>::iter().count() as u32;
			let contracts_after = ContractNfts::<T>::iter().count() as u32;
			if (purses, contracts) != (purses_after, contracts_after) {
				log::error!(
					target: LOG_TARGET,
					"{purses} purse and {contracts} contract NFTs before the migration, \
					 {purses_after} and {contracts_after} after"
				);
				return Err("an NFT did not survive the migration".into());
			}
			Ok(())
		}
	}
}
