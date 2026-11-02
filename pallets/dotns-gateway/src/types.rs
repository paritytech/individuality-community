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

//! Types for the dotNS gateway pallet.

use alloc::{string::String, vec::Vec};
use alloy_core::{
	primitives::{Address, Bytes},
	sol,
	sol_types::{SolCall, SolError},
};
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{weights::Weight, BoundedVec};
use indiv_support::{
	labels::{is_lite_person_label, is_person_label},
	traits::{Identifier, MembershipProver, PEOPLE_IDENTIFIER, PEOPLE_LITE_IDENTIFIER},
};
use scale_info::TypeInfo;
use sp_core::{ConstU32, H160};
use sp_runtime::DispatchError;
use verifiable::GenerateVerifiable;

use crate::Config;

/// The proof type from the configured `MemberService`.
pub type ProofOf<T> =
	<<<T as Config>::MemberService as MembershipProver>::Crypto as GenerateVerifiable>::Proof;

/// A dotNS label forwarded via `RootGatewayDispatcher` to `DotnsPopController`.
///
/// Format validation is applied at the call site via
/// [`BaseLabel::is_valid_lite`] (for lite-person `NAME.XX` labels) or
/// [`BaseLabel::is_valid_person`] (for full-person labels, lowercase letters
/// only).
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub struct BaseLabel(pub BoundedVec<u8, ConstU32<32>>);

impl BaseLabel {
	/// Returns whether the label matches the lite-person PoP format
	/// `<dns-stem>.<2+ digits>` (e.g. `alice.42`), per `StringUtils.isLitePersonLabel`
	/// on the contract side.
	pub fn is_valid_lite(&self) -> bool {
		is_lite_person_label(self.0.as_slice())
	}

	/// Returns whether the label is a valid full-person label: lowercase ASCII
	/// letters only, no digits or hyphens.
	pub fn is_valid_person(&self) -> bool {
		is_person_label(self.0.as_slice())
	}

	pub fn as_slice(&self) -> &[u8] {
		self.0.as_slice()
	}

	/// Returns the sole username part of a lite label (bytes preceding the `.`).
	pub fn lite_base(&self) -> &[u8] {
		let bytes = self.as_slice();
		match bytes.iter().position(|b| *b == b'.') {
			Some(idx) => &bytes[..idx],
			None => bytes,
		}
	}
}

impl core::ops::Deref for BaseLabel {
	type Target = [u8];
	fn deref(&self) -> &Self::Target {
		self.as_slice()
	}
}

impl TryFrom<Vec<u8>> for BaseLabel {
	type Error = ();
	fn try_from(v: Vec<u8>) -> Result<Self, Self::Error> {
		BoundedVec::try_from(v).map(Self).map_err(|_| ())
	}
}

/// A 65-byte public key used as an ECDH chat key in the
/// dotNS contract.
#[derive(
	Clone,
	Copy,
	PartialEq,
	Eq,
	Debug,
	Encode,
	Decode,
	MaxEncodedLen,
	TypeInfo,
	DecodeWithMemTracking,
)]
pub struct ChatKey(pub [u8; 65]);

impl ChatKey {
	/// Returns the inner 65-byte array.
	pub fn as_bytes(&self) -> &[u8; 65] {
		&self.0
	}

	/// Returns the inner bytes as a slice.
	pub fn as_slice(&self) -> &[u8] {
		&self.0
	}
}

impl From<[u8; 65]> for ChatKey {
	fn from(v: [u8; 65]) -> Self {
		Self(v)
	}
}

/// A dotNS label an account acquired through this gateway.
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub struct NameEntry {
	/// The label.
	pub label: BaseLabel,
	/// The ECDH chat key the contracts hold for this label. `None` when the
	/// pallet never saw the key: a label backfilled by migration, or a full
	/// label linked to a lite label other than the recorded one.
	pub chat: Option<ChatKey>,
}

/// The dotNS labels an account acquired through this gateway.
///
/// `lite` is the most recently registered lite-person label. `full` is the
/// registered full-person label; the [`crate::AsDotnsGateway`] extension
/// admits at most one full registration per account.
#[derive(
	Clone,
	PartialEq,
	Eq,
	Debug,
	Default,
	Encode,
	Decode,
	MaxEncodedLen,
	TypeInfo,
	DecodeWithMemTracking,
)]
pub struct AccountNameRecord {
	/// The lite-person label (`<dns-stem>.<digits>`), if one was registered.
	pub lite: Option<NameEntry>,
	/// The full-person label, if one was registered.
	pub full: Option<NameEntry>,
}

