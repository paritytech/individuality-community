#!/usr/bin/env node

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

import { spawnSync } from "node:child_process";
import { accessSync, constants, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const PKG_DIR = path.resolve(SCRIPT_DIR, "..");
const E2E_DIR = path.resolve(PKG_DIR, "..", "..");
const REPO_ROOT = path.resolve(E2E_DIR, "..");
const DESCRIPTORS_PACKAGE_JSON = path.join(PKG_DIR, ".papi", "descriptors", "package.json");
const WORKSPACE_PKG = JSON.parse(readFileSync(path.join(E2E_DIR, "package.json"), "utf8"));
const POLKADOT_API_RANGE = WORKSPACE_PKG.devDependencies["polkadot-api"];

const PEOPLE_RUNTIME_NAME = process.env.PEOPLE_RUNTIME_NAME ?? "next-people-paseo-runtime";
const AH_RUNTIME_NAME = process.env.AH_RUNTIME_NAME ?? "next-asset-hub-paseo-runtime";
const PEOPLE_WASM = resolveRepoPath(
  process.env.PEOPLE_WASM ??
    `target/release/wbuild/${PEOPLE_RUNTIME_NAME}/next_people_paseo_runtime.compact.compressed.wasm`,
);
const AH_WASM = resolveRepoPath(
  process.env.AH_WASM ??
    `target/release/wbuild/${AH_RUNTIME_NAME}/next_asset_hub_paseo_runtime.compact.compressed.wasm`,
);

function resolveRepoPath(value) {
  return path.isAbsolute(value) ? value : path.join(REPO_ROOT, value);
}

function requireReadable(filePath) {
  try {
    accessSync(filePath, constants.R_OK);
  } catch {
    console.error(`Required runtime WASM is missing or unreadable: ${filePath}`);
    console.error("Build the runtimes first: cd e2e && just build-runtimes");
    process.exit(1);
  }
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: PKG_DIR,
    env: process.env,
    stdio: "inherit",
    ...options,
  });

  if (result.error) {
    console.error(`Failed to run ${command}: ${result.error.message}`);
    process.exit(1);
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function commandSucceeds(command, args) {
  const result = spawnSync(command, args, {
    cwd: PKG_DIR,
    env: process.env,
    stdio: "ignore",
  });
  return result.status === 0;
}

function syncDescriptorPeerDependency() {
  const descriptorPackage = JSON.parse(readFileSync(DESCRIPTORS_PACKAGE_JSON, "utf8"));
  descriptorPackage.peerDependencies ??= {};
  descriptorPackage.peerDependencies["polkadot-api"] = POLKADOT_API_RANGE;
  writeFileSync(DESCRIPTORS_PACKAGE_JSON, `${JSON.stringify(descriptorPackage, null, 2)}\n`);
}

requireReadable(PEOPLE_WASM);
requireReadable(AH_WASM);

if (!commandSucceeds("pnpm", ["exec", "papi", "--version"])) {
  console.error("papi/polkadot-api not found in node_modules.");
  console.error("Run `pnpm install` at the e2e workspace root first, then re-run this.");
  process.exit(1);
}

run("pnpm", [
  "exec",
  "papi",
  "add",
  "people",
  "--config",
  "./polkadot-api.json",
  "--wasm",
  PEOPLE_WASM,
  "--skip-codegen",
]);
run("pnpm", [
  "exec",
  "papi",
  "add",
  "assethub",
  "--config",
  "./polkadot-api.json",
  "--wasm",
  AH_WASM,
  "--skip-codegen",
]);
run("pnpm", ["exec", "papi", "generate", "--config", "./polkadot-api.json"]);
syncDescriptorPeerDependency();

console.log(`Generated @polkadot-api/descriptors (people, assethub) in ${PKG_DIR}/.papi/descriptors`);
