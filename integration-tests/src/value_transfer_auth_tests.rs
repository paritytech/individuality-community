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

use codec::Encode;
use frame_support::{construct_runtime, derive_impl, parameter_types};
use indiv_pallet_value_transfer_auth::extension::{payload_hash, AuthorizeValueTransfer};
use sp_core::{ed25519, Pair};
use sp_runtime::{
	traits::{DispatchInfoOf, ImplicationParts, TransactionExtension as _},
	transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidityError},
};

type Block = frame_system::mocking::MockBlock<TestRuntime>;

construct_runtime!(
	pub enum TestRuntime {
		System: frame_system,
	}
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for TestRuntime {
	type Block = Block;
}

fn test_keypair() -> (ed25519::Pair, ed25519::Public) {
	let pair = ed25519::Pair::from_seed(&[0x42u8; 32]);
	let public = pair.public();
	(pair, public)
}

parameter_types! {
	pub TestPubkey: ed25519::Public = test_keypair().1;
}

type Ext = AuthorizeValueTransfer<TestRuntime, TestPubkey>;

fn new_test_ext() -> sp_io::TestExternalities {
	use sp_runtime::BuildStorage;
	frame_system::GenesisConfig::<TestRuntime>::default()
		.build_storage()
		.expect("genesis")
		.into()
}

fn implication_for(call: &RuntimeCall) -> ImplicationParts<(u8, &RuntimeCall), (), ()> {
	ImplicationParts { base: (0u8, call), explicit: (), implicit: () }
}

fn signed_extension_for(call: &RuntimeCall) -> Ext {
	let (pair, _) = test_keypair();
	let implication = implication_for(call);
	let payload = payload_hash(&implication);
	AuthorizeValueTransfer(Some(pair.sign(&payload)), core::marker::PhantomData)
}

fn validate_ext(
	extension: &Ext,
	call: &RuntimeCall,
	implication: &impl sp_runtime::traits::Implication,
) -> Result<(sp_runtime::transaction_validity::ValidTransaction, bool), TransactionValidityError> {
	let info = DispatchInfoOf::<RuntimeCall>::default();
	extension
		.validate(
			RuntimeOrigin::none(),
			call,
			&info,
			call.encode().len(),
			(),
			implication,
			TransactionSource::External,
		)
		.map(|(validity, val, _origin)| (validity, val))
}

#[test]
fn signed_tx_returns_val_true() {
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
		let implication = implication_for(&call);
		let extension = signed_extension_for(&call);
		let (_, val) = validate_ext(&extension, &call, &implication).expect("validate succeeds");
		assert!(val);
	});
}

#[test]
fn unsigned_tx_returns_val_false() {
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
		let implication = implication_for(&call);
		let extension = Ext::default();
		let (_, val) = validate_ext(&extension, &call, &implication).expect("validate succeeds");
		assert!(!val);
	});
}

#[test]
fn bad_signature_rejected() {
	new_test_ext().execute_with(|| {
		let call = RuntimeCall::System(frame_system::Call::remark { remark: vec![1, 2, 3] });
		let implication = implication_for(&call);
		let wrong_pair = ed25519::Pair::from_seed(&[0x24u8; 32]);
		let bad_sig = wrong_pair.sign(&payload_hash(&implication));
		let extension: Ext = AuthorizeValueTransfer(Some(bad_sig), core::marker::PhantomData);
		assert_eq!(
			validate_ext(&extension, &call, &implication).map(|(v, _)| v),
			Err(TransactionValidityError::Invalid(InvalidTransaction::BadProof))
		);
	});
}
