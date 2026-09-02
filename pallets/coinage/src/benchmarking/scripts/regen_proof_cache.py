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
       regeneration feature, run `frame-omni-bencher` once per extrinsic and
       per step count, capture the `CACHE_ENTRY:` log lines.
    3. Deduplicate and sort the captured entries.
    4. Splice the entries into `CACHE_ENTRIES_R2E10` in
       `pallets/coinage/src/benchmarking/proof_cache.rs`.

A cache entry is keyed on the component values the proof was built over, so
the harvest runs every step count a later benchmark run may use (2 to 8 by
default) and takes the union. `frame-omni-bencher` is single-threaded, so the
(extrinsic, steps) jobs run in parallel, one process each.

After the script finishes, run the coinage benchmarks and check the log for
`alias proof cache miss` lines.

Example:
    python3 pallets/coinage/src/benchmarking/scripts/regen_proof_cache.py
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[5]
PROOF_CACHE = REPO_ROOT / "pallets" / "coinage" / "src" / "benchmarking" / "proof_cache.rs"

RUNTIME = "next-people-paseo"
PALLET = "indiv_pallet_coinage"
BENCHER = "frame-omni-bencher"
DEFAULT_STEPS = [2, 3, 4, 5, 6, 7, 8]

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


def bencher_cmd(wasm: Path, extrinsic: str) -> list[str]:
    return [
        BENCHER, "v1", "benchmark", "pallet",
        "--runtime", str(wasm),
        "--pallet", PALLET,
        "--extrinsic", extrinsic,
        "--genesis-builder", "runtime",
    ]


def list_extrinsics(wasm: Path) -> list[str]:
    """The extrinsics `frame-omni-bencher` would run for `--extrinsic '*'`.

    `--list` prints a `pallet, extrinsic` CSV with a header row."""
    cmd = bencher_cmd(wasm, "*") + ["--list"]
    log("$ " + " ".join(cmd))
    result = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True, text=True)
    if result.returncode != 0:
        die(f"listing benchmarks failed:\n{result.stderr}", result.returncode)
    names = []
    for line in result.stdout.splitlines():
        parts = [part.strip() for part in line.split(",")]
        if len(parts) == 2 and parts[0] == PALLET:
            names.append(parts[1])
    if not names:
        die(f"`--list` returned no benchmarks for {PALLET}")
    return names


def harvest_one(wasm: Path, extrinsic: str, steps: int) -> tuple[int, set[str], str]:
    """Run one extrinsic at one step count and return (exit status, entries, output)."""
    cmd = bencher_cmd(wasm, extrinsic) + [
        "--steps", str(steps),
        "--repeat", "1",
        "--min-duration", "0",
        "--quiet",
    ]
    # `RUNTIME_LOG=error` lets the `CACHE_ENTRY:` lines through; `off` would hide them.
    env = {**os.environ, "RUNTIME_LOG": "error"}
    proc = subprocess.run(
        cmd, cwd=REPO_ROOT, env=env,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    entries = set()
    for line in proc.stdout.splitlines():
        m = CACHE_ENTRY_RE.search(line)
        if m:
            entries.add(m.group(1))
    return proc.returncode, entries, proc.stdout


def harvest(runtime: str, profile: str, steps_list: list[int], jobs: int) -> set[str]:
    wasm = runtime_wasm_path(runtime, profile)
    if not wasm.exists():
        die(f"WASM not found at {wasm} — did the build succeed?")

    extrinsics = list_extrinsics(wasm)
    tasks = [(extrinsic, steps) for steps in steps_list for extrinsic in extrinsics]
    log(f"{len(extrinsics)} extrinsics x steps {steps_list}: {len(tasks)} runs, {jobs} at a time")

    entries: set[str] = set()
    failures: list[tuple[str, int, int, str]] = []
    done = 0
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = {
            pool.submit(harvest_one, wasm, extrinsic, steps): (extrinsic, steps)
            for extrinsic, steps in tasks
        }
        for future in as_completed(futures):
            extrinsic, steps = futures[future]
            status, found, output = future.result()
            done += 1
            if status != 0:
                failures.append((extrinsic, steps, status, output))
                log(f"[{done}/{len(tasks)}] {extrinsic} --steps {steps}: FAILED ({status})")
            else:
                log(f"[{done}/{len(tasks)}] {extrinsic} --steps {steps}: {len(found)} entries")
            entries |= found

    if failures:
        for extrinsic, steps, status, output in failures:
            tail = "\n".join(output.splitlines()[-15:])
            print(f"\n--- {extrinsic} --steps {steps} exited {status}:\n{tail}", file=sys.stderr)
        die(f"{len(failures)} of {len(tasks)} runs failed; nothing written")
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

    new_body = "\t// Entries are sorted by key for binary_search_by_key.\n"
    new_body += "".join(f"\t{e}\n" for e in entries)
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
    p.add_argument(
        "--steps", type=int, nargs="+", default=DEFAULT_STEPS, metavar="N",
        help="Step counts to harvest; a run at any of them then hits the cache "
             f"(default: {' '.join(map(str, DEFAULT_STEPS))})",
    )
    p.add_argument(
        "--jobs", type=int, default=os.cpu_count() or 1, metavar="N",
        help="Parallel frame-omni-bencher processes (default: CPU count)",
    )
    args = p.parse_args()

    if args.no_build:
        log(f"skipping build for {RUNTIME}; using existing WASM")
    else:
        log(f"building {RUNTIME}-runtime with proof-cache regeneration feature")
        cargo_build(RUNTIME, args.profile)
    log(f"harvesting CACHE_ENTRY lines from {RUNTIME}")
    entries = sorted(harvest(RUNTIME, args.profile, args.steps, args.jobs))
    log(f"{len(entries)} unique entries from {RUNTIME}")

    if args.no_write:
        log("--no-write set; leaving proof_cache.rs untouched")
        return

    splice_into_proof_cache(entries)
    log("done; run the coinage benchmarks and check the log for `alias proof cache miss`")


if __name__ == "__main__":
    main()
