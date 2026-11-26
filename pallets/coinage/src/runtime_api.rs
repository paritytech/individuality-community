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

//! Runtime API definition for Coinage fee quotes.

use crate::InstanceId;
use codec::Codec;

sp_api::decl_runtime_apis! {
    #[api_version(1)]
    pub trait CoinageApi<NativeBalance, AssetBalance>
    where
    	NativeBalance: Codec,
    	AssetBalance: Codec,
    {
    	/// The current price of one paid unload token, in the native currency.
    	///
    	/// Varies with chain state through `WeightToFee`.
    	fn paid_unload_token_fee_in_native() -> NativeBalance;

    	/// The current price of one paid unload token, in the instance's underlying asset.
    	///
    	/// `None` if the instance does not exist, or if its asset cannot currently be
    	/// converted into the native currency, in which case only native payment is
    	/// available.
    	fn paid_unload_token_fee_in_asset(instance_id: InstanceId) -> Option<AssetBalance>;

    	/// The current price of `count` paid unload tokens, in the instance's underlying
    	/// asset.
    	///
    	/// Quoted as a single swap, so pool slippage makes this differ from `count` times
    	/// the single-token quote.
    	fn paid_unload_token_fee_quote_in_asset(
    		instance_id: InstanceId,
    		count: u32,
    	) -> Option<AssetBalance>;
    }
}
