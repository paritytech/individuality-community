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

//! # DotNS Gateway Pallet
//!
//! Gateway for verified persons (People and People Lite) to reserve and register usernames
//! in the dotNS smart contracts via pallet_revive.
//!
//! ## Overview
//!
//! This pallet:
//! - Accepts attester-based reservations for Lite People, verifying the attester's allowance and
//!   the user's signature.
//! - Accepts ring membership proofs for Full People username registration, verified against
//!   `members-subscriber` rings.
//! - Encodes EVM ABI calldata and dispatches contract calls to the `RootGatewayDispatcher` (which
//!   forwards to `DotnsPopController`) via a `ContractCaller` trait.
//!
//! ## Wiring
//!
//! The dispatcher contract checks whether the caller is root, so the runtime's `ContractCaller`
//! implementation must invoke the dispatcher as `RuntimeOrigin::root()`; non-Root calls
//! revert with `NotRoot`, surfaced here as
//! `Error::ContractRevert(DispatcherRevert::NotRoot)`.
//!
//! ## Maintainer account
//!
//! The pallet holds a keyless account derived from [`Config::PalletId`]. The dotNS contracts
//! accept this account's EVM address as their maintainer, which may tune bounded operational
//! values. [`Config::MaintenanceOrigin`] acts through the account with
//! [`Pallet::call_as_maintainer`].
//!
//! The account calls only the contract functions in [`AllowedMaintainerCalls`], each identified by
//! the contract address and the function selector. [`Config::MaintainerCallsManager`] sets this
//! list, so `MaintenanceOrigin` reaches only the functions that this stricter origin allows.
//!
//! `pallet-revive` accepts a signed caller only when its account is mapped to an EVM address.
//! Before the first call, the pallet gives the account a provider reference, so the account exists
//! without a balance. The runtime must map new accounts automatically (`pallet_revive::AutoMapper`
//! as `OnNewAccount` with `AutoMap` set), or map the account itself. The account needs a balance
//! only for calls that pay a storage deposit.
//!
//! ## Limits
//!
//! [`Limits`] holds the contract call weight, the signature validity and the future clock skew.
//! [`Config::MaintenanceOrigin`] sets them with [`Pallet::set_limits`], within bounds that the
//! runtime fixes. The bounds stop one referendum from setting a value that disrupts the gateway:
//!
//! - A contract call weight below the cost of the heaviest gateway call makes that call fail. A
//!   weight above one normal extrinsic makes every gateway call fail.
//! - A signature validity shorter than the time from signing to inclusion makes reservations
//!   expire. A long one lets an attester hold a reusable consent for longer.
//! - A future skew below the clock drift of the signing devices rejects their signatures. Each
//!   second of skew also extends how long a signature stays valid.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;
pub mod extension;
pub mod migration;
pub mod types;
pub mod weights;

pub use extension::{AsDotnsGateway, AsDotnsGatewayInfo};

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use pallet::*;
pub use types::*;
pub use weights::WeightInfo;

use frame_support::{
	traits::{OriginTrait, UnixTime},
	PalletId,
};
use indiv_support::{
	context::{build_product_context, personhood},
	traits::{Alias, MembershipProver, RevisionIndex, RingExponent, RingIndex},
};
use sp_runtime::traits::{AccountIdConversion, IdentifyAccount, Verify};
use verifiable::GenerateVerifiable;

#[frame_support::pallet]
pub mod pallet {

	use super::*;
	use alloc::vec::Vec;
	use frame_support::{
		dispatch::{DispatchErrorWithPostInfo, PostDispatchInfo},
		pallet_prelude::*,
	};
	use frame_system::pallet_prelude::*;
	use sp_core::H160;

	/// Message prefix for candidate signature verification during name reservation.
	pub const RESERVE_MSG_PREFIX: &[u8] = b"pop:dotns-gateway:reserve";

	const LOG_TARGET: &str = "runtime::indiv-pallet-dotns-gateway";

	/// The in-code storage version. Bump it and add a migration when the layout of a
	/// storage item changes; see [`migration`].
	const STORAGE_VERSION: StorageVersion = StorageVersion::new(1);

	#[pallet::pallet]
	#[pallet::storage_version(STORAGE_VERSION)]
	pub struct Pallet<T>(_);

