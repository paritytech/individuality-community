# Release process

Two kinds of releases exist, both built with srtool through the shared
[`build-runtimes.yml`](.github/workflows/build-runtimes.yml) workflow:

- **Nightly release**: an automatic pre-release of the two runtimes, built from `main` every night
  at 02:03 UTC ([`nightly-release.yml`](.github/workflows/nightly-release.yml)). A manual
  `workflow_dispatch` run creates one at any time; any number per day is fine.
- **Release**: a deliberate, non-pre-release version, cut manually
  ([`release.yml`](.github/workflows/release.yml)).

## Spec versions

The two runtimes (`next-people-paseo`, `next-asset-hub-paseo`) share one `spec_version` and move in
lockstep. The format is `M_XXX_YYY`:

- The last 3 digits (`YYY`) are reserved for nightlies and are always zero on `main`. The `Spec
  version` CI check ([`check-spec-version.yml`](.github/workflows/check-spec-version.yml)) enforces
  both rules on every pull request.
- Each nightly reads the spec version of the newest release (of either kind) and adds 1, stamping
  the value into the build workspace only; nothing is pushed back. After a release `3_001_000`,
  nightlies ship `3_001_001`, `3_001_002` and so on. The sequence is exhausted at `999` and fails to
  proceed; cut a release to move to the next thousand.
- Each release states its spec version as a dispatch input and must jump past every released spec
  version, so after nightlies of `3_001_xxx` the next release is at least `3_002_000`.

Later workflow runs read a release's spec version from its attached `*_srtool_output.json` assets,
falling back to the plain `Spec version: <n>` line in the release notes. Do not edit or remove
either.

## Cutting a release

```bash
gh workflow run release.yml -f spec-version=3001000 -f release-version=0.3.1
```

(or use the Actions tab). By convention the release version mirrors the spec version: `3_000_000` is
`0.3.0`, `3_001_000` is `0.3.1`, `4_000_000` is `0.4.0`. The workflow then:

1. Validates the inputs before building: the spec version is a number ending in `000` and above
   every released spec version, the release version looks like `0.3.1`, is above the previous
   release's version and its tag `v<release-version>` is free. Neither number has to increase by
   exactly one; it has to go up.
2. Builds both runtimes with srtool, with the spec version stamped in.
3. Publishes the release under the tag `v<release-version>` with the wasm blobs and srtool digests
   attached. The notes carry the runtime info, the Polkadot SDK version the release is compatible
   with, the previous and current spec versions and the list of pull requests since the previous
   release with their audit (`D<n>-`) and compatibility (`C<n>-`) labels.
4. Commits the released spec version back to `main`. The branch ruleset rejects a direct push, so
   this opens a pull request instead; close and reopen it to trigger CI, then merge it. Granting the
   repository's GitHub Actions a bypass on the `main` ruleset makes this step fully automatic.

There are no release candidates; nightlies fill that role.
