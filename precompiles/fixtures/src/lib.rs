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

//! Compiles the Solidity example contracts in `contracts/` to bytecode for end-to-end precompile
//! tests, on demand at test time.
//!
//! `solc` and `resolc` are invoked from `PATH`, as upstream `pallet-revive-fixtures` does. A
//! missing compiler, a compile error or unparseable output panics, so a passing test always
//! exercised real compiled bytecode.

use std::process::Command;

/// The `precompiles/` directory. Fixtures compile from here so their imports of a sibling crate's
/// canonical interface (`../../scarcity/sol/...`) resolve under one base path.
const PRECOMPILES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// A fixture's Solidity source, relative to [`PRECOMPILES_DIR`].
fn fixture_source(name: &str) -> String {
	format!("fixtures/contracts/{name}.sol")
}

/// Compile `name`'s `contracts/{name}.sol` fixture to EVM creation bytecode with `solc`.
///
/// Panics on a missing `solc`, a compile error or unparseable output, so a caller never receives
/// empty or partial bytecode.
pub fn fixture_code(name: &str) -> Vec<u8> {
	compile_solc(name)
}

/// Compile `name`'s fixture to PolkaVM creation bytecode with `resolc`, under the same failure
/// rules as [`fixture_code`].
pub fn resolc_code(name: &str) -> Vec<u8> {
	compile_resolc(name)
}

fn compile_solc(name: &str) -> Vec<u8> {
	let output = Command::new("solc")
		.current_dir(PRECOMPILES_DIR)
		.arg("--bin")
		.arg("--optimize")
		.args(["--evm-version", "cancun"])
		.args(["--base-path", "."])
		.arg(fixture_source(name))
		.output()
		.unwrap_or_else(|error| panic!("could not run solc for {name} ({error}); install solc"));
	assert!(
		output.status.success(),
		"solc failed for {name}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	extract_bytecode(&String::from_utf8_lossy(&output.stdout), name)
}

fn compile_resolc(name: &str) -> Vec<u8> {
	let output = Command::new("resolc")
		.current_dir(PRECOMPILES_DIR)
		.arg("--bin")
		.arg("-O3")
		.args(["--base-path", "."])
		.arg(fixture_source(name))
		.output()
		.unwrap_or_else(|error| {
			panic!("could not run resolc for {name} ({error}); install resolc")
		});
	assert!(
		output.status.success(),
		"resolc failed for {name}: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	extract_bytecode(&String::from_utf8_lossy(&output.stdout), name)
}

/// Read `name`'s creation bytecode out of a `--bin` transcript from `solc` or `resolc`.
///
/// The compiler prints a `======= file:Contract =======` banner for each contract; this finds the
/// one whose name matches and decodes the hex on the line after its `Binary:` marker. It panics
/// when that bytecode is absent: the compiler already exited successfully, so a missing binary
/// means the output format changed or the contract produced none, which is a defect to surface, not
/// an environment problem to skip.
fn extract_bytecode(stdout: &str, name: &str) -> Vec<u8> {
	let banner = format!(":{name} =======");
	let mut lines = stdout.lines();
	while let Some(line) = lines.next() {
		if !line.contains(&banner) {
			continue;
		}
		for inner in lines.by_ref() {
			if inner.starts_with("Binary:") {
				let hex_text =
					lines.next().unwrap_or_else(|| panic!("no bytecode line for {name}"));
				let stripped = hex_text.trim().trim_start_matches("0x");
				return array_bytes::hex2bytes(stripped)
					.unwrap_or_else(|error| panic!("invalid hex for {name}: {error:?}"));
			}
		}
		break;
	}
	panic!("compiler output had no bytecode for {name}")
}
