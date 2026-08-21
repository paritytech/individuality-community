#!/usr/bin/env bash
# Fail if a `ValidTransaction` is constructed without an explicit priority decision.
#
# Authorized calls and origin-producing transaction extensions are mostly feeless, so
# without an explicit priority they all land at `0` and the pool cannot order them under
# congestion. Every explicitly-constructed `ValidTransaction` must therefore make a visible
# priority choice (see support/src/tx_priority.rs).
#
# Two construction forms are checked:
#
#   1. Builder:  `ValidTransaction::with_tag_prefix(..) .. (.build()|.into())`
#      Must contain a `.priority(..)` call.
#
#   2. Struct literal:  `= ValidTransaction { .. }`
#      Must set a `priority:` field, OR carry the marker `lint:allow-default-priority`
#      for paths that deliberately keep the default (e.g. fee-paying account paths ordered
#      by fee-based priority, where a non-zero tier would be wrong).
#
# `ValidTransaction::default()` is the deliberate "extension not in use" pass-through that
# contributes no tags or priority, so it is intentionally exempt.
#
# Excluded paths (third-party / generated):
#   runtimes/next-asset-hub-paseo/   — synced from polkadot-fellows/runtimes
#   paseo-support/                   — synced from upstream Substrate
#
# Run locally:
#   .github/scripts/check_validtx_priority.sh

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Tracked Rust files, minus synced third-party trees.
files=$(
	git ls-files '*.rs' \
		| grep -Ev '^(runtimes/next-asset-hub-paseo/|paseo-support/)' \
		|| true
)

if [[ -z "$files" ]]; then
	echo "no files to scan"
	exit 0
fi

offenders=$(
	printf '%s\n' "$files" | xargs perl -0777 -ne '
		# Form 1: builder chain up to its first .build()/.into() terminator.
		while (/ValidTransaction::with_tag_prefix\b.*?\.(?:build|into)\(\)/sg) {
			my ($match, $start) = ($&, $-[0]);
			next if $match =~ /\.priority\s*\(/;
			my $line = (substr($_, 0, $start) =~ tr/\n//) + 1;
			print "$ARGV:$line: builder chain without .priority()\n";
		}
		# Form 2: struct literal assigned with `= ValidTransaction { .. }`. Anchoring on
		# `=` skips `-> ValidTransaction {` return types and the `ValidTransactionBuilder`.
		while (/=\s*ValidTransaction\s*\{.*?\}/sg) {
			my ($match, $start) = ($&, $-[0]);
			next if $match =~ /\bpriority\s*:/;
			# The opt-out marker is also honoured on the comment lines immediately above the
			# literal, so a deliberate default can be documented next to its explanation.
			my @before = split /\n/, substr($_, 0, $start), -1;
			my $from = @before >= 3 ? @before - 3 : 0;
			my $ctx = join("\n", @before[$from .. $#before]) . $match;
			next if $ctx =~ /lint:allow-default-priority/;
			my $line = (substr($_, 0, $start) =~ tr/\n//) + 1;
			print "$ARGV:$line: struct literal without a priority: field\n";
		}
	' || true
)

if [[ -n "$offenders" ]]; then
	echo "Found ValidTransaction constructions without an explicit priority:" >&2
	echo "$offenders" >&2
	echo >&2
	echo "Every explicitly-constructed ValidTransaction must make a visible priority choice" >&2
	echo "with a tier from indiv_support::tx_priority (CLEANUP, USER_DEFAULT," >&2
	echo "BACKGROUND_PROGRESS, USER_HIGH, PROTOCOL_LIVENESS). See support/src/tx_priority.rs." >&2
	echo "A struct literal that deliberately keeps the default (fee-ordered account paths) may" >&2
	echo "instead carry the marker comment 'lint:allow-default-priority' inside the literal." >&2
	exit 1
fi

echo "all ValidTransaction constructions set a priority"
