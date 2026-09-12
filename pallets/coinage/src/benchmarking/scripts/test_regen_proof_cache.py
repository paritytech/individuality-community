#!/usr/bin/env python3

import importlib.util
import unittest
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


if __name__ == "__main__":
    unittest.main()
