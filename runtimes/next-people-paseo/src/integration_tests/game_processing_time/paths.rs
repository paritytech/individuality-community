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

//! The sign-up and report transactions, and what `CheckWeight` books for each.

use super::{tables::describe, *};
use frame_support::dispatch::GetDispatchInfo;
use indiv_pallet_game::{FullReport, Report, WeightInfo as _, MAX_GAME_AIRDROPS};
use indiv_pallet_people::extension::{AsPerson, AsPersonInfo};
use indiv_support::crypto::BandersnatchSuite;
use verifiable::ring::ring_signature_size;

/// The airdrop entry counts of the airdrop sweep.
const AIRDROP_COUNTS: [u8; 5] = [0, 1, 2, 4, MAX_GAME_AIRDROPS];

/// The airdrop entries of a sign-up.
type Airdrops = AirdropVrfs<indiv_pallet_airdrop::ProofOf<Runtime>>;

/// A way to submit a sign-up or a report, with what `CheckWeight` books for one transaction.
pub(super) struct TxPath {
	pub(super) name: &'static str,
	/// The weight a block books after the refund, with the encoded length as proof size.
	pub(super) weight: Weight,
	/// The weight before the refund, with the encoded length as proof size.
	/// It must fit one extrinsic.
	pub(super) declared: Weight,
	/// The encoded length, which counts toward the `Normal` block length.
	pub(super) length: u64,
}

impl TxPath {
	/// The path of `uxt`, which pays its declared call weight.
	fn new(name: &'static str, limits: &Limits, uxt: &UncheckedExtrinsic) -> Self {
		Self::refunded(name, limits, uxt, uxt.function.get_dispatch_info().call_weight)
	}

	/// The path of `uxt`, whose call weight is refunded to `call_weight` after dispatch.
	fn refunded(
		name: &'static str,
		limits: &Limits,
		uxt: &UncheckedExtrinsic,
		call_weight: Weight,
	) -> Self {
		let length = uxt.encoded_size() as u64;
		let extension_weight = uxt.extension_weight();
		let declared = uxt
			.function
			.get_dispatch_info()
			.call_weight
			.saturating_add(extension_weight)
			.saturating_add_proof_size(length);
		let weight = limits
			.base_extrinsic
			.saturating_add(extension_weight)
			.saturating_add(call_weight)
			.saturating_add_proof_size(length);
		Self { name, weight, declared, length }
	}

	pub(super) fn describe(&self) -> String {
		format!("{} {}B", describe(self.weight), self.length)
	}
}

/// The sign-up and report paths the estimate compares.
pub(super) struct Paths {
	pub(super) sign_ups: SignUps,
	pub(super) reports: Reports,
	/// The sign-ups by the number of airdrop entries they carry, one row per count in
	/// `AIRDROP_COUNTS`.
	pub(super) airdrop_sweep: Vec<AirdropSweepRow>,
}

/// The sign-up paths, split by whether the transaction carries a ring proof.
pub(super) struct SignUps {
	pub(super) without_proof: SignUpsWithoutProof,
	pub(super) with_proof: SignUpsWithProof,
}

/// The sign-ups authenticated by a signature, entering airdrops by VRF if at all.
pub(super) struct SignUpsWithoutProof {
	pub(super) account: TxPath,
	pub(super) account_with_vrf_airdrops: TxPath,
	pub(super) alias_account: TxPath,
	pub(super) invite: TxPath,
	pub(super) invite_with_vrf_airdrops: TxPath,
	pub(super) lite_alias_account: TxPath,
	pub(super) lite_alias_account_with_vrf_airdrops: TxPath,
}

/// The sign-ups carrying a ring proof, in their authentication or in their airdrop entries.
pub(super) struct SignUpsWithProof {
	pub(super) account_with_proof_airdrops: TxPath,
	pub(super) alias_account_with_proof_airdrops: TxPath,
	pub(super) ring_proof: TxPath,
	pub(super) lite_alias_account_revised: TxPath,
}

/// The report paths.
pub(super) struct Reports {
	pub(super) account: TxPath,
	pub(super) alias_account: TxPath,
	pub(super) ring_proof: TxPath,
}

/// The sign-ups carrying `airdrops` entries. A player who is not recognized enters an airdrop by
/// VRF, a recognized one by ring proof.
pub(super) struct AirdropSweepRow {
	pub(super) airdrops: u8,
	/// A new player signing up with its account.
	pub(super) account_by_vrf: TxPath,
	/// An invited player. `GameAsInvited` validates the entries as well as the call.
	pub(super) invite_by_vrf: TxPath,
	/// A recognized player signing up with its account.
	pub(super) account_by_proof: TxPath,
	/// A recognized player signing up with its alias account.
	pub(super) alias_account_by_proof: TxPath,
	/// A recognized player signing up with a ring proof.
	pub(super) ring_proof_by_proof: TxPath,
}

