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

import unittest
from unittest.mock import patch, mock_open, MagicMock, call
import json
import sys
import os
import argparse

# Mock data for runtimes-matrix.json
mock_runtimes_matrix = [
    {
        "name": "next-people-paseo",
        "package": "next-people-paseo-runtime",
        "path": "runtimes/next-people-paseo",
        "header": ".github/scripts/cmd/file_header.txt",
        "bench_features": "runtime-benchmarks",
        "bench_flags": ""
    },
    {
        "name": "next-asset-hub-paseo",
        "package": "next-asset-hub-paseo-runtime",
        "path": "runtimes/next-asset-hub-paseo",
        "header": ".github/scripts/cmd/file_header.txt",
        "bench_features": "runtime-benchmarks",
        "bench_flags": ""
    },
    {
        "name": "pallet-weights",
        "package": "next-people-paseo-runtime",
        "path": "pallets",
        "header": ".github/scripts/cmd/file_header.txt",
        "template": "templates/frame-weight-template.hbs",
        "bench_features": "runtime-benchmarks",
        "bench_flags": "",
        "is_pallet_weights": True
    },
    {
        "name": "asset-hub-pallet-weights",
        "package": "next-asset-hub-paseo-runtime",
        "path": "pallets",
        "header": ".github/scripts/cmd/file_header.txt",
        "template": "templates/frame-weight-template.hbs",
        "bench_features": "runtime-benchmarks",
        "bench_flags": "",
        "is_pallet_weights": True
    }
]


def get_mock_bench_output(package, pallet, output_path, header, bench_flags='', template=None):
    return (
        f"RUNTIME_LOG=off frame-omni-bencher v1 benchmark pallet "
        f"--extrinsic=* "
        f"--runtime=target/production/wbuild/{package}/{package.replace('-', '_')}.wasm "
        f"--pallet={pallet} "
        f"--header={header} "
        f"--output={output_path} "
        f"--wasm-execution=compiled "
        f"--steps=50 "
        f"--repeat=20 "
        f"--min-duration=1 "
        f"--heap-pages=4096 "
        f"--genesis-builder=runtime "
        f"--no-storage-info --no-min-squares --no-median-slopes "
        f"{f'--template={template} ' if template else ''}"
        f"{bench_flags}"
    )


