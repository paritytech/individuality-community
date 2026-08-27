#!/usr/bin/env bash
# Print a markdown list of the pull requests merged between two refs.
#
# Usage: release_pr_notes.sh <previous-ref> <current-ref>
#
# Compares the refs through the GitHub API and prints one line per pull
# request, taken from the `(#N)` suffix of each squash-merge commit subject.
# A line carries the pull request's audit label (`D<n>-`, tracked in issue
# #54) and compatibility label (`C<n>-`, tracked in issue #56) when present.
# The output ends with the overall compatibility of the range: the highest
# `C<n>-` label of any listed pull request.
#
# Requires GH_TOKEN and GITHUB_REPOSITORY in the environment.

set -euo pipefail

PREV="${1:?usage: release_pr_notes.sh <previous-ref> <current-ref>}"
CURR="${2:?usage: release_pr_notes.sh <previous-ref> <current-ref>}"

# The compare endpoint lists at most 250 commits, more than any release here
# spans. grep exits non-zero when no subject carries a pull request number;
# treat that as an empty list.
numbers=$(gh api "repos/$GITHUB_REPOSITORY/compare/$PREV...$CURR" \
	--jq '.commits[].commit.message | split("\n")[0]' \
	| { grep -oE '\(#[0-9]+\)$' || true; } | tr -d '(#)' | sort -un)

if [ -z "$numbers" ]; then
	echo "No pull requests since \`$PREV\`."
	exit 0
fi

c_labels=""
for n in $numbers; do
	# A `(#N)` subject suffix can also point at an issue, e.g. in a
	# hand-written commit; skip numbers that are not pull requests.
	pr=$(gh api "repos/$GITHUB_REPOSITORY/pulls/$n" 2> /dev/null) || continue
	jq -r '
		def tags(re): [.labels[].name | select(test(re))] | map(" `\(.)`") | add // "";
		"- #\(.number) \(.title)" + tags("^D[0-9]+-") + tags("^C[0-9]+-")
	' <<< "$pr"
	c_labels+=$(jq -r '[.labels[].name | select(test("^C[0-9]+-"))] | join("\n")' <<< "$pr")$'\n'
done

overall=$(printf '%s' "$c_labels" | grep -E '^C[0-9]+-' | sort -uV | tail -n1) || true
if [ -n "$overall" ]; then
	printf '\nOverall compatibility: `%s`, the highest label of any pull request above.\n' "$overall"
fi