impl Paths {
	/// Builds the transaction of each path. The transaction extensions read the externalities.
	pub(super) fn build(limits: &Limits, rounds: u32, group_size: u32) -> Self {
		let player = Sr25519Keyring::Alice.pair();
		// A full report covers every co-player of every round.
		let full_report: FullReport<Runtime> = (0..rounds)
			.map(|_| {
				(1..group_size)
					.map(|_| Report::Person)
					.collect::<Vec<_>>()
					.try_into()
					.expect("a round reports fewer players than the group size")
			})
			.collect::<Vec<_>>()
			.try_into()
			.expect("a report has one entry per round");
		let report = RuntimeCall::Game(indiv_pallet_game::Call::report { full_report });
		let refunded_report = GameWeights::report(rounds * (group_size - 1), 1);

		let without_proof = SignUpsWithoutProof {
			account: TxPath::new(
				"account, no airdrop",
				limits,
				&build_signed_ext(&player, sign_up_with_account(None)),
			),
			account_with_vrf_airdrops: TxPath::new(
				"account, max airdrops by VRF",
				limits,
				&build_signed_ext(
					&player,
					sign_up_with_account(vrf_airdrops(&player, MAX_GAME_AIRDROPS)),
				),
			),
			alias_account: TxPath::new(
				"alias account, no airdrop",
				limits,
				&build_signed_as_alias_with_account_ext(&player, sign_up_with_alias(&player, None)),
			),
			invite: TxPath::new(
				"invite, no airdrop",
				limits,
				&by_invite(&player, sign_up_with_invite(None)),
			),
			invite_with_vrf_airdrops: TxPath::new(
				"invite, max airdrops by VRF",
				limits,
				&by_invite(&player, sign_up_with_invite(vrf_airdrops(&player, MAX_GAME_AIRDROPS))),
			),
			lite_alias_account: TxPath::new(
				"lite alias account, no airdrop",
				limits,
				&build_people_lite_auth_ext(
					&player,
					indiv_pallet_people_lite::PeopleLiteAuthData::AsLiteAliasWithAccount,
					sign_up_with_account_lite_invite(None),
				),
			),
			lite_alias_account_with_vrf_airdrops: TxPath::new(
				"lite alias account, max airdrops by VRF",
				limits,
				&build_people_lite_auth_ext(
					&player,
					indiv_pallet_people_lite::PeopleLiteAuthData::AsLiteAliasWithAccount,
					sign_up_with_account_lite_invite(vrf_airdrops(&player, MAX_GAME_AIRDROPS)),
				),
			),
		};
		let with_proof = SignUpsWithProof {
			account_with_proof_airdrops: TxPath::new(
				"account, max airdrops by alias proof",
				limits,
				&build_signed_ext(&player, sign_up_with_account(proof_airdrops(MAX_GAME_AIRDROPS))),
			),
			alias_account_with_proof_airdrops: TxPath::new(
				"alias account, max airdrops by alias proof",
				limits,
				&build_signed_as_alias_with_account_ext(
					&player,
					sign_up_with_alias(&player, proof_airdrops(MAX_GAME_AIRDROPS)),
				),
			),
			ring_proof: TxPath::new(
				"alias ring proof, no airdrop",
				limits,
				&by_ring_proof(sign_up_with_alias(&player, None)),
			),
			lite_alias_account_revised: TxPath::new(
				"lite alias account revised, no airdrop",
				limits,
				&build_people_lite_auth_ext(
					&player,
					|nonce| {
						indiv_pallet_people_lite::PeopleLiteAuthData::AsLiteAliasWithAccountRevised(
							nonce,
							ring_proof(),
							0,
							0,
							[0; 32],
						)
					},
					sign_up_with_account_lite_invite(None),
				),
			),
		};
		let reports = Reports {
			account: TxPath::refunded(
				"account",
				limits,
				&build_signed_ext(&player, report.clone()),
				refunded_report,
			),
			alias_account: TxPath::refunded(
				"alias account",
				limits,
				&build_signed_as_alias_with_account_ext(&player, report.clone()),
				refunded_report,
			),
			ring_proof: TxPath::refunded(
				"alias ring proof",
				limits,
				&by_ring_proof(report),
				refunded_report,
			),
		};
		let airdrop_sweep = AIRDROP_COUNTS
			.into_iter()
			.map(|airdrops| AirdropSweepRow::build(limits, &player, airdrops))
			.collect();

		Self { sign_ups: SignUps { without_proof, with_proof }, reports, airdrop_sweep }
	}
}

impl SignUps {
	/// Every sign-up path, those without a ring proof first.
	pub(super) fn all(&self) -> impl Iterator<Item = &TxPath> + Clone {
		self.without_proof.all().into_iter().chain(self.with_proof.all())
	}
}

impl SignUpsWithoutProof {
	fn all(&self) -> [&TxPath; 7] {
		[
			&self.account,
			&self.account_with_vrf_airdrops,
			&self.alias_account,
			&self.invite,
			&self.invite_with_vrf_airdrops,
			&self.lite_alias_account,
			&self.lite_alias_account_with_vrf_airdrops,
		]
	}
}

impl SignUpsWithProof {
	fn all(&self) -> [&TxPath; 4] {
		[
			&self.account_with_proof_airdrops,
			&self.alias_account_with_proof_airdrops,
			&self.ring_proof,
			&self.lite_alias_account_revised,
		]
	}
}

