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

//! Submission of, and pacing constants for, the authorized calls an offchain worker submits every
//! block.
//!
//! A submitter carries a `discriminator` of `block_number / RETRY_WINDOW`. Attempts inside one
//! window encode identically, so the transaction pool deduplicates them. A new window changes the
//! transaction hash, which escapes that deduplication and the pool rotator's ban on a hash it has
//! already seen in a block.
//!
//! Every pallet that submits this way goes through [`submit_authorized`], so a stalled submitter
//! behaves the same whichever pallet it belongs to.

use frame_system::{
	offchain::{CreateAuthorizedTransaction, SubmitTransaction},
	pallet_prelude::BlockNumberFor,
};
use sp_runtime::traits::Zero;

/// Retry window, in blocks, for an offchain worker's submissions.
/// The `discriminator` a submission carries stays constant over one window.
pub const RETRY_WINDOW: u32 = 8;

/// Longevity, in blocks, of an offchain worker's submission.
/// A stranded retry leaves the pool instead of waiting to be mined against state it no longer
/// matches.
pub const TX_LONGEVITY: u64 = 64;

/// Period, in blocks, at which a failed submission logs a warning instead of a `debug` message.
///
/// A submission fails inside every retry window, so only a streak of failures shows a stall.
/// Nothing else reports a stalled submitter.
pub const STALL_WARN_PERIOD: u32 = 32;

/// Hands the authorized `call` of pallet `T` to the transaction pool, reporting a stall under
/// `log_target` as `name`.
///
/// A submission fails inside every retry window, because each attempt there encodes identically and
/// the pool deduplicates them. Only a streak of failures shows a stall, so the failure logs a
/// warning every [`STALL_WARN_PERIOD`] blocks and a `debug` message otherwise.
pub fn submit_authorized<T, Call>(
	call: Call,
	block_number: BlockNumberFor<T>,
	name: &str,
	log_target: &str,
) where
	T: frame_system::Config + CreateAuthorizedTransaction<Call>,
{
	let tx = T::create_authorized_transaction(call.into());
	if SubmitTransaction::<T, Call>::submit_transaction(tx).is_ok() {
		return;
	}

	if (block_number % STALL_WARN_PERIOD.into()).is_zero() {
		log::warn!(
			target: log_target,
			"offchain worker: `{name}` repeatedly rejected by the transaction pool, possible stall",
		);
	} else {
		log::debug!(target: log_target, "offchain worker: failed to submit `{name}`");
	}
}
