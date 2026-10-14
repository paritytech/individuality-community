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

use frame_support::derive_impl;
use indiv_support::traits::{Alias, RingMembershipProof};
use sp_runtime::BuildStorage;
use verifiable::{
	mock::{Mock, MockMembers},
	GenerateVerifiable,
};

pub type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
	pub enum Test
	{
		System: frame_system,
		ProofBatch: crate,
	}
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
	type Block = Block;
}

impl crate::Config for Test {
	type Crypto = Mock;
	type WeightInfo = ();
	#[cfg(feature = "runtime-benchmarks")]
	type BenchmarkHelper = MockBenchmarkHelper;
}

const CONTEXT: &[u8] = b"proof-batch-test";

fn secret(seed: u8) -> [u8; 32] {
	[seed; 32]
}

/// A ring as `(configuration, members)`.
pub type MockRing = (<Mock as GenerateVerifiable>::Config, <Mock as GenerateVerifiable>::Members);

/// A one-member ring, distinct for each `seed`.
pub fn ring(seed: u8) -> MockRing {
	let members = MockMembers(alloc::vec![secret(seed)].try_into().expect("one member fits"));
	((), members)
}

/// A valid proof of the member of `ring(seed)` for `message`, with its alias.
pub fn proof(
	seed: u8,
	message: &[u8],
) -> (RingMembershipProof<<Mock as GenerateVerifiable>::Proof>, Alias) {
	let commitment = Mock::open((), &secret(seed), core::iter::once(secret(seed)))
		.expect("the member is in the ring");
	let (proof, alias) =
		Mock::create(commitment, &secret(seed), CONTEXT, message).expect("the proof is valid");
	(RingMembershipProof { proof, context: CONTEXT.to_vec(), message: message.to_vec() }, alias)
}

/// A proof of `ring(seed)` whose aliases read fine but which fails verification.
pub fn invalid_proof(seed: u8) -> RingMembershipProof<<Mock as GenerateVerifiable>::Proof> {
	let (mut item, _) = proof(seed, b"invalid");
	item.proof.tag = [0; 32];
	item
}

/// A proof of `ring(seed)` with two aliases.
pub fn two_context_proof(seed: u8) -> RingMembershipProof<<Mock as GenerateVerifiable>::Proof> {
	let commitment = Mock::open((), &secret(seed), core::iter::once(secret(seed)))
		.expect("the member is in the ring");
	let (proof, _) =
		Mock::create_multi_context(commitment, &secret(seed), &[CONTEXT, b"other"], b"two")
			.expect("the proof is valid");
	RingMembershipProof { proof, context: CONTEXT.to_vec(), message: b"two".to_vec() }
}

#[cfg(feature = "runtime-benchmarks")]
pub struct MockBenchmarkHelper;

#[cfg(feature = "runtime-benchmarks")]
impl crate::BenchmarkHelper<Mock> for MockBenchmarkHelper {
	fn ring_with_proof(seed: u32) -> crate::BenchmarkRing<Mock> {
		let seed = seed as u8;
		let (config, members) = ring(seed);
		let (proof, _) = proof(seed, b"benchmark");
		crate::BenchmarkRing { config, members, proof }
	}
}

pub fn new_test_ext() -> sp_io::TestExternalities {
	RuntimeGenesisConfig::default().build_storage().unwrap().into()
}