	#[pallet::config]
	pub trait Config:
		frame_system::Config<
		RuntimeOrigin: From<Origin>
		                   + From<<Self::RuntimeOrigin as OriginTrait>::PalletsOrigin>
		                   + OriginTrait<
			PalletsOrigin: From<Origin>
			                   + TryInto<
				Origin,
				Error = <Self::RuntimeOrigin as OriginTrait>::PalletsOrigin,
			>,
		>,
		RuntimeCall: frame_support::traits::IsSubType<Call<Self>>,
	>
	{
		/// Weight information for extrinsics in this pallet.
		type WeightInfo: WeightInfo;

		/// Runtime-wide network suffix used to derive product contexts.
		type Suffix: Get<indiv_support::context::ProductContextNetworkSuffix>;

		/// Ring-membership prover used to verify proofs sent to [`Pallet::register_name`].
		type MemberService: MembershipProver<
			Crypto: GenerateVerifiable<
				Config: TryFrom<RingExponent>,
				Proof: Send + Sync,
				Signature: Send + Sync,
			>,
		>;

		/// Contract call abstraction (backed by pallet_revive in production).
		type ContractCaller: ContractCaller + SignedContractCaller<Self::AccountId>;

		/// Converts AccountId to EVM H160 address.
		///
		/// In production, wired to pallet_revive's `AccountId32Mapper`.
		type AddressMapper: AddressMapper<Self::AccountId>;

		/// The limits that apply until [`Pallet::set_limits`] sets others.
		#[pallet::constant]
		type DefaultLimits: Get<GatewayLimits>;

		/// The lowest limits that [`Pallet::set_limits`] accepts.
		#[pallet::constant]
		type MinLimits: Get<GatewayLimits>;

		/// The highest limits that [`Pallet::set_limits`] accepts.
		///
		/// The pallet overhead plus this contract call weight must fit one normal extrinsic.
		#[pallet::constant]
		type MaxLimits: Get<GatewayLimits>;

		/// Unix time source.
		type UnixTime: UnixTime;

		/// The origin that can manage attestation allowances.
		type AttestationAllowanceManager: EnsureOrigin<Self::RuntimeOrigin>;

		/// The origin that can set the `RootGatewayDispatcher` contract address.
		type DispatcherAddressManager: EnsureOrigin<Self::RuntimeOrigin>;

		/// Identifier from which the maintainer account derives.
		#[pallet::constant]
		type PalletId: Get<PalletId>;

		/// The origin that can call contracts as the maintainer account and set the limits.
		type MaintenanceOrigin: EnsureOrigin<Self::RuntimeOrigin>;

		/// The origin that sets which contract functions the maintainer account can call.
		///
		/// Use an origin at least as strict as `MaintenanceOrigin`.
		type MaintainerCallsManager: EnsureOrigin<Self::RuntimeOrigin>;

		/// Signature type used by candidates to consent to name reservations.
		type AttestationSignature: Verify<Signer: IdentifyAccount<AccountId = Self::AccountId>>
			+ Parameter
			+ Send
			+ Sync;

		/// Benchmark helper.
		#[cfg(feature = "runtime-benchmarks")]
		type BenchmarkHelper: benchmarking::BenchmarkHelper<Self>;
	}

	// ========== Storage Items ==========

	/// Tracks which aliases have registered names via this gateway.
	#[pallet::storage]
	pub type AliasRegistration<T: Config> =
		StorageMap<_, Blake2_128Concat, Alias, RegistrationRecord<T>, OptionQuery>;

	/// Reverse lookup from a registering account to the alias it used.
	///
	/// Populated alongside [`AliasRegistration`] in [`Pallet::register_name`].
	#[pallet::storage]
	pub type AccountAlias<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, Alias, OptionQuery>;

	/// Number of username reservations an attester is allowed to grant.
	#[pallet::storage]
	pub type AttestationAllowance<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

	/// Records which account owns each reserved lite-person label, so
	/// [`Pallet::register_name`] can verify that a `Link::LiteUsername` target
	/// belongs to the caller.
	#[pallet::storage]
	pub type LiteLabelOwner<T: Config> =
		StorageMap<_, Blake2_128Concat, BaseLabel, T::AccountId, OptionQuery>;

