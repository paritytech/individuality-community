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

import os
import sys
import json
import argparse
import subprocess
import _help

_HelpAction = _help._HelpAction

f = open('.github/workflows/runtimes-matrix.json', 'r')
runtimesMatrix = json.load(f)

runtimeNames = list(map(lambda x: x['name'], runtimesMatrix))

common_args = {
    '--continue-on-fail': {"action": "store_true", "help": "Won't exit(1) on failed command and continue with next "
                                                           "steps. Helpful when you want to push at least successful "
                                                           "pallets, and then run failed ones separately"},
    '--quiet': {"action": "store_true", "help": "Won't print start/end/failed messages in Pull Request"},
    '--clean': {"action": "store_true", "help": "Clean up the previous bot's & author's comments in Pull Request "
                                                "which triggered /cmd"},
}

parser = argparse.ArgumentParser(prog="/cmd ", description='A command runner for individuality repo', add_help=False)
parser.add_argument('--help', action=_HelpAction, help='help for help if you need some help')  # help for help

subparsers = parser.add_subparsers(help='a command to run', dest='command')

"""
BENCH
"""

bench_example = '''**Examples**:

 > runs all benchmarks (runtime weights + pallet weights)

 %(prog)s

 > runs benchmarks for indiv_pallet_coinage everywhere (pallet + both runtimes)
 > --quiet makes it to output nothing to PR but reactions

 %(prog)s --pallet indiv_pallet_coinage --quiet

 > runs bench for all pallets for next-people-paseo runtime only

 %(prog)s --runtime next-people-paseo

 > runs bench for pallet-level weights only (pallets/*/src/weights.rs)

 %(prog)s --runtime pallet-weights

 > runs bench for all pallets for next-people-paseo runtime and continues even if some benchmarks fail

 %(prog)s --runtime next-people-paseo --continue-on-fail

 > quick test run: fewer steps/repeats, posts diff as comment instead of committing

 %(prog)s --runtime next-people-paseo --pallet indiv_pallet_coinage --dry-run

 > runs a benchmark with custom steps/repeats

 %(prog)s --runtime next-people-paseo --pallet indiv_pallet_coinage --steps 10 --repeat 4

 > does not output anything and cleans up the previous bot's & author command triggering comments in PR

 %(prog)s --runtime next-people-paseo next-asset-hub-paseo --pallet pallet_balances --quiet --clean

 '''

parser_bench = subparsers.add_parser('bench', help='Runs benchmarks', epilog=bench_example, formatter_class=argparse.RawDescriptionHelpFormatter)

for arg, config in common_args.items():
    parser_bench.add_argument(arg, **config)

parser_bench.add_argument('--runtime', help='Runtime(s) space separated', choices=runtimeNames, nargs='*', default=runtimeNames)
parser_bench.add_argument('--pallet', help='Pallet(s) space separated', nargs='*', default=[])
parser_bench.add_argument('--steps', help='Number of steps (default: 50)', type=int, default=50)
parser_bench.add_argument('--repeat', help='Number of repeats (default: 20)', type=int, default=20)
parser_bench.add_argument('--dry-run', action='store_true', help='Quick test run: uses --steps 10 --repeat 4, skips commit, posts diff as PR comment')

"""
FMT
"""
parser_fmt = subparsers.add_parser('fmt', help='Formats code')
for arg, config in common_args.items():
    parser_fmt.add_argument(arg, **config)