class TestCmd(unittest.TestCase):

    def setUp(self):
        self.patcher1 = patch('builtins.open', new_callable=mock_open, read_data=json.dumps(mock_runtimes_matrix))
        self.patcher2 = patch('json.load', return_value=mock_runtimes_matrix)
        self.patcher3 = patch('argparse.ArgumentParser.parse_known_args')
        self.patcher4 = patch('os.system', return_value=0)
        self.patcher5 = patch('os.popen')
        self.patcher6 = patch('subprocess.run')

        self.mock_open = self.patcher1.start()
        self.mock_json_load = self.patcher2.start()
        self.mock_parse_args = self.patcher3.start()
        self.mock_system = self.patcher4.start()
        self.mock_popen = self.patcher5.start()
        self.mock_subprocess_run = self.patcher6.start()

        # Make subprocess.run return success for builds
        self.mock_subprocess_run.return_value = MagicMock(returncode=0, stdout="pallet_balances,extrinsic\n", stderr="")

        # Ensure that cmd.py uses the mock_runtimes_matrix
        import cmd
        cmd.runtimesMatrix = mock_runtimes_matrix

    def tearDown(self):
        self.patcher1.stop()
        self.patcher2.stop()
        self.patcher3.stop()
        self.patcher4.stop()
        self.patcher5.stop()
        self.patcher6.stop()

    def test_bench_command_single_runtime_single_pallet(self):
        """Test bench for a single pallet on a single runtime"""
        self.mock_parse_args.return_value = (argparse.Namespace(
            command='bench',
            runtime=['next-people-paseo'],
            pallet=['pallet_balances'],
            continue_on_fail=False,
            quiet=False,
            clean=False,
            steps=50,
            repeat=20,
            dry_run=False
        ), [])

        # Mock subprocess.run for build (success) and list pallets
        self.mock_subprocess_run.side_effect = [
            MagicMock(returncode=0),  # Build
            MagicMock(returncode=0, stdout="pallet_balances\nframe_system\n", stderr=""),  # List pallets
        ]

        header_path = os.path.abspath(mock_runtimes_matrix[0]['header'])

        with patch('sys.exit') as mock_exit:
            import cmd
            cmd.main()
            mock_exit.assert_not_called()

            expected_call = call(get_mock_bench_output(
                package='next-people-paseo-runtime',
                pallet='pallet_balances',
                output_path='./runtimes/next-people-paseo/src/weights',
                header=header_path
            ))
            self.mock_system.assert_has_calls([expected_call], any_order=True)

    def test_bench_command_pallet_weights_mode(self):
        """Test bench for pallet-weights mode (writes to pallets/*/src/weights.rs)"""
        self.mock_parse_args.return_value = (argparse.Namespace(
            command='bench',
            runtime=['pallet-weights'],
            pallet=['indiv_pallet_coinage'],
            continue_on_fail=False,
            quiet=False,
            clean=False,
            steps=50,
            repeat=20,
            dry_run=False
        ), [])

        # Mock subprocess.run for build and list pallets
        self.mock_subprocess_run.side_effect = [
            MagicMock(returncode=0),  # Build (next-people-paseo-runtime)
            MagicMock(returncode=0, stdout="indiv_pallet_coinage\nframe_system\n", stderr=""),  # List pallets
        ]

        # Mock cargo metadata for manifest path lookup
        self.mock_popen.return_value.read.return_value = "/workspace/pallets/coinage/Cargo.toml\n"

        header_path = os.path.abspath(mock_runtimes_matrix[0]['header'])

        with patch('sys.exit') as mock_exit:
            import cmd
            cmd.main()
            mock_exit.assert_not_called()

            expected_call = call(get_mock_bench_output(
                package='next-people-paseo-runtime',
                pallet='indiv_pallet_coinage',
                output_path='/workspace/pallets/coinage/src/weights.rs',
                header=header_path,
                template='templates/frame-weight-template.hbs'
            ))
            self.mock_system.assert_has_calls([expected_call], any_order=True)

    def test_bench_command_pallet_without_runtime(self):
        """Test that --pallet without --runtime runs on all runtimes (including pallet-weights)"""
        self.mock_parse_args.return_value = (argparse.Namespace(
            command='bench',
            runtime=list(map(lambda x: x['name'], mock_runtimes_matrix)),
            pallet=['indiv_pallet_coinage'],
            continue_on_fail=False,
            quiet=False,
            clean=False,
            steps=50,
            repeat=20,
            dry_run=False
        ), [])

        # Mock subprocess.run for builds and list pallets
        self.mock_subprocess_run.side_effect = [
            MagicMock(returncode=0),  # Build next-people-paseo-runtime
            MagicMock(returncode=0, stdout="indiv_pallet_coinage\nframe_system\n", stderr=""),  # List for next-people-paseo
            MagicMock(returncode=0),  # Build next-asset-hub-paseo-runtime
            MagicMock(returncode=0, stdout="indiv_pallet_coinage\nframe_system\n", stderr=""),  # List for next-asset-hub-paseo
            # pallet-weights reuses next-people-paseo-runtime build (skipped)
            MagicMock(returncode=0, stdout="indiv_pallet_coinage\nframe_system\n", stderr=""),  # List for pallet-weights
            # asset-hub-pallet-weights reuses next-asset-hub-paseo-runtime build (skipped)
            MagicMock(returncode=0, stdout="indiv_pallet_coinage\nframe_system\n", stderr=""),  # List for asset-hub-pallet-weights
        ]

        # Mock cargo metadata for pallet-weights mode
        self.mock_popen.return_value.read.return_value = "/workspace/pallets/coinage/Cargo.toml\n"

        header_path = os.path.abspath(mock_runtimes_matrix[0]['header'])

        with patch('sys.exit') as mock_exit:
            import cmd
            cmd.main()
            mock_exit.assert_not_called()

            # Should have 4 benchmark calls: next-people-paseo, next-asset-hub-paseo,
            # pallet-weights, asset-hub-pallet-weights
            bench_calls = [c for c in self.mock_system.call_args_list if 'frame-omni-bencher' in str(c)]
            self.assertEqual(len(bench_calls), 4, f"Expected 4 benchmark calls, got {len(bench_calls)}")

    def test_bench_command_xcm_pallets_excluded(self):
        """Test that XCM pallets are excluded"""
        self.mock_parse_args.return_value = (argparse.Namespace(
            command='bench',
            runtime=['next-people-paseo'],
            pallet=[],
            continue_on_fail=False,
            quiet=False,
            clean=False,
            steps=50,
            repeat=20,
            dry_run=False
        ), [])

        # Mock list pallets returning XCM pallets
        self.mock_subprocess_run.side_effect = [
            MagicMock(returncode=0),  # Build
            MagicMock(returncode=0, stdout="pallet_balances\npallet_xcm_benchmarks::fungible\npallet_xcm_benchmarks::generic\npallet_xcm\n", stderr=""),
        ]

        with patch('sys.exit') as mock_exit:
            import cmd
            cmd.main()

            # XCM pallets should be filtered out, only pallet_balances benchmarked
            bench_calls = [c for c in self.mock_system.call_args_list if 'frame-omni-bencher' in str(c)]
            self.assertEqual(len(bench_calls), 1, f"Expected 1 benchmark call (xcm excluded), got {len(bench_calls)}")
            self.assertIn('pallet_balances', str(bench_calls[0]))
            self.assertNotIn('pallet_xcm', str(bench_calls[0]))

    def test_bench_pallet_weights_skips_external_deps(self):
        """Test that pallet-weights mode silently skips external deps (pallet_balances etc.)"""
        self.mock_parse_args.return_value = (argparse.Namespace(
            command='bench',
            runtime=['pallet-weights'],
            pallet=[],
            continue_on_fail=False,
            quiet=False,
            clean=False,
            steps=50,
            repeat=20,
            dry_run=False
        ), [])

        # Mock build and list pallets — includes both workspace and external pallets
        self.mock_subprocess_run.side_effect = [
            MagicMock(returncode=0),  # Build
            MagicMock(returncode=0, stdout="indiv_pallet_coinage\npallet_balances\nframe_system\n", stderr=""),
        ]

        # Mock cargo metadata: returns path for indiv_pallet_coinage, empty for externals
        def mock_popen_side_effect(cmd):
            mock_result = MagicMock()
            if 'indiv-pallet-coinage' in cmd:
                mock_result.read.return_value = "/workspace/pallets/coinage/Cargo.toml\n"
            else:
                mock_result.read.return_value = "\n"  # Not found — external dep
            return mock_result

        self.mock_popen.side_effect = mock_popen_side_effect

        with patch('sys.exit') as mock_exit:
            import cmd
            cmd.main()
            # Should NOT exit — external pallets are silently skipped
            mock_exit.assert_not_called()

            # Only indiv_pallet_coinage should have been benchmarked
            bench_calls = [c for c in self.mock_system.call_args_list if 'frame-omni-bencher' in str(c)]
            self.assertEqual(len(bench_calls), 1, f"Expected 1 benchmark call, got {len(bench_calls)}")
            self.assertIn('indiv_pallet_coinage', str(bench_calls[0]))

    @patch('argparse.ArgumentParser.parse_known_args',
           return_value=(argparse.Namespace(command='fmt', continue_on_fail=False, quiet=False, clean=False), []))
    @patch('os.system', return_value=0)
    @patch('os.getenv', return_value='2026-04-26')
    def test_fmt_command(self, mock_getenv, mock_system, mock_parse_args):
        with patch('sys.exit') as mock_exit:
            import cmd
            cmd.main()
            mock_exit.assert_not_called()
            mock_system.assert_any_call('cargo +nightly-2026-04-26 fmt')
            mock_system.assert_any_call('taplo format --config .config/taplo.toml')


if __name__ == '__main__':
    unittest.main()
