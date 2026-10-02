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

import { defineConfig } from "vitest/config";

// The suite imports `packages/shared` by relative path, from outside this directory. `fs.allow`
// opens the whole e2e workspace, which lets vite serve it.
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    // The scenario drives real games on a live chain. One worker, no parallelism.
    fileParallelism: false,
    hookTimeout: 120_000,
  },
  server: {
    fs: { allow: [new URL("../..", import.meta.url).pathname] },
  },
});