impl Reports {
	pub(super) fn all(&self) -> [&TxPath; 3] {
		[&self.account, &self.alias_account, &self.ring_proof]
	}
}

impl AirdropSweepRow {
	fn build(limits: &Limits, player: &sr25519::Pair, airdrops: u8) -> Self {
		let by_vrf = || vrf_airdrops(player, airdrops);
		let by_proof = || proof_airdrops(airdrops);
		Self {
			airdrops,
			account_by_vrf: TxPath::new(
				"account by VRF",
				limits,
				&build_signed_ext(player, sign_up_with_account(by_vrf())),
			),
			invite_by_vrf: TxPath::new(
				"invite by VRF",
				limits,
				&by_invite(player, sign_up_with_invite(by_vrf())),
			),
			account_by_proof: TxPath::new(
				"account by proof",
				limits,
				&build_signed_ext(player, sign_up_with_account(by_proof())),
			),
			alias_account_by_proof: TxPath::new(
				"alias account by proof",
				limits,
				&build_signed_as_alias_with_account_ext(
					player,
					sign_up_with_alias(player, by_proof()),
				),
			),
			ring_proof_by_proof: TxPath::new(
				"ring proof by proof",
				limits,
				&by_ring_proof(sign_up_with_alias(player, by_proof())),
			),
		}
	}

	pub(super) fn all(&self) -> [&TxPath; 5] {
		[
			&self.account_by_vrf,
			&self.invite_by_vrf,
			&self.account_by_proof,
			&self.alias_account_by_proof,
			&self.ring_proof_by_proof,
		]
	}
}

/// `count` airdrop entries by VRF, as a player who is not recognized enters them.
fn vrf_airdrops(player: &sr25519::Pair, count: u8) -> Option<Airdrops> {
	(count > 0).then(|| {
		let event_ids = (0..count).map(|index| airdrop_event_id_for(0, index)).collect::<Vec<_>>();
		build_account_airdrop_vrfs(player, &event_ids)
	})
}

/// `count` airdrop entries by ring proof, as a recognized player enters them.
fn proof_airdrops(count: u8) -> Option<Airdrops> {
	(count > 0).then(|| AirdropVrfs::Alias {
		proofs: (0..count)
			.map(|_| ring_proof())
			.collect::<Vec<_>>()
			.try_into()
			.expect("the proofs are bounded by MAX_GAME_AIRDROPS"),
		ring_index: 0,
		revision: 0,
	})
}

fn sign_up_with_account(airdrops: Option<Airdrops>) -> RuntimeCall {
	RuntimeCall::Game(indiv_pallet_game::Call::sign_up_with_account {
		identifier_key: [0; 65],
		airdrops,
	})
}

fn sign_up_with_alias(player: &sr25519::Pair, airdrops: Option<Airdrops>) -> RuntimeCall {
	RuntimeCall::Game(indiv_pallet_game::Call::sign_up_with_alias {
		identifier_key: [0; 65],
		statement_account: Sr25519Keyring::Alice.to_account_id(),
		sig: MultiSignature::from(player.sign(&[])),
		airdrops,
	})
}

/// An invite signs up a new player, whose airdrop entries are VRFs.
fn sign_up_with_invite(airdrops: Option<Airdrops>) -> RuntimeCall {
	RuntimeCall::Game(indiv_pallet_game::Call::sign_up_with_invite {
		identifier_key: [0; 65],
		airdrops,
	})
}

/// A lite invite also signs up a new player. `AsLiteAliasWithProof` only dispatches
/// `set_alias_account`, so a lite alias signs up through its account.
fn sign_up_with_account_lite_invite(airdrops: Option<Airdrops>) -> RuntimeCall {
	RuntimeCall::Game(indiv_pallet_game::Call::sign_up_with_account_lite_invite {
		account: Sr25519Keyring::Alice.to_account_id(),
		identifier_key: [0; 65],
		airdrops,
	})
}

/// `call` signed by `player` through the `GameAsInvited` extension.
fn by_invite(player: &sr25519::Pair, call: RuntimeCall) -> UncheckedExtrinsic {
	build_signed_game_invited_ext(player, &sr25519::Pair::from_seed(&[98; 32]), call)
}

/// `call` from a person alias authenticated by a ring proof, which no signature accompanies.
fn by_ring_proof(call: RuntimeCall) -> UncheckedExtrinsic {
	let mut tx_ext = base_tx_ext(call.clone());
	tx_ext.0 .0 .2 =
		AsPerson::new(Some(AsPersonInfo::AsPersonalAliasWithProof(ring_proof(), 0, 0, [0; 32])));
	finalize_uxt(call, tx_ext)
}

/// A ring proof of one context. Only its length matters to the estimate.
fn ring_proof<P: TryFrom<Vec<u8>>>() -> P {
	P::try_from(vec![0; ring_signature_size::<BandersnatchSuite>(1)])
		.unwrap_or_else(|_| panic!("a ring proof of one context fits the proof bound"))
}
