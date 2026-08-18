# Definition of Done

A change is done, and a PR is mergeable, only when **all** gates below pass. Each gate is a runnable command, run from the repo root. These mirror the CI in `.github/workflows`, so green locally means green in CI.

Formatting is auto-applied: a `PostToolUse` hook in `.claude/settings.json` runs `scripts/format.sh` after every Rust/TOML edit. The remaining gates are run manually before marking a change done; CI is the final authority.

## Gates

Run the local runner for fast feedback before pushing:

- **`scripts/check.sh`** — rustfmt (pinned nightly), taplo, `zepter run check`, clippy (`--all-features -D warnings`), and `cargo nextest` (`--all-features`).
- **`.github/scripts/check_todos.sh`** and **`.github/scripts/check_validtx_priority.sh`** — repo lint checks (fast, no build).

`check.sh` trades coverage for speed: it sets `SKIP_WASM_BUILD=1`, runs tests in debug (not `--release`), and only the `--all-features` variant. A green run is necessary but not sufficient.

**CI is the authority for the full gate set** and the change is done only when it is green. CI additionally runs default-feature clippy, rustdoc (`-D warnings`), the release test suite, `cargo deny`, `psvm`, and the WASM build the local steps skip. The workflows in `.github/workflows` are the source of truth.

## Weights

- Every new or changed weight function has a matching benchmark, and `weights.rs` is regenerated and committed (see [Extrinsics](extrinsics.md)). A weight signature change with stale weights is not done.

## Quality (required, not command-checked)

- Conventions in [Style](style.md), [Extrinsics](extrinsics.md), and [Testing](testing.md) are applied.
- New behaviour and every new branch are covered by tests, including negative cases. No tautological tests: a test that stays green when you invert the logic it claims to check is dead weight, so delete it rather than keep the count.
- No leftover debug logging, commented-out code, or committed build artifacts.
- The PR follows the template (`Closes #`, context, summary, `## Changes`) and the description matches the code.
