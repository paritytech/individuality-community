// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Individuality.
// SPDX-License-Identifier: Apache-2.0
//
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

import { Binary, Enum, type SizedHex } from "polkadot-api";

/**
 * The full-people collection identifier as a 32-byte hex value: the text
 * `pop:polkadot.network/people` (27 chars) right-padded with 5 spaces to 32.
 */
export const PEOPLE_IDENTIFIER = Binary.toHex(
  Binary.fromText("pop:polkadot.network/people     "),
) as SizedHex<32>;

/** Ring exponent for the people collection (`MembersFlexibleRingExponent = R2e9`). */
export const PEOPLE_RING_EXPONENT = Enum("R2e9");

/** People para id in this local network (= `PEOPLE_ID` / `NEXT_PEOPLE_ID`). */
export const PEOPLE_PARA_ID = 1502;

/** Asset Hub para id in this local network (= `ASSET_HUB_ID` / `NEXT_ASSET_HUB_ID`). */
export const ASSET_HUB_PARA_ID = 1500;

/**
 * Index of `MembersSubscriber` in the Asset Hub runtime's construct_runtime
 * (`MembersSubscriber: indiv_pallet_members_subscriber = 97`).
 */
export const SUBSCRIBER_PALLET_INDEX = 97;