/// Which ring membership collection the proof targets.
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub enum Collection {
	/// Full personhood collection.
	People,
	/// Lite personhood collection.
	PeopleLite,
}

impl Collection {
	/// Returns the collection identifier bytes.
	pub fn identifier(&self) -> &Identifier {
		match self {
			Collection::People => PEOPLE_IDENTIFIER,
			Collection::PeopleLite => PEOPLE_LITE_IDENTIFIER,
		}
	}
}

/// Record of a name registration by an alias.
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
#[scale_info(skip_type_params(T))]
pub struct RegistrationRecord<T: Config> {
	/// Which collection the person belongs to.
	pub collection: Collection,
	/// The account that initiated the registration.
	pub account: T::AccountId,
}

/// Describes if and how a full-person username relates to a lite-person username.
///
/// Passed to the dotNS contract as a `(uint8, string, bytes)` tuple matching
/// `IDotnsPopController.Link { LinkKind kind; string deviceLabel; bytes chatKey; }`.
#[derive(
	Clone, PartialEq, Eq, Debug, Encode, Decode, MaxEncodedLen, TypeInfo, DecodeWithMemTracking,
)]
pub enum Link {
	/// Links full username to an existing lite-person username.
	/// The chat key is inherited from the lite username's entry.
	LiteUsername(BaseLabel),
	/// Standalone registration with a new uncompressed secp256k1 ECDH chat key.
	None(ChatKey),
}

/// Converts a runtime AccountId to an EVM H160 address.
pub trait AddressMapper<AccountId> {
	/// Converts an account id to an Ethereum address.
	fn to_address(account_id: &AccountId) -> H160;
}

/// A contract call failure, with optional revert data for typed decoding.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ContractCallError {
	/// The dispatch error raised by the underlying contract caller
	/// (e.g. out-of-gas, low-level failure, or a mapped revert).
	pub dispatch: DispatchError,
	/// Raw revert bytes if the failure was a contract revert; `None` for
	/// low-level failures (out-of-gas, caller errors, etc.).
	pub revert_data: Option<Vec<u8>>,
}

impl From<DispatchError> for ContractCallError {
	fn from(dispatch: DispatchError) -> Self {
		Self { dispatch, revert_data: None }
	}
}

/// Abstraction for calling smart contracts via pallet_revive.
pub trait ContractCaller {
	/// Calls a contract at `dest` with the given ABI-encoded `data`.
	///
	/// `value` is the amount of native currency to transfer with the call.
	/// Returns `(raw_return_data, weight_consumed)` on success, allowing the
	/// caller to refund unused weight from the contract call budget.
	fn call(dest: H160, data: Vec<u8>, value: u128)
		-> Result<(Vec<u8>, Weight), ContractCallError>;
}

impl ContractCaller for () {
	fn call(
		_dest: H160,
		_data: Vec<u8>,
		_value: u128,
	) -> Result<(Vec<u8>, Weight), ContractCallError> {
		Ok((Vec::new(), Weight::zero()))
	}
}

// ========== EVM ABI Encoding ==========

sol! {
	struct DeviceNameIssuance {
		string label;
		address user;
		bytes chatKey;
	}

	struct DeviceNameIssuanceWithReservation {
		DeviceNameIssuance issuance;
		string reservedLabel;
	}

	struct SolLink {
		uint8 kind;
		string deviceLabel;
		bytes chatKey;
	}

	struct PersonhoodNameIssuance {
		string label;
		address user;
		SolLink link;
	}

	function issueDeviceName(DeviceNameIssuance params);
	function issueDeviceNameWithReservation(DeviceNameIssuanceWithReservation params);
	function issuePersonhoodName(PersonhoodNameIssuance params);

	error NotRoot();
	error NotGateway(address caller);
	error InvalidDeviceLabel();
	error InvalidPersonhoodLabel();
	error NoActiveReservation(address user);
	error QueueFull(bytes32 labelhash);
	error AlreadyReserved(address user, bytes32 labelhash);
	error NotHolder(address user, bytes32 labelhash);
	error PersonhoodNameUnavailable();
}

impl issueDeviceNameCall {
	/// Encodes `DotnsPopController.issueDeviceName((string,address,bytes))`.
	pub fn new_abi_encoded(lite_label: &[u8], user: H160, chat_key: &[u8]) -> Vec<u8> {
		Self {
			params: DeviceNameIssuance {
				label: string_from_bytes(lite_label),
				user: Address::from(user.0),
				chatKey: Bytes::copy_from_slice(chat_key),
			},
		}
		.abi_encode()
	}
}

