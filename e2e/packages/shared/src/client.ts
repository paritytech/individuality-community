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

import { createClient, Enum } from "polkadot-api";
import { getWsProvider } from "polkadot-api/ws";

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Shared connection helpers for the locally-spawned People + Asset Hub network.
 *
 * Endpoints default to the RPC URLs in `scripts/initial-setup/config-local.env`
 * and can still be overridden via env vars.
 * These helpers intentionally use PAPI's unsafe API so the health checks work
 * in a clean checkout without requiring generated descriptor artifacts first.
 */
type UnsafeQueryEntry = {
  getValue: (...args: readonly unknown[]) => Promise<unknown>;
};

type UnsafeChainApi = {
  query: Record<string, Record<string, UnsafeQueryEntry>>;
};

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const CONFIG_LOCAL_ENV = path.resolve(
  SCRIPT_DIR,
  "..",
  "..",
  "..",
  "..",
  "scripts",
  "initial-setup",
  "config-local.env",
);

function readLocalConfigEnv() {
  try {
    const source = readFileSync(CONFIG_LOCAL_ENV, "utf8");
    return Object.fromEntries(
      source
        .split(/\r?\n/u)
        .map(line => line.trim())
        .filter(line => line.length > 0 && !line.startsWith("#"))
        .map((line): [string, string] => {
          const separator = line.indexOf("=");
          const key = line.slice(0, separator).trim();
          const value = line
            .slice(separator + 1)
            .trim()
            .replace(/^"(.*)"$/u, "$1");
          return [key, value];
        }),
    );
  } catch {
    return {};
  }
}

const localConfigEnv = readLocalConfigEnv();

export const PEOPLE_RPC =
  process.env.PEOPLE_RPC ?? process.env.RPC_PEOPLE ?? localConfigEnv.RPC_PEOPLE ?? "ws://localhost:10010";

export const ASSET_HUB_RPC =
  process.env.ASSET_HUB_RPC ??
  process.env.RPC_ASSET_HUB ??
  localConfigEnv.RPC_ASSET_HUB ??
  "ws://localhost:10020";

/**
 * The People runtime has many custom signed extensions (`AsPerson`, `AsCoinage`, …),
 * but they're each `Option<…>` that encode to `None` on their own, so only
 * `VerifyMultiSignature` needs an explicit value. PAPI can't auto-default it
 * because the payload is a non-trivial enum. `Disabled` (variant 0) means a
 * normal single-signature origin.
 *
 * Only pass this on People: Asset Hub's pipeline has no such extension.
 *
 * Pass as `signAndSubmit(signer, { customSignedExtensions })`.
 * (Unused by the read-only health test; kept here for the signing suites.)
 */
export const customSignedExtensions = {
  VerifyMultiSignature: { value: Enum("Disabled") },
};

/** Connect to the People chain and return the API plus the raw client. */
export function connectPeople() {
  const client = createClient(getWsProvider(PEOPLE_RPC));
  return { client, api: client.getUnsafeApi() as UnsafeChainApi };
}

/** Connect to the Asset Hub and return the API plus the raw client. */
export function connectAssetHub() {
  const client = createClient(getWsProvider(ASSET_HUB_RPC));
  return { client, api: client.getUnsafeApi() as UnsafeChainApi };
}