	/// The dotNS labels each account acquired through this gateway.
	///
	/// Keyed by account so clients can watch a set of accounts with one storage
	/// subscription.
	///
	/// Warning: this map is not the source of truth; the dotNS contracts are. It
	/// is written on [`Pallet::reserve_name`] and [`Pallet::register_name`] only,
	/// so a label that changes purely contract-side (for example a transfer) is
	/// not reflected here and clients must re-verify on read via contract views.
	/// Names registered outside these paths never appear here at all, including
	/// pre-launch whitelist allocations, which claimants register directly against
	/// the dotNS registrar controller.
	/// The map is temporary and goes away once apps can observe contract storage
	/// directly: <https://github.com/paritytech/individuality-community/issues/52>.
	#[pallet::storage]
	pub type AccountNames<T: Config> =
		StorageMap<_, Blake2_128Concat, T::AccountId, AccountNameRecord, OptionQuery>;

	/// Address of the `RootGatewayDispatcher` contract. Must be set (via genesis or
	/// [`Pallet::set_dispatcher_address`]) before [`Pallet::reserve_name`] or
	/// [`Pallet::register_name`] can succeed.
	#[pallet::storage]
	pub type DispatcherAddress<T: Config> = StorageValue<_, H160, OptionQuery>;

	/// The default for [`Limits`].
	#[pallet::type_value]
	pub fn DefaultLimitsValue<T: Config>() -> GatewayLimits {
		T::DefaultLimits::get()
	}

	/// The operational limits in force. Holds [`Config::DefaultLimits`] until
	/// [`Pallet::set_limits`] writes it.
	#[pallet::storage]
	pub type Limits<T: Config> = StorageValue<_, GatewayLimits, ValueQuery, DefaultLimitsValue<T>>;

	/// The contract functions that [`Pallet::call_as_maintainer`] can call, keyed by contract
	/// address and function selector.
	#[pallet::storage]
	pub type AllowedMaintainerCalls<T: Config> =
		StorageDoubleMap<_, Blake2_128Concat, H160, Blake2_128Concat, [u8; 4], (), OptionQuery>;

	// ========== Origin ==========