impl issueDeviceNameWithReservationCall {
	/// Encodes
	/// `DotnsPopController.issueDeviceNameWithReservation(((string,address,bytes),string))`.
	pub fn new_abi_encoded(
		lite_label: &[u8],
		user: H160,
		chat_key: &[u8],
		reserved_base_label: &[u8],
	) -> Vec<u8> {
		Self {
			params: DeviceNameIssuanceWithReservation {
				issuance: DeviceNameIssuance {
					label: string_from_bytes(lite_label),
					user: Address::from(user.0),
					chatKey: Bytes::copy_from_slice(chat_key),
				},
				reservedLabel: string_from_bytes(reserved_base_label),
			},
		}
		.abi_encode()
	}
}

impl issuePersonhoodNameCall {
	/// Encodes `DotnsPopController.issuePersonhoodName((string,address,(uint8,string,bytes)))`.
	pub fn new_abi_encoded(label: &[u8], user: H160, link: &Link) -> Vec<u8> {
		let sol_link = match link {
			Link::LiteUsername(name) => SolLink {
				kind: 1u8,
				deviceLabel: string_from_bytes(name.as_slice()),
				chatKey: Bytes::copy_from_slice(&[]),
			},
			Link::None(key) => SolLink {
				kind: 0u8,
				deviceLabel: String::new(),
				chatKey: Bytes::copy_from_slice(key.as_slice()),
			},
		};
		Self {
			params: PersonhoodNameIssuance {
				label: string_from_bytes(label),
				user: Address::from(user.0),
				link: sol_link,
			},
		}
		.abi_encode()
	}
}

/// Typed revert observed from a call through `RootGatewayDispatcher`.
///
/// Variants cover both the dispatcher's own `NotRoot` error and the
/// `DotnsPopController` errors.
#[derive(
	Clone,
	Copy,
	PartialEq,
	Eq,
	Debug,
	Encode,
	Decode,
	DecodeWithMemTracking,
	MaxEncodedLen,
	TypeInfo,
	frame_support::PalletError,
)]
pub enum DispatcherRevert {
	/// Dispatcher rejected the call because the substrate origin was not `Root`.
	NotRoot,
	/// Caller is not the registered PoP gateway.
	NotGateway,
	/// Device label did not match `<stem>.<digits>`.
	InvalidDeviceLabel,
	/// Personhood label was not a valid single DNS label.
	InvalidPersonhoodLabel,
	/// Caller has no active reservation to operate on.
	NoActiveReservation,
	/// Reservation queue for the label has reached its capacity.
	QueueFull,
	/// Caller already holds an active reservation for a different label.
	AlreadyReserved,
	/// Caller is not the head-of-queue reservation holder.
	NotHolder,
	/// Personhood name is already registered or held by a whitelist grant.
	PersonhoodNameUnavailable,
}

/// Decodes contract revert `data` into a [`DispatcherRevert`].
///
/// Returns `None` when `data` is shorter than four bytes or its selector
/// does not match any known revert.
pub fn decode_revert(data: &[u8]) -> Option<DispatcherRevert> {
	if data.len() < 4 {
		return None;
	}
	let mut selector = [0u8; 4];
	selector.copy_from_slice(&data[..4]);

	if selector == NotRoot::SELECTOR {
		Some(DispatcherRevert::NotRoot)
	} else if selector == NotGateway::SELECTOR {
		Some(DispatcherRevert::NotGateway)
	} else if selector == InvalidDeviceLabel::SELECTOR {
		Some(DispatcherRevert::InvalidDeviceLabel)
	} else if selector == InvalidPersonhoodLabel::SELECTOR {
		Some(DispatcherRevert::InvalidPersonhoodLabel)
	} else if selector == NoActiveReservation::SELECTOR {
		Some(DispatcherRevert::NoActiveReservation)
	} else if selector == QueueFull::SELECTOR {
		Some(DispatcherRevert::QueueFull)
	} else if selector == AlreadyReserved::SELECTOR {
		Some(DispatcherRevert::AlreadyReserved)
	} else if selector == NotHolder::SELECTOR {
		Some(DispatcherRevert::NotHolder)
	} else if selector == PersonhoodNameUnavailable::SELECTOR {
		Some(DispatcherRevert::PersonhoodNameUnavailable)
	} else {
		None
	}
}

