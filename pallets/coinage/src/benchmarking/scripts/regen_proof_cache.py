#!/usr/bin/env python3

# Copyright (C) Parity Technologies (UK) Ltd.
# This file is part of Individuality.
# SPDX-License-Identifier: Apache-2.0

# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""
Regenerate the coinage benchmark proof cache (`CACHE_ENTRIES_R2E10`).

Mirrors steps 2-4 of `pallets/coinage/src/benchmarking/README.md`:

    2. Build the R2e10 runtime (next-people-paseo) with the proof-cache
       regeneration feature, run `frame-omni-bencher`, capture the
       `CACHE_ENTRY:` log lines.
    3. Deduplicate and sort the captured entries.
    4. Splice the entries into `CACHE_ENTRIES_R2E10` in
       `pallets/coinage/src/benchmarking/proof_cache.rs`.

The unload benchmarks sample fixed alias counts, so the proofs a run needs do
not depend on `--steps` or `--repeat`: one harvest at `--steps 2 --repeat 1`
is warm for every run.

After the script finishes, run the coinage benchmarks under `RUNTIME_LOG=warn`
and check that no `alias proof cache miss` line appears.

Example:
    python3 pallets/coinage/src/benchmarking/scripts/regen_proof_cache.py
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[5]
PROOF_CACHE = REPO_ROOT / "pallets" / "coinage" / "src" / "benchmarking" / "proof_cache.rs"

RUNTIME = "next-people-paseo"
PALLET = "indiv_pallet_coinage"
BENCHER = "frame-omni-bencher"

CACHE_ENTRY_RE = re.compile(r"CACHE_ENTRY:\s*(\(.*\),)\s*$")


def log(msg: str) -> None:
    print(f"\033[1;34m==>\033[0m {msg}", flush=True)


def die(msg: str, code: int = 1) -> None:
    print(f"\033[1;31merror:\033[0m {msg}", file=sys.stderr)
    sys.exit(code)


def run(cmd: list[str], env: dict | None = None) -> None:
    log("$ " + " ".join(cmd))
    result = subprocess.run(cmd, cwd=REPO_ROOT, env=env)
    if result.returncode != 0:
        die(f"command exited with status {result.returncode}", result.returncode)


def run_capture(cmd: list[str], env: dict | None = None) -> str:
    """Run a command, streaming combined stdout+stderr to the terminal while
    also returning the captured text. Used for `frame-omni-bencher` so we can
    parse `CACHE_ENTRY:` lines without losing visibility into the run."""
    log("$ " + " ".join(cmd))
    proc = subprocess.Popen(
        cmd, cwd=REPO_ROOT, env=env,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    chunks: list[str] = []
    assert proc.stdout is not None
    for line in proc.stdout:
        sys.stdout.write(line)
        chunks.append(line)
    rc = proc.wait()
    if rc != 0:
        die(f"command exited with status {rc}", rc)
    return "".join(chunks)


def runtime_wasm_path(runtime: str, profile: str) -> Path:
    snake = runtime.replace("-", "_") + "_runtime"
    return (
        REPO_ROOT / "target" / profile / "wbuild"
        / f"{runtime}-runtime" / f"{snake}.compact.compressed.wasm"
    )


def cargo_build(runtime: str, profile: str) -> None:
    # Plain `cargo` uses the stable toolchain pinned in rust-toolchain.toml.
    env = {k: v for k, v in os.environ.items() if k != "SKIP_WASM_BUILD"}
    env["SKIP_PALLET_REVIVE_FIXTURES"] = "1"
    run(
        [
            "cargo", "build",
            "--profile", profile,
            "-p", f"{runtime}-runtime",
            "--features", "runtime-benchmarks,coinage-benchmark-proof-cache-regenerate",
            "--locked",
        ],
        env=env,
    )


def harvest(runtime: str, profile: str) -> set[str]:
    wasm = runtime_wasm_path(runtime, profile)
    if not wasm.exists():
        die(f"WASM not found at {wasm} — did the build succeed?")

    # `RUNTIME_LOG=error` lets the `CACHE_ENTRY:` lines through; `off` would hide them.
    env = {**os.environ, "RUNTIME_LOG": "error"}
    output = run_capture(
        [
            BENCHER, "v1", "benchmark", "pallet",
            "--runtime", str(wasm),
            "--pallet", PALLET,
            "--extrinsic", "*",
            "--steps", "2",
            "--repeat", "1",
            "--min-duration", "0",
            "--genesis-builder", "runtime",
            "--quiet",
        ],
        env=env,
    )

    entries: set[str] = set()
    for line in output.splitlines():
        m = CACHE_ENTRY_RE.search(line)
        if m:
            entries.add(m.group(1))
    return entries


def splice_into_proof_cache(entries: list[str]) -> None:
    """Replace the body of `CACHE_ENTRIES_R2E10` with the given entries.
    Each entry is expected to be the full tuple text including the trailing
    comma, e.g. `(hex!("..."), &hex!("..."), hex!("...")),`."""
    text = PROOF_CACHE.read_text()
    m = re.search(r"pub static CACHE_ENTRIES_R2E10:[^=]*= &\[\n", text)
    if m is None:
        die(f"could not locate `CACHE_ENTRIES_R2E10 = &[` in {PROOF_CACHE}")
    body_start = m.end()
    body_end = text.find("];", body_start)
    if body_end == -1:
        die("could not locate closing `];` of CACHE_ENTRIES_R2E10")

    new_body = "".join(f"\t{e}\n" for e in entries)
    new_text = text[:body_start] + new_body + text[body_end:]
    PROOF_CACHE.write_text(new_text)
    log(
        f"wrote {len(entries)} entries into CACHE_ENTRIES_R2E10 "
        f"({PROOF_CACHE.relative_to(REPO_ROOT)})"
    )


def main() -> None:
    p = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument(
        "--no-build", action="store_true",
        help="Skip the cargo build and use the existing WASM",
    )
    p.add_argument(
        "--no-write", action="store_true",
        help="Print the entry count but do not modify proof_cache.rs",
    )
    p.add_argument(
        "--profile", default="production",
        help="Cargo profile to build the runtime with. The cached proofs do not depend on it, "
             "so `release` trades a slower harvest for a much shorter build (default: production)",
    )
    args = p.parse_args()

    if args.no_build:
        log(f"skipping build for {RUNTIME}; using existing WASM")
    else:
        log(f"building {RUNTIME}-runtime with proof-cache regeneration feature")
        cargo_build(RUNTIME, args.profile)
    log(f"harvesting CACHE_ENTRY lines from {RUNTIME}")
    entries = sorted(harvest(RUNTIME, args.profile))
    log(f"{len(entries)} unique entries from {RUNTIME}")

    if args.no_write:
        log("--no-write set; leaving proof_cache.rs untouched")
        return

    splice_into_proof_cache(entries)
    log("done; run the coinage benchmarks under RUNTIME_LOG=warn and check for `alias proof cache miss`")


if __name__ == "__main__":
    main()