def main():
    global runtimesMatrix

    args, unknown = parser.parse_known_args()

    print(f'args: {args}')

    if args.command == 'bench':
        runtime_pallets_map = {}
        failed_benchmarks = {}
        successful_benchmarks = {}

        # Dry-run: override steps/repeat for a quick test.
        if args.dry_run:
            args.steps = 10
            args.repeat = 4
            min_duration = 0
            print('🏃 Dry-run mode: using --steps 10 --repeat 4 --min-duration 0, changes will NOT be committed')
        else:
            min_duration = 1

        profile = "production"

        # Excluded pallets (XCM benchmarks excluded for now - phase 2)
        excluded_pallets = {
            'pallet_xcm_benchmarks::fungible',
            'pallet_xcm_benchmarks::generic',
            'pallet_xcm',
        }

        print(f'Provided runtimes: {args.runtime}')
        # convert to mapped dict
        runtimesMatrix = list(filter(lambda x: x['name'] in args.runtime, runtimesMatrix))
        runtimesMatrix = {x['name']: x for x in runtimesMatrix}
        print(f'Filtered out runtimes: {runtimesMatrix}')

        # Track which packages we've already built to avoid rebuilding
        built_packages = set()

        # loop over remaining runtimes to collect available pallets
        for runtime in runtimesMatrix.values():
            package = runtime['package']

            # Only build if we haven't built this package already
            if package not in built_packages:
                print(f'-- compiling the runtime {runtime["name"]} (package: {package})')
                features = runtime.get("bench_features", "runtime-benchmarks")
                print(f'-- with features {features}')
                result = subprocess.run(
                    ["cargo", "build", "-p", package, "--profile", profile, "-q", "--features", features])
                if result.returncode != 0:
                    print(f"Failed to build {runtime['name']}")
                    sys.exit(1)
                built_packages.add(package)
            else:
                print(f'-- skipping build for {runtime["name"]} (already built {package})')

            print(f'-- listing pallets for benchmark for {runtime["name"]}')
            wasm_file = f"target/{profile}/wbuild/{package}/{package.replace('-', '_')}.wasm"
            result = subprocess.run(
                ["frame-omni-bencher", "v1", "benchmark", "pallet", "--no-csv-header", "--all", "--list", f"--runtime={wasm_file}"],
                capture_output=True, text=True)
            if result.returncode != 0:
                print(f"Failed to list pallets for {runtime['name']}: {result.stderr}")
                sys.exit(1)
            raw_pallets = result.stdout.split('\n')

            all_pallets = set()
            for pallet in raw_pallets:
                if pallet:
                    all_pallets.add(pallet.split(',')[0].strip())

            # Remove excluded pallets
            all_pallets -= excluded_pallets

            pallets = list(all_pallets)
            print(f'Pallets in {runtime["name"]}: {pallets}')
            runtime_pallets_map[runtime['name']] = pallets

        # filter out only the specified pallets from collected runtimes/pallets
        if args.pallet:
            print(f'Pallet: {args.pallet}')
            new_pallets_map = {}
            # keep only specified pallets if they exist in the runtime
            for runtime in runtime_pallets_map:
                matching = [p for p in args.pallet if p in runtime_pallets_map[runtime]]
                if matching:
                    new_pallets_map[runtime] = matching

            runtime_pallets_map = new_pallets_map

        print(f'Filtered out runtimes & pallets: {runtime_pallets_map}')

        if not runtime_pallets_map:
            if args.pallet and not args.runtime:
                print(f"No pallets [{args.pallet}] found in any runtime")
            elif args.runtime and not args.pallet:
                print(f"{args.runtime} runtime does not have any pallets")
            elif args.runtime and args.pallet:
                print(f"No pallets [{args.pallet}] found in {args.runtime}")
            else:
                print('No runtimes found')
            sys.exit(0)

        for runtime in runtime_pallets_map:
            for pallet in runtime_pallets_map[runtime]:
                config = runtimesMatrix[runtime]
                is_pallet_weights = config.get('is_pallet_weights', False)
                header_path = os.path.abspath(config['header'])
                template = None

                print(f'-- config: {config}')

                if is_pallet_weights:
                    # Pallet-weights mode: find the pallet's source directory via cargo metadata.
                    # Only workspace pallets will be found — external deps (pallet_balances, etc.)
                    # are silently skipped since we can't write to their source.
                    search_manifest_path = f"cargo metadata --locked --format-version 1 --no-deps | jq -r '.packages[] | select(.name == \"{pallet.replace('_', '-')}\") | .manifest_path'"
                    print(f'-- running: {search_manifest_path}')
                    manifest_path = os.popen(search_manifest_path).read().strip()
                    if not manifest_path:
                        print(f'-- pallet {pallet} is not a workspace member, skipping pallet-weights')
                        continue
                    package_dir = os.path.dirname(manifest_path)
                    print(f'-- package_dir: {package_dir}')
                    output_path = os.path.join(package_dir, "src", "weights.rs")
                    template = config.get('template')
                    print(f'-- template: {template}')
                else:
                    # Runtime-weights mode: output to runtime weights directory
                    default_path = f"./{config['path']}/src/weights"
                    xcm_path = f"./{config['path']}/src/weights/xcm"
                    output_path = default_path if not pallet.startswith("pallet_xcm_benchmarks") else xcm_path

                print(f'-- benchmarking {pallet} in {runtime} into {output_path}')

                cmd = (f"RUNTIME_LOG=off frame-omni-bencher v1 benchmark pallet "
                       f"--extrinsic=* "
                       f"--runtime=target/{profile}/wbuild/{config['package']}/{config['package'].replace('-', '_')}.wasm "
                       f"--pallet={pallet} "
                       f"--header={header_path} "
                       f"--output={output_path} "
                       f"--wasm-execution=compiled "
                       f"--steps={args.steps} "
                       f"--repeat={args.repeat} "
                       f"--min-duration={min_duration} "
                       f"--heap-pages=4096 "
                       f"--genesis-builder=runtime "
                       f"--no-storage-info --no-min-squares --no-median-slopes "
                       f"{f'--template={template} ' if template else ''}"
                       f"{config.get('bench_flags', '')}"
                       )

                print(f'-- Running: {cmd}')
                status = os.system(cmd)

                if status != 0 and not args.continue_on_fail:
                    print(f'Failed to benchmark {pallet} in {runtime}')
                    sys.exit(1)

                if status != 0:
                    failed_benchmarks[runtime] = failed_benchmarks.get(runtime, []) + [pallet]
                else:
                    successful_benchmarks[runtime] = successful_benchmarks.get(runtime, []) + [pallet]

        if failed_benchmarks:
            print('❌ Failed benchmarks of runtimes/pallets:')
            for runtime, pallets in failed_benchmarks.items():
                print(f'-- {runtime}: {pallets}')

        if successful_benchmarks:
            print('✅ Successful benchmarks of runtimes/pallets:')
            for runtime, pallets in successful_benchmarks.items():
                print(f'-- {runtime}: {pallets}')

    elif args.command == 'fmt':
        nightly_version = os.getenv('RUST_NIGHTLY_VERSION')
        command = f"cargo +nightly-{nightly_version} fmt" if nightly_version else "cargo +nightly fmt"
        print(f'Formatting with `{command}`')
        nightly_status = os.system(f'{command}')

        command = "taplo format --config .config/taplo.toml"
        print(f'Formatting toml files with `{command}`')
        taplo_status = os.system(command)

        if nightly_status != 0 or taplo_status != 0:
            print('❌ Failed to format code')
            if not args.continue_on_fail:
                sys.exit(1)

    print('🚀 Done')


if __name__ == '__main__':
    main()
