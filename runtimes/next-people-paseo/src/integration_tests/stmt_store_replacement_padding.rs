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

//! Demonstrates how a node that relays a signed statement store slot replacement can pad the
//! transaction.
//!
//! The holder's signature covers the call only. The tip, asset id and nonce are part of the
//! transaction extensions, and `ChargeAssetTxPayment` and `CheckNonce` ignore them for a
//! transaction without a signer. The origin restriction charges the slot by the transaction
//! length, so a padded copy uses up the rate limit of the slot for longer.

use super::*;
use crate::{parameters::AccountsApiAllowance, RestrictedEntity};
use frame_support::storage::{with_transaction, TransactionOutcome};
use indiv_pallet_origin_restriction::{RestrictedEntity as _, Usages};
use indiv_pallet_resources::{
	extension::{AsResources, AsResourcesInfo},
	StatementStoreAllowances,
};
use indiv_support::{traits::Alias, utils::BigEndianU32};
use sp_runtime::{
	generic::Preamble,
	traits::ExtensionVariant,
	transaction_validity::{InvalidTransaction, TransactionSource, TransactionValidity},
	DispatchError,
};
use sp_statement_store::get_allowance;
use xcm::latest::{Junction, NetworkId};

/// Returns the general transaction that moves the statement store slot of `alias` in `period` to
/// `new_account`, with `holder`'s signature over the replacement message for `since`.
fn signed_replacement(
	holder: &sr25519::Pair,
	period: u32,
	alias: Alias,
	new_account: AccountId32,
	since: u64,
) -> UncheckedExtrinsic {
	let call =
		RuntimeCall::Resources(indiv_pallet_resources::Call::replace_statement_store_account {
			period,
			alias,
			new_account: new_account.clone(),
		});
	let msg = Resources::replace_stmt_store_account_message(period, &alias, &new_account, since);
	let signature: MultiSignature = holder.sign(&Resources::account_signing_payload(&msg)).into();
	let mut tx_ext = base_tx_ext(call.clone());
	tx_ext.0 .0 .9 =
		AsResources::new(Some(AsResourcesInfo::ReplaceStatementStoreAccount(signature)));
	finalize_uxt(call, tx_ext)
}

/// Returns the transaction a relaying node submits after it receives `broadcast`: the same call
/// and holder signature, with the tip, asset id and nonce padded to their longest encodings.
fn pad_relayed_replacement(broadcast: &[u8]) -> UncheckedExtrinsic {
	let mut uxt = UncheckedExtrinsic::decode(&mut &broadcast[..]).expect("relayed bytes decode");
	let Preamble::General(ExtensionVariant::Other(pipeline)) = &mut uxt.preamble else {
		panic!("a signed replacement is a general transaction");
	};
	// The longest junction is an account on a forked network: 75 bytes, at most 8 of them.
	let junction = Junction::AccountId32 {
		network: Some(NetworkId::ByFork { block_number: u64::MAX, block_hash: [0xff; 32] }),
		id: [0xff; 32],
	};
	let asset_id = Location::new(0, [junction; 8]);
	pipeline.extension.0 .7 = frame_system::CheckNonce::<Runtime>::from(u32::MAX);
	pipeline.extension.0 .9 =
		pallet_asset_tx_payment::ChargeAssetTxPayment::<Runtime>::from(u128::MAX, Some(asset_id));
	uxt
}

/// Validates `uxt` as the transaction pool does at the current state and discards the state
/// changes of the validation.
fn pool_validity(uxt: UncheckedExtrinsic) -> TransactionValidity {
	with_transaction(|| {
		let validity = Executive::validate_transaction(
			TransactionSource::External,
			uxt,
			frame_system::Pallet::<Runtime>::parent_hash(),
		);
		TransactionOutcome::Rollback(Ok::<_, DispatchError>(validity))
	})
	.expect("the rollback returns the validity")
}

