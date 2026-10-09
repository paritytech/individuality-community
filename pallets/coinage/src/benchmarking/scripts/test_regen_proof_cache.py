#!/usr/bin/env python3

import importlib.util
import unittest
from unittest.mock import patch
from pathlib import Path


SCRIPT = Path(__file__).with_name("regen_proof_cache.py")
SPEC = importlib.util.spec_from_file_location("regen_proof_cache", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
regen_proof_cache = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(regen_proof_cache)


class RuntimeWasmPathTest(unittest.TestCase):
    def test_profile_directory_and_artifact_match_wasm_builder_outputs(self):
        runtime = "next-people-paseo"
        expected = {
            "dev": "target/debug/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.wasm",
            "test": "target/debug/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.wasm",
            "bench": "target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm",
            "release": "target/release/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm",
            "production": "target/production/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm",
            "testnet": "target/testnet/wbuild/next-people-paseo-runtime/next_people_paseo_runtime.compact.compressed.wasm",
        }
        for profile, path in expected.items():
            self.assertEqual(regen_proof_cache.runtime_wasm_path(runtime, profile), regen_proof_cache.REPO_ROOT / path)


class HarvestTest(unittest.TestCase):
    @patch.object(Path, "exists", return_value=True)
    @patch.object(regen_proof_cache, "run_capture", return_value="benchmark completed without cache entries")
    def test_missing_regeneration_output_fails_before_replacing_cache(self, run_capture, exists):
        with self.assertRaises(SystemExit):
            regen_proof_cache.harvest("next-people-paseo", "production")

    @patch.object(Path, "exists", return_value=True)
    @patch.object(regen_proof_cache, "run_capture")
    def test_harvest_deduplicates_entries_and_keeps_full_sample_coverage(self, run_capture, exists):
        entry = '(hex!("01"), &hex!("02"), hex!("03")),'
        run_capture.return_value = f"CACHE_ENTRY: {entry}\nCACHE_ENTRY: {entry}\n"
        self.assertEqual(regen_proof_cache.harvest("next-people-paseo", "production"), {entry})
        command = run_capture.call_args.args[0]
        self.assertIn("--all", command)
        self.assertNotIn("--extra", command)
        self.assertEqual(command[command.index("--exclude-pallets") + 1],
                         "pallet_xcm_benchmarks::fungible,pallet_xcm_benchmarks::generic,pallet_xcm")
        self.assertEqual(command[command.index("--steps") + 1], "2")
        self.assertEqual(command[command.index("--repeat") + 1], "1")
        self.assertEqual(run_capture.call_args.kwargs["env"]["RUNTIME_LOG"], "error")


if __name__ == "__main__":
    unittest.main()