/// Converts a byte slice to an alloy-compatible String for ABI encoding.
fn string_from_bytes(bytes: &[u8]) -> String {
	String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod abi_tests {
	use super::*;

	#[test]
	fn encode_reserve_lite_name_structure() {
		let addr = H160::from_low_u64_be(0x1234);
		let data = issueDeviceNameCall::new_abi_encoded(b"alice42", addr, b"key");

		// Selector present and non-zero.
		assert_ne!(&data[..4], &[0u8; 4]);
		// Encoding is non-trivial (selector + params).
		assert!(data.len() > 4);
	}

	#[test]
	fn encode_reserve_base_name_structure() {
		let addr = H160::from_low_u64_be(0x1234);
		let data =
			issueDeviceNameWithReservationCall::new_abi_encoded(b"alice42", addr, b"key", b"alice");

		assert_ne!(&data[..4], &[0u8; 4]);
		assert!(data.len() > 4);
	}

	#[test]
	fn reserve_lite_and_base_have_distinct_selectors() {
		let addr = H160::from_low_u64_be(0x1234);
		let lite = issueDeviceNameCall::new_abi_encoded(b"alice42", addr, b"key");
		let base =
			issueDeviceNameWithReservationCall::new_abi_encoded(b"alice42", addr, b"key", b"alice");

		// Different functions, different selectors.
		assert_ne!(&lite[..4], &base[..4]);
	}

	#[test]
	fn encode_register_base_name_with_lite_username_link() {
		let addr = H160::from_low_u64_be(0x5678);
		let link = Link::LiteUsername(BaseLabel::try_from(b"alice42".to_vec()).unwrap());
		let data = issuePersonhoodNameCall::new_abi_encoded(b"alice", addr, &link);

		assert_ne!(&data[..4], &[0u8; 4]);
		assert!(data.len() > 4);
	}

	#[test]
	fn encode_register_base_name_with_none_link() {
		let addr = H160::from_low_u64_be(0x5678);
		let link = Link::None(ChatKey::from([0xAB; 65]));
		let data = issuePersonhoodNameCall::new_abi_encoded(b"alice", addr, &link);

		assert_ne!(&data[..4], &[0u8; 4]);
		assert!(data.len() > 4);
	}

	#[test]
	fn register_link_variants_share_selector() {
		let addr = H160::from_low_u64_be(0x5678);
		let link_a = Link::LiteUsername(BaseLabel::try_from(b"alice42".to_vec()).unwrap());
		let link_b = Link::None(ChatKey::from([0x01; 65]));

		let data_a = issuePersonhoodNameCall::new_abi_encoded(b"alice", addr, &link_a);
		let data_b = issuePersonhoodNameCall::new_abi_encoded(b"alice", addr, &link_b);

		// Same selector regardless of link variant.
		assert_eq!(&data_a[..4], &data_b[..4]);
	}

	#[test]
	fn selectors_are_distinct() {
		let link = Link::None(ChatKey::from([0x01; 65]));
		let lite = issueDeviceNameCall::new_abi_encoded(b"x42", H160::zero(), b"");
		let base =
			issueDeviceNameWithReservationCall::new_abi_encoded(b"x42", H160::zero(), b"", b"x");
		let register = issuePersonhoodNameCall::new_abi_encoded(b"x", H160::zero(), &link);

		// All three selectors are pairwise different.
		assert_ne!(&lite[..4], &base[..4]);
		assert_ne!(&lite[..4], &register[..4]);
		assert_ne!(&base[..4], &register[..4]);
	}

	#[test]
	fn reserve_lite_round_trip() {
		let addr = H160::from_low_u64_be(0x1234);
		let data = issueDeviceNameCall::new_abi_encoded(b"alice42", addr, b"key-bytes");

		let decoded = issueDeviceNameCall::abi_decode(&data).expect("decode");
		assert_eq!(decoded.params.label, "alice42");
		assert_eq!(decoded.params.user, Address::from(addr.0));
		assert_eq!(&decoded.params.chatKey[..], b"key-bytes");
	}

	#[test]
	fn reserve_base_round_trip() {
		let addr = H160::from_low_u64_be(0x1234);
		let data =
			issueDeviceNameWithReservationCall::new_abi_encoded(b"alice42", addr, b"key", b"alice");

		let decoded = issueDeviceNameWithReservationCall::abi_decode(&data).expect("decode");
		assert_eq!(decoded.params.issuance.label, "alice42");
		assert_eq!(decoded.params.issuance.user, Address::from(addr.0));
		assert_eq!(&decoded.params.issuance.chatKey[..], b"key");
		assert_eq!(decoded.params.reservedLabel, "alice");
	}

	#[test]
	fn register_round_trip_lite_link() {
		let addr = H160::from_low_u64_be(0x5678);
		let link = Link::LiteUsername(BaseLabel::try_from(b"alice42".to_vec()).unwrap());
		let data = issuePersonhoodNameCall::new_abi_encoded(b"alice", addr, &link);

		let decoded = issuePersonhoodNameCall::abi_decode(&data).expect("decode");
		assert_eq!(decoded.params.label, "alice");
		assert_eq!(decoded.params.user, Address::from(addr.0));
		assert_eq!(decoded.params.link.kind, 1);
		assert_eq!(decoded.params.link.deviceLabel, "alice42");
		assert!(decoded.params.link.chatKey.is_empty());
	}

	#[test]
	fn register_round_trip_none_link() {
		let addr = H160::from_low_u64_be(0x5678);
		let key = [0xABu8; 65];
		let link = Link::None(ChatKey::from(key));
		let data = issuePersonhoodNameCall::new_abi_encoded(b"alice", addr, &link);

		let decoded = issuePersonhoodNameCall::abi_decode(&data).expect("decode");
		assert_eq!(decoded.params.link.kind, 0);
		assert!(decoded.params.link.deviceLabel.is_empty());
		assert_eq!(&decoded.params.link.chatKey[..], &key[..]);
	}

	#[test]
	fn selectors_match_solidity_canonical_signatures() {
		// Mirrors the function signatures in IDotnsPopController.sol.
		fn selector(sig: &str) -> [u8; 4] {
			let h = sp_io::hashing::keccak_256(sig.as_bytes());
			[h[0], h[1], h[2], h[3]]
		}

		assert_eq!(
			issueDeviceNameCall::SELECTOR,
			selector("issueDeviceName((string,address,bytes))")
		);
		assert_eq!(
			issueDeviceNameWithReservationCall::SELECTOR,
			selector("issueDeviceNameWithReservation(((string,address,bytes),string))"),
		);
		assert_eq!(
			issuePersonhoodNameCall::SELECTOR,
			selector("issuePersonhoodName((string,address,(uint8,string,bytes)))"),
		);
	}

	#[test]
	fn decode_revert_matches_known_selectors() {
		let selectors_and_reverts = [
			(NotRoot::SELECTOR, DispatcherRevert::NotRoot),
			(NotGateway::SELECTOR, DispatcherRevert::NotGateway),
			(InvalidDeviceLabel::SELECTOR, DispatcherRevert::InvalidDeviceLabel),
			(InvalidPersonhoodLabel::SELECTOR, DispatcherRevert::InvalidPersonhoodLabel),
			(NoActiveReservation::SELECTOR, DispatcherRevert::NoActiveReservation),
			(QueueFull::SELECTOR, DispatcherRevert::QueueFull),
			(AlreadyReserved::SELECTOR, DispatcherRevert::AlreadyReserved),
			(NotHolder::SELECTOR, DispatcherRevert::NotHolder),
			(PersonhoodNameUnavailable::SELECTOR, DispatcherRevert::PersonhoodNameUnavailable),
		];
		for (selector, expected) in selectors_and_reverts {
			assert_eq!(decode_revert(&selector), Some(expected));
		}
	}

	#[test]
	fn not_root_selector_matches_solidity_canonical_signature() {
		fn selector(sig: &str) -> [u8; 4] {
			let h = sp_io::hashing::keccak_256(sig.as_bytes());
			[h[0], h[1], h[2], h[3]]
		}
		assert_eq!(NotRoot::SELECTOR, selector("NotRoot()"));
	}

	#[test]
	fn decode_revert_rejects_unknown_or_short() {
		assert_eq!(decode_revert(&[]), None);
		assert_eq!(decode_revert(&[0xde, 0xad]), None);
		assert_eq!(decode_revert(&[0xde, 0xad, 0xbe, 0xef]), None);
	}

	#[test]
	fn revert_selectors_are_pairwise_distinct() {
		let selectors = [
			NotRoot::SELECTOR,
			NotGateway::SELECTOR,
			InvalidDeviceLabel::SELECTOR,
			InvalidPersonhoodLabel::SELECTOR,
			NoActiveReservation::SELECTOR,
			QueueFull::SELECTOR,
			AlreadyReserved::SELECTOR,
			NotHolder::SELECTOR,
			PersonhoodNameUnavailable::SELECTOR,
		];
		for i in 0..selectors.len() {
			for j in (i + 1)..selectors.len() {
				assert_ne!(selectors[i], selectors[j]);
			}
		}
	}
}
