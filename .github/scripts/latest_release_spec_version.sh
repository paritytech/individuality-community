#!/usr/bin/env bash
# Print the spec version of the newest readable release, or nothing.
#
# Usage: latest_release_spec_version.sh
#
# Walks the non-draft releases whose tag starts with `nightly-` or `v`,
# newest first. A release is readable when its attached srtool digests
# (`*_srtool_output.json`) agree on one spec version, or when its
# description carries a `Spec version: <n>` line. The first readable
# release wins. Releases from before this scheme carry neither and are
# skipped.
#
# Requires GH_TOKEN and GITHUB_REPOSITORY in the environment.

set -euo pipefail

tags=$(gh release list --repo "$GITHUB_REPOSITORY" --exclude-drafts --limit 100 \
	--json tagName --jq '.[].tagName | select(startswith("nightly-") or startswith("v"))')

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

for tag in $tags; do
	rm -rf "$workdir/assets"
	mkdir -p "$workdir/assets"
	if gh release download "$tag" --repo "$GITHUB_REPOSITORY" \
		--pattern '*_srtool_output.json' --dir "$workdir/assets" 2> /dev/null; then
		specs=$(jq -r '.runtimes.compressed.subwasm.core_version.specVersion' \
			"$workdir"/assets/*_srtool_output.json | sort -u)
		if [ "$(wc -l <<< "$specs")" -ne 1 ]; then
			echo "::error::release $tag has diverging runtime spec versions: $(tr '\n' ' ' <<< "$specs")" >&2
			exit 1
		fi
		echo "$specs"
		exit 0
	fi
	body=$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" --json body --jq .body)
	spec=$(grep -oE '^Spec version: [0-9]+' <<< "$body" | head -n1 | grep -oE '[0-9]+' || true)
	if [ -n "$spec" ]; then
		echo "$spec"
		exit 0
	fi
done