/// A node that relays a signed replacement pads the fields the holder's signature does not cover.
/// The padded copy is about 5 times longer, competes for the same pool tag and charges the slot
/// about 5 times the usage of one replacement. The next replacement of the slot then waits about
/// 74 minutes instead of 15.
#[test]
fn relaying_node_pads_a_signed_replacement_to_delay_the_next_one() {
	new_test_ext().execute_with(|| {
		let alias = [20u8; 32];
		let holder_pair = sr25519::Pair::from_seed(&[100u8; 32]);
		let next_pair = sr25519::Pair::from_seed(&[101u8; 32]);
		let next_holder = pair_to_account_id(&next_pair);
		let final_holder = pair_to_account_id(&sr25519::Pair::from_seed(&[102u8; 32]));
		// A second slot is replaced through an honest relay and serves as the control.
		let control_alias = [21u8; 32];
		let control_pair = sr25519::Pair::from_seed(&[103u8; 32]);
		let control_next_pair = sr25519::Pair::from_seed(&[104u8; 32]);
		let para_block_at = |relay_block: u32| relay_block * BLOCK_PROCESSING_VELOCITY;

		// Both slots are claimed and replaced on the first relay chain block boundary.
		advance_to_block(para_block_at(1));
		let now_secs = pallet_timestamp::Now::<Runtime>::get() / 1000;
		let period = Resources::stmt_store_period_from_timestamp(now_secs);
		let period_key = BigEndianU32::from(period);
		for (slot, pair) in [(alias, &holder_pair), (control_alias, &control_pair)] {
			Resources::set_statement_store_account(
				OriginCaller::Resources(indiv_pallet_resources::Origin::StmtStoreAlias(slot))
					.into(),
				period,
				0,
				pair_to_account_id(pair),
			)
			.unwrap();
		}
		let since =
			|slot| StatementStoreAllowances::<Runtime>::get(period_key, slot).unwrap().since;
		set_time(since(alias) + 1);

		// The holder signs a replacement and broadcasts it.
		let broadcast =
			signed_replacement(&holder_pair, period, alias, next_holder.clone(), since(alias))
				.encode();

		// A relaying node pads the copy it forwards to about 5 times the length.
		let padded = pad_relayed_replacement(&broadcast);
		let padded_len = padded.encoded_size();
		assert!(
			padded_len * 10 >= broadcast.len() * 49, //almost 5 times greater
			"padded {padded_len} bytes against {} broadcast bytes",
			broadcast.len()
		);

		// Both copies are valid, provide the same tag and have the same priority, so the pool keeps
		// whichever copy it receives first.
		let broadcast_validity =
			pool_validity(UncheckedExtrinsic::decode(&mut &broadcast[..]).unwrap())
				.expect("the broadcast replacement is valid");
		let padded_validity = pool_validity(padded.clone()).expect("the padded copy is valid");
		assert_eq!(padded_validity.provides, broadcast_validity.provides);
		assert_eq!(padded_validity.priority, broadcast_validity.priority);

		// The padded copy is included and moves the slot as the holder signed.
		Executive::apply_extrinsic(padded)
			.expect("the padded copy is valid")
			.expect("the padded copy dispatches");
		assert_eq!(get_allowance(&next_holder), AccountsApiAllowance::get());
		let control = signed_replacement(
			&control_pair,
			period,
			control_alias,
			pair_to_account_id(&control_next_pair),
			since(control_alias),
		);
		Executive::apply_extrinsic(control)
			.expect("the control replacement is valid")
			.expect("the control replacement dispatches");

		// The padded copy charged the slot about 5 times the usage of one replacement.
		let usage =
			|slot| Usages::<Runtime>::get(RestrictedEntity::StmtStoreHolder(slot)).unwrap().used;
		let (padded_usage, control_usage) = (usage(alias), usage(control_alias));
		assert!(
			padded_usage * 10 >= control_usage * 49,
			"padded usage {padded_usage} against {control_usage}"
		);

		// After 15 minutes only the control slot can be replaced again.
		let next_replacement = |pair: &sr25519::Pair, slot| {
			signed_replacement(pair, period, slot, final_holder.clone(), since(slot))
		};
		advance_to_block(para_block_at(1 + 150));
		Executive::apply_extrinsic(next_replacement(&control_next_pair, control_alias))
			.expect("the control slot is replaceable after 15 minutes")
			.expect("the control replacement dispatches");
		assert_eq!(
			Executive::apply_extrinsic(next_replacement(&next_pair, alias)),
			Err(InvalidTransaction::Payment.into())
		);

		// The padded slot is replaceable only when the padded usage has recovered.
		let recovery_per_block =
			RestrictedEntity::StmtStoreHolder(alias).allowance().recovery_per_block;
		let recovery_relay_blocks =
			u32::try_from(padded_usage.div_ceil(recovery_per_block)).unwrap();
		assert!(recovery_relay_blocks >= 700, "recovers in {recovery_relay_blocks} relay blocks");
		advance_to_block(para_block_at(1 + recovery_relay_blocks) - 1);
		assert_eq!(
			Executive::apply_extrinsic(next_replacement(&next_pair, alias)),
			Err(InvalidTransaction::Payment.into())
		);
		advance_to_block(para_block_at(1 + recovery_relay_blocks));
		Executive::apply_extrinsic(next_replacement(&next_pair, alias))
			.expect("the padded slot is replaceable once its usage has recovered")
			.expect("the replacement dispatches");
		assert_eq!(
			StatementStoreAllowances::<Runtime>::get(period_key, alias).unwrap().account_id,
			final_holder
		);
	});
}