	/// Origin for person calls in this pallet.
	#[pallet::origin]
	#[derive(
		Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
	)]
	pub enum Origin {
		/// A full-person registration that has been authenticated by [`AsDotnsGateway`].
		PersonRegistration(Alias),
	}

	// ========== Events ==========

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A username has been reserved for a candidate by an attester.
		NameReserved {
			/// The candidate whose name was reserved.
			candidate: T::AccountId,
			/// The attester who performed the reservation.
			attester: T::AccountId,
			/// The lite-person label (`<dns-stem>.<digits>`).
			lite_label: BaseLabel,
			/// The ECDH chat key stored alongside the lite-person label.
			chat_key: ChatKey,
			/// The optional base label reserved for future full-person claiming.
			reserved_base_label: Option<BaseLabel>,
		},
		/// A username has been registered by a verified person.
		NameRegistered {
			/// The alias derived from the ring proof.
			alias: Alias,
			/// The account that initiated the registration.
			account: T::AccountId,
			/// The registered full-person label.
			label: BaseLabel,
			/// How the full-person label relates to a lite-person label.
			link: Link,
		},
		/// Attestation allowance was increased for an account.
		AttestationAllowanceIncreased {
			/// The attester account.
			account: T::AccountId,
			/// The number of attestations added.
			count: u32,
		},
		/// All attestation allowance has been removed for the attester.
		AllAttestationAllowanceCleared {
			/// The attester account.
			attester: T::AccountId,
		},
		/// The `RootGatewayDispatcher` contract address was set.
		DispatcherAddressSet {
			/// The new contract address.
			address: H160,
		},
		/// The maintainer account called a contract.
		MaintainerCallExecuted {
			/// The called contract.
			dest: H160,
			/// The function selector of the call.
			selector: [u8; 4],
		},
		/// The maintainer account can call a contract function.
		MaintainerCallAllowed {
			/// The contract.
			dest: H160,
			/// The function selector.
			selector: [u8; 4],
		},
		/// The maintainer account can no longer call a contract function.
		MaintainerCallDisallowed {
			/// The contract.
			dest: H160,
			/// The function selector.
			selector: [u8; 4],
		},
		/// The operational limits were set.
		LimitsSet {
			/// The new limits.
			limits: GatewayLimits,
		},
	}

	// ========== Errors ==========

	#[pallet::error]
	pub enum Error<T> {
		/// The alias has already registered a name.
		AlreadyRegistered,
		/// The contract call failed (non-revert: out-of-gas, low-level, or
		/// undecodable revert).
		ContractCallFailed,
		/// Contract reverted with a typed error from `RootGatewayDispatcher` or
		/// the `DotnsPopController` it forwards to.
		ContractRevert(DispatcherRevert),
		/// The name is invalid (empty or too long, or does not match the
		/// required DNS/lite-label format).
		InvalidName,
		/// The attester has no attestation allowance remaining.
		NoAttestationAllowance,
		/// The candidate's signature is invalid.
		InvalidAttestationSignature,
		/// The reservation signature is older than the signature validity in [`Limits`].
		ReservationSignatureExpired,
		/// The reservation signature is dated further ahead of chain time than the future skew in
		/// [`Limits`] allows.
		ReservationSignatureFromFuture,
		/// The `Link::LiteUsername` target is not owned by the caller (or has
		/// no on-chain ownership record).
		NotLiteLabelOwner,
		/// The `RootGatewayDispatcher` contract address has not been set.
		DispatcherAddressNotSet,
		/// The candidate already holds a device name.
		AccountAlreadyHasDeviceName,
		/// The call data is shorter than a 4-byte function selector.
		MissingSelector,
		/// A limit is outside [`Config::MinLimits`] and [`Config::MaxLimits`].
		LimitsOutOfBounds,
		/// The contract function is not in [`AllowedMaintainerCalls`].
		MaintainerCallNotAllowed,
	}

	// ========== Extrinsics ==========

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Reserves a lite-person label on `DotnsPopController` (via `RootGatewayDispatcher`)
		/// for a Lite Person, and optionally enqueues a reservation for a base label the user
		/// intends to claim as a Full Person later.
		///
		/// The caller must be an attester with available allowance. The user's
		/// signature proves their consent to the reservation with this specific label
		/// and attester. A candidate that already holds a device name is rejected.
		///
		/// # Parameters
		/// - `candidate`: the Lite Person's account for whom the label is being reserved. Also the
		///   signer of `candidate_signature`.
		/// - `candidate_signature`: signature over the bytes returned by
		///   `Pallet::construct_reservation_message` in the personhood signing context (see
		///   [`Pallet::account_signing_payload`]). The message is a SCALE-encoded tuple binding the
		///   attester, candidate, the candidate's chosen *base* username and chat key. The digit
		///   suffix of `lite_label` is **not** part of the signed message (the digits are allocated
		///   server-side after the candidate signs, so the candidate cannot commit to them at
		///   signing time).
		/// - `lite_label`: the lite-person label. Must be `<dns-stem>.<2+ digits>` (e.g.
		///   `alice.42`), matching `StringUtils.isLitePersonLabel` in the contracts. Only the
		///   `<dns-stem>` portion enters the signed message; the digits ride along as an unsigned
		///   extrinsic argument used for the contract calldata and storage key.
		/// - `chat_key`: 65-byte public key used for ECDH chat.
		/// - `reserved_base_label`: optional personhood label queued for the candidate, to issue
		///   later as their personhood name. Must be lowercase ASCII letters only (e.g. `alice`).
		/// - `signed_at`: Unix-time second at which the candidate signed the reservation message.
		///   The signature is accepted only if `signed_at <= now + future_skew_seconds` and `now <=
		///   signed_at + signature_validity_seconds`, where the limits come from [`Limits`] and
		///   `now` comes from [`Config::UnixTime`].
		#[pallet::call_index(0)]
		#[pallet::weight(
			<T as Config>::WeightInfo::reserve_name()
				.saturating_add(T::MaxLimits::get().contract_call_weight)
		)]
		pub fn reserve_name(
			origin: OriginFor<T>,
			candidate: T::AccountId,
			candidate_signature: T::AttestationSignature,
			lite_label: BaseLabel,
			chat_key: ChatKey,
			reserved_base_label: Option<BaseLabel>,
			signed_at: u64,
		) -> DispatchResultWithPostInfo {
			let attester = ensure_signed(origin)?;

			ensure!(lite_label.is_valid_lite(), Error::<T>::InvalidName);
			if let Some(ref label) = reserved_base_label {
				ensure!(label.is_valid_person(), Error::<T>::InvalidName);
			}
			ensure!(
				AccountNames::<T>::get(&candidate).and_then(|record| record.lite).is_none(),
				Error::<T>::AccountAlreadyHasDeviceName,
			);

			let limits = Limits::<T>::get();
			let now = <T as Config>::UnixTime::now().as_secs();
			ensure!(
				signed_at <= now.saturating_add(limits.future_skew_seconds),
				Error::<T>::ReservationSignatureFromFuture,
			);
			ensure!(
				now <= signed_at.saturating_add(limits.signature_validity_seconds),
				Error::<T>::ReservationSignatureExpired,
			);

			// Verifying the candidate's signature.
			let msg = Self::construct_reservation_message(
				&candidate,
				&attester,
				lite_label.lite_base(),
				chat_key.as_bytes(),
				reserved_base_label.as_ref().map(BaseLabel::as_slice),
				signed_at,
			);
			ensure!(
				candidate_signature.verify(&Self::account_signing_payload(&msg)[..], &candidate),
				Error::<T>::InvalidAttestationSignature,
			);

			// Checking and decrementing attestation allowance
			let available = AttestationAllowance::<T>::get(&attester)
				.checked_sub(1)
				.ok_or(Error::<T>::NoAttestationAllowance)?;

			if available > 0 {
				AttestationAllowance::<T>::insert(&attester, available);
			} else {
				AttestationAllowance::<T>::remove(&attester);
			}

			// Encoding and calling the contract
			let candidate_h160 = T::AddressMapper::to_address(&candidate);
			let calldata = match reserved_base_label.as_ref() {
				Some(base) => issueDeviceNameWithReservationCall::new_abi_encoded(
					lite_label.as_slice(),
					candidate_h160,
					chat_key.as_slice(),
					base.as_slice(),
				),
				None => issueDeviceNameCall::new_abi_encoded(
					lite_label.as_slice(),
					candidate_h160,
					chat_key.as_slice(),
				),
			};
			let contract_weight =
				Self::call_dispatcher(calldata, <T as Config>::WeightInfo::reserve_name())?;

			LiteLabelOwner::<T>::insert(&lite_label, &candidate);
			AccountNames::<T>::mutate(&candidate, |record| {
				record.get_or_insert_with(AccountNameRecord::default).lite =
					Some(NameEntry { label: lite_label.clone(), chat: Some(chat_key) });
			});

			Self::deposit_event(Event::NameReserved {
				candidate,
				attester,
				lite_label,
				chat_key,
				reserved_base_label,
			});
			Ok(PostDispatchInfo {
				actual_weight: Some(
					<T as Config>::WeightInfo::reserve_name().saturating_add(contract_weight),
				),
				pays_fee: Pays::No,
			})
		}

		/// Registers a full-person label on `DotnsPopController` (via `RootGatewayDispatcher`)
		/// for a fully proven person.
		///
		/// The origin must be [`Origin::PersonRegistration`], produced by [`AsDotnsGateway`] after
		/// it verifies the ring membership proof and the off-chain signature from `who`. All
		/// authentication, format and replay checks live in the extension.
		///
		/// # Parameters
		/// - `who`: the account that owns the full-person label; its EVM-mapped address is the
		///   recipient on the `DotnsPopController` contract. Must match the `who` already
		///   authenticated by [`AsDotnsGateway`].
		/// - `label`: the personhood label. Must be lowercase ASCII letters only (e.g. `alice`),
		///   matching `StringUtils.isPersonhoodLabel` in the contracts.
		/// - `link`: how the full-person label relates to a pre-existing lite-person label:
		///   - `Link::LiteUsername(lite_label)` — links to an existing lite-person label (must
		///     match the lite format `<dns-stem>.<2+ digits>`); the chat key is inherited from the
		///     lite entry.
		///   - `Link::None(chat_key)` — standalone registration with a fresh ECDH chat key.
		#[pallet::call_index(1)]
		#[pallet::weight(
			<T as Config>::WeightInfo::register_name()
				.saturating_add(T::MaxLimits::get().contract_call_weight)
		)]
		pub fn register_name(
			origin: OriginFor<T>,
			who: T::AccountId,
			label: BaseLabel,
			link: Link,
		) -> DispatchResultWithPostInfo {
			let alias = Self::ensure_person_registration(origin)?;

			let who_h160 = T::AddressMapper::to_address(&who);
			let calldata =
				issuePersonhoodNameCall::new_abi_encoded(label.as_slice(), who_h160, &link);
			let contract_weight =
				Self::call_dispatcher(calldata, <T as Config>::WeightInfo::register_name())?;

			AliasRegistration::<T>::insert(
				alias,
				RegistrationRecord { collection: Collection::People, account: who.clone() },
			);
			AccountAlias::<T>::insert(&who, alias);
			AccountNames::<T>::mutate(&who, |record| {
				let record = record.get_or_insert_with(AccountNameRecord::default);
				let chat = match &link {
					Link::None(chat_key) => Some(*chat_key),
					// The contracts copy the lite label's key to the full label. The pallet
					// holds that key only when the linked label is the recorded one.
					Link::LiteUsername(lite) => record
						.lite
						.as_ref()
						.filter(|entry| entry.label == *lite)
						.and_then(|entry| entry.chat),
				};
				record.full = Some(NameEntry { label: label.clone(), chat });
			});

			Self::deposit_event(Event::NameRegistered { alias, account: who, label, link });
			Ok(PostDispatchInfo {
				actual_weight: Some(
					<T as Config>::WeightInfo::register_name().saturating_add(contract_weight),
				),
				pays_fee: Pays::No,
			})
		}

		/// Grants attestation allowance to an attester account.
		///
		/// The origin must be `AttestationAllowanceManager`.
		#[pallet::call_index(2)]
		#[pallet::weight(<T as Config>::WeightInfo::increase_attestation_allowance())]
		pub fn increase_attestation_allowance(
			origin: OriginFor<T>,
			account: T::AccountId,
			count: u32,
		) -> DispatchResult {
			T::AttestationAllowanceManager::ensure_origin(origin)?;
			AttestationAllowance::<T>::mutate(&account, |old| {
				*old = old.saturating_add(count);
			});
			Self::deposit_event(Event::AttestationAllowanceIncreased { account, count });
			Ok(())
		}

		/// Clears all attestation allowance for an attester account.
		///
		/// The origin must be `AttestationAllowanceManager`.
		#[pallet::call_index(3)]
		#[pallet::weight(<T as Config>::WeightInfo::clear_attestation_allowance())]
		pub fn clear_attestation_allowance(
			origin: OriginFor<T>,
			account: T::AccountId,
		) -> DispatchResult {
			T::AttestationAllowanceManager::ensure_origin(origin)?;
			AttestationAllowance::<T>::remove(&account);
			Self::deposit_event(Event::AllAttestationAllowanceCleared { attester: account });
			Ok(())
		}

		/// Sets the `RootGatewayDispatcher` contract address.
		///
		/// The origin must be `DispatcherAddressManager`. Overwrites any prior value.
		#[pallet::call_index(4)]
		#[pallet::weight(<T as Config>::WeightInfo::set_dispatcher_address())]
		pub fn set_dispatcher_address(origin: OriginFor<T>, address: H160) -> DispatchResult {
			T::DispatcherAddressManager::ensure_origin(origin)?;
			DispatcherAddress::<T>::put(address);
			Self::deposit_event(Event::DispatcherAddressSet { address });
			Ok(())
		}

		/// Calls the contract at `dest` with the ABI-encoded `data` as the maintainer account.
		///
		/// The origin must be `MaintenanceOrigin`, and [`AllowedMaintainerCalls`] must hold `dest`
		/// with the selector of `data`. The call transfers no value. The maintainer account pays
		/// the storage deposit, up to `deposit_limit`.
		#[pallet::call_index(5)]
		#[pallet::weight(
			<T as Config>::WeightInfo::call_as_maintainer().saturating_add(*weight_limit)
		)]
		pub fn call_as_maintainer(
			origin: OriginFor<T>,
			dest: H160,
			data: Vec<u8>,
			weight_limit: Weight,
			#[pallet::compact] deposit_limit: u128,
		) -> DispatchResultWithPostInfo {
			T::MaintenanceOrigin::ensure_origin(origin)?;
			let selector = *data.first_chunk::<4>().ok_or(Error::<T>::MissingSelector)?;
			ensure!(
				AllowedMaintainerCalls::<T>::contains_key(dest, selector),
				Error::<T>::MaintainerCallNotAllowed
			);

			let maintainer = Self::maintainer_account();
			if frame_system::Pallet::<T>::providers(&maintainer) == 0 {
				frame_system::Pallet::<T>::inc_providers(&maintainer);
			}

			let (_, contract_weight) =
				T::ContractCaller::call_as(&maintainer, dest, data, weight_limit, deposit_limit)
					.map_err(|err| {
						Self::contract_call_failure(
							<T as Config>::WeightInfo::call_as_maintainer(),
							err,
						)
					})?;

			Self::deposit_event(Event::MaintainerCallExecuted { dest, selector });
			Ok(Some(
				<T as Config>::WeightInfo::call_as_maintainer().saturating_add(contract_weight),
			)
			.into())
		}

		/// Sets the operational limits.
		///
		/// The origin must be `MaintenanceOrigin`. Each limit must be within
		/// [`Config::MinLimits`] and [`Config::MaxLimits`].
		#[pallet::call_index(6)]
		#[pallet::weight(<T as Config>::WeightInfo::set_limits())]
		pub fn set_limits(origin: OriginFor<T>, limits: GatewayLimits) -> DispatchResult {
			T::MaintenanceOrigin::ensure_origin(origin)?;
			ensure!(
				limits.is_within(&T::MinLimits::get(), &T::MaxLimits::get()),
				Error::<T>::LimitsOutOfBounds
			);
			Limits::<T>::put(&limits);
			Self::deposit_event(Event::LimitsSet { limits });
			Ok(())
		}

		/// Lets [`Pallet::call_as_maintainer`] call the function `selector` of the contract at
		/// `dest`.
		///
		/// The origin must be `MaintainerCallsManager`.
		#[pallet::call_index(7)]
		#[pallet::weight(<T as Config>::WeightInfo::allow_maintainer_call())]
		pub fn allow_maintainer_call(
			origin: OriginFor<T>,
			dest: H160,
			selector: [u8; 4],
		) -> DispatchResult {
			T::MaintainerCallsManager::ensure_origin(origin)?;
			AllowedMaintainerCalls::<T>::insert(dest, selector, ());
			Self::deposit_event(Event::MaintainerCallAllowed { dest, selector });
			Ok(())
		}

		/// Stops [`Pallet::call_as_maintainer`] from calling the function `selector` of the
		/// contract at `dest`.
		///
		/// The origin must be `MaintainerCallsManager`.
		#[pallet::call_index(8)]
		#[pallet::weight(<T as Config>::WeightInfo::disallow_maintainer_call())]
		pub fn disallow_maintainer_call(
			origin: OriginFor<T>,
			dest: H160,
			selector: [u8; 4],
		) -> DispatchResult {
			T::MaintainerCallsManager::ensure_origin(origin)?;
			AllowedMaintainerCalls::<T>::remove(dest, selector);
			Self::deposit_event(Event::MaintainerCallDisallowed { dest, selector });
			Ok(())
		}
	}

	// ========== Hooks ==========

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn integrity_test() {
			let max = T::MaxLimits::get();
			assert!(
				T::DefaultLimits::get().is_within(&T::MinLimits::get(), &max),
				"`DefaultLimits` must be within `MinLimits` and `MaxLimits`",
			);
			let overhead = <T as Config>::WeightInfo::reserve_name().max(
				<T as Config>::WeightInfo::register_name()
					.saturating_add(<T as Config>::WeightInfo::as_register_full_name_tx_ext()),
			);
			let worst_case = overhead.saturating_add(max.contract_call_weight);
			let max_extrinsic = indiv_support::weight_budget::normal_max_extrinsic::<T>();
			assert!(
				worst_case.all_lte(max_extrinsic),
				"a gateway call with the maximum contract call weight takes {worst_case:?}, more \
				 than one normal extrinsic allows ({max_extrinsic:?})",
			);
		}
	}

	// ========== Internal Functions ==========

	impl<T: Config> Pallet<T> {
		/// Builds the payload the candidate signs to authorize a
		/// name reservation.
		pub(crate) fn construct_reservation_message(
			candidate: &T::AccountId,
			attester: &T::AccountId,
			username_base: &[u8],
			chat_key: &[u8; 65],
			reserved_base_label: Option<&[u8]>,
			signed_at: u64,
		) -> Vec<u8> {
			(
				RESERVE_MSG_PREFIX,
				candidate,
				attester,
				username_base,
				chat_key.as_slice(),
				reserved_base_label,
				signed_at,
			)
				.encode()
		}

		/// Hashes the registration intent (`who`, `label`, `link`) into a
		/// 32-byte digest.
		pub(crate) fn construct_register_proof_message(
			who: &T::AccountId,
			label: &[u8],
			link: &Link,
		) -> [u8; 32] {
			(who, label, link).using_encoded(sp_io::hashing::blake2_256)
		}

		/// Calls `RootGatewayDispatcher` with the given ABI-encoded `calldata`
		/// (which the dispatcher forwards to `DotnsPopController`) and returns
		/// the weight reported by the contract caller.
		///
		/// A failed call is charged `base` plus the weight the contract used.
		fn call_dispatcher(
			calldata: Vec<u8>,
			base: Weight,
		) -> Result<Weight, DispatchErrorWithPostInfo> {
			let address =
				DispatcherAddress::<T>::get().ok_or(Error::<T>::DispatcherAddressNotSet)?;
			T::ContractCaller::call(address, calldata, 0, Limits::<T>::get().contract_call_weight)
				.map(|(_, weight)| weight)
				.map_err(|err| Self::contract_call_failure(base, err))
		}

		/// Maps a failed contract call to its pallet error, charged `base` plus the weight the
		/// contract used, so a failure refunds the unused contract call budget like a success.
		fn contract_call_failure(
			base: Weight,
			err: ContractCallError,
		) -> DispatchErrorWithPostInfo {
			let actual_weight = base.saturating_add(err.weight_consumed);
			DispatchErrorWithPostInfo {
				post_info: Some(actual_weight).into(),
				error: Error::<T>::from(err).into(),
			}
		}

		/// Verifies a ring membership proof against the `MemberService`.
		pub(crate) fn verify_proof(
			collection: &Collection,
			ring_index: RingIndex,
			revision: RevisionIndex,
			proof: &ProofOf<T>,
			message: &[u8],
		) -> Result<Alias, DispatchError> {
			let identifier = collection.identifier();
			let context = Self::proof_context();
			let validated = T::MemberService::verify_membership(
				identifier, proof, ring_index, revision, context, message,
			)?;
			Ok(validated.alias)
		}
	}

	#[pallet::extra_constants]
	impl<T: Config> Pallet<T> {
		/// The personhood signing context on this network. An account signs this context
		/// followed by the message when it signs a message that is not a transaction; see
		/// [`Pallet::account_signing_payload`].
		#[pallet::constant_name(SigningContext)]
		pub fn signing_context() -> indiv_support::traits::Context {
			indiv_support::context::signing_context(personhood::PRODUCT_NAME, &T::Suffix::get())
		}

		/// The keyless account that calls contracts in [`Pallet::call_as_maintainer`]. The dotNS
		/// contracts know it by its EVM address.
		#[pallet::constant_name(MaintainerAccount)]
		pub fn maintainer_account() -> T::AccountId {
			T::PalletId::get().into_account_truncating()
		}
	}

	impl<T: Config> Pallet<T> {
		/// Returns the bytes an account signs for `msg`: the personhood signing context followed by
		/// `msg`.
		pub fn account_signing_payload(msg: &[u8]) -> Vec<u8> {
			[&Self::signing_context()[..], msg].concat()
		}

		pub fn proof_context() -> indiv_support::traits::Context {
			build_product_context(
				personhood::PRODUCT_NAME,
				&T::Suffix::get(),
				personhood::DOTNS_GATEWAY,
			)
		}

		/// Extracts the alias carried by [`Origin::PersonRegistration`], fails for any other
		/// origin.
		pub(crate) fn ensure_person_registration(
			origin: OriginFor<T>,
		) -> Result<Alias, DispatchError> {
			match origin.into_caller().try_into() {
				Ok(Origin::PersonRegistration(alias)) => Ok(alias),
				_ => Err(DispatchError::BadOrigin),
			}
		}
	}

	impl<T: Config> From<ContractCallError> for Error<T> {
		fn from(err: ContractCallError) -> Self {
			let Some(bytes) = err.revert_data.as_deref() else {
				return Error::<T>::ContractCallFailed;
			};
			match decode_revert(bytes) {
				Some(variant) => Error::<T>::ContractRevert(variant),
				None => {
					log::warn!(
						target: LOG_TARGET,
						"unknown contract revert selector; data = {bytes:?}",
					);
					Error::<T>::ContractCallFailed
				},
			}
		}
	}

	// ========== Genesis ==========

	#[pallet::genesis_config]
	#[derive(frame_support::DefaultNoBound)]
	pub struct GenesisConfig<T: Config> {
		/// Initial `RootGatewayDispatcher` contract address. When `None`, the
		/// address must be set via [`Pallet::set_dispatcher_address`] before
		/// [`Pallet::reserve_name`] or [`Pallet::register_name`] can succeed.
		pub dispatcher_address: Option<H160>,
		#[serde(skip)]
		pub _phantom: PhantomData<T>,
	}

	#[pallet::genesis_build]
	impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
		fn build(&self) {
			if let Some(address) = self.dispatcher_address {
				DispatcherAddress::<T>::put(address);
			}
		}
	}
}
