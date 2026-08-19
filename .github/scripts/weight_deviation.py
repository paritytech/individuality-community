#!/usr/bin/env python3
"""Compare generated runtime weights between a base ref and a current ref/tree.

The script evaluates each generated weight function at its documented
max-component point and reports large changes in ``ref_time`` or
``proof_size``. An extrinsic is flagged when either metric deviates by at
least the threshold; the headline names the metric(s) that crossed it and
rows over the threshold are marked in the table. Each file section also
compares the bench machine headers (CPU, hostname, date, sampling) of the
base and current files, since ref_time deltas can reflect hardware rather
than code when the machines differ.

"Worst-case" here means the generated fit evaluated with every named component
(e.g. ``n``, ``r``, ``p``) pinned to the high end of that function's documented
benchmark range. A zero-component weight is evaluated as its constant value.

What this is
------------
A diff tool for generated per-pallet runtime weight files
(``runtimes/*/src/weights/*.rs``). By default it scans only files changed
against the base; pass ``--all`` to scan every runtime weight file. Helper files
such as ``mod.rs`` and DB weight files are skipped.

For each function present on both sides, the script parses ``fn name(..) ->
Weight { .. }``, reads component ranges from the preceding ``/// The range of
component `x` is `[lo, hi]`.`` doc lines, evaluates base and current at their
respective high-component points, and reports deviations above the configured
threshold.

Limits
------
This is a delta tool, not a fit-quality checker. It only compares the generated
formulas. If both formulas miss the same real cost shape, for example a cross
term in code that is modeled as ``a + b*p + c*r``, their evaluated values can
still look close.

Useful workflow
---------------
The script is most valuable when the comparison straddles a benchmark refactor
such as a component added or removed, a range expanded, or corrected sampling.
If the underlying code did not change, a large delta at the worst-case point
after re-benching is a strong signal that the old fit had been silently cheap.

Usage
-----
    .github/scripts/weight_deviation.py                       # base=merge-base(origin/main, HEAD),
                                                              # current=working tree
    .github/scripts/weight_deviation.py --base main
    .github/scripts/weight_deviation.py --current HEAD        # compare two committed refs
    .github/scripts/weight_deviation.py --all                 # every weight file, not just changed ones
    .github/scripts/weight_deviation.py --pallet indiv_pallet_game
    .github/scripts/weight_deviation.py --max p=15            # override a component's max (e.g. the
                                                              # charge-site value, not the bench range)

Notes
-----
* The benchmark only *samples* each component up to its documented range, but a
  call site may *charge* the weight at a larger value (e.g. ``report`` is charged
  at ``max_enactments = MaxRounds*(MaxGroupSize-1)`` while ``p`` is sampled in
  ``[0, MaxGroupSize-1]``). Use ``--max name=value`` to override that component
  wherever it appears and evaluate at the real charge-site maximum.
* ref_time includes the DB portion (``reads``/``writes`` x RocksDbWeight). proof
  size is taken straight from the ``from_parts(_, proof)`` terms (DB access adds
  no proof in ``RuntimeDbWeight``).
"""

from __future__ import annotations

import argparse
import glob
import re
import subprocess
import sys
from dataclasses import dataclass, field

# RocksDbWeight for next-people-paseo: read = 25_000 ns, write = 100_000 ns,
# and WEIGHT_REF_TIME_PER_NANOS = 1_000 ps. DB access contributes no proof size.
DEFAULT_DB_READ_PS = 25_000 * 1_000
DEFAULT_DB_WRITE_PS = 100_000 * 1_000

WEIGHT_PATHS = [
    "runtimes/*/src/weights/*.rs",
]

# Files under the weights dirs that are not per-pallet extrinsic weights.
SKIP_BASENAMES = {
    "mod.rs",
    "block_weights.rs",
    "extrinsic_weights.rs",
    "rocksdb_weights.rs",
    "paritydb_weights.rs",
}

_INT = r"\d[\d_]*"


def _num(s: str) -> int:
    return int(s.replace("_", ""))


@dataclass
class WeightFn:
    name: str
    components: list[str] = field(default_factory=list)
    comp_max: dict[str, int] = field(default_factory=dict)
    ref_const: int = 0
    proof_const: int = 0
    ref_slope: dict[str, int] = field(default_factory=dict)
    proof_slope: dict[str, int] = field(default_factory=dict)
    reads_const: int = 0
    writes_const: int = 0
    reads_slope: dict[str, int] = field(default_factory=dict)
    writes_slope: dict[str, int] = field(default_factory=dict)
    storage: dict[str, tuple[int, int]] = field(default_factory=dict)  # item -> (reads, writes)
    missing_ranges: list[str] = field(default_factory=list)

    def evaluate(self, db_read: int, db_write: int, overrides: dict[str, int]):
        """Return (ref_time, proof_size, reads, writes) at max component values."""

        def maxv(c: str) -> int:
            return overrides.get(c, self.comp_max.get(c, 0))

        reads = self.reads_const + sum(self.reads_slope.get(c, 0) * maxv(c) for c in self.components)
        writes = self.writes_const + sum(self.writes_slope.get(c, 0) * maxv(c) for c in self.components)
        ref = self.ref_const + sum(self.ref_slope.get(c, 0) * maxv(c) for c in self.components)
        ref += reads * db_read + writes * db_write
        proof = self.proof_const + sum(self.proof_slope.get(c, 0) * maxv(c) for c in self.components)
        return ref, proof, reads, writes


# --- parsing -----------------------------------------------------------------

_FN_HEADER = re.compile(r"\bfn\s+(\w+)\s*\(([^)]*)\)\s*->\s*Weight\s*\{")
_RANGE = re.compile(r"range of component `(\w+)` is `\[\s*(\d+)\s*,\s*(\d+)\s*\]`")
_STORAGE = re.compile(r"Storage: `([^`]+)` \(r:(\d+) w:(\d+)\)")
_FROM_PARTS = re.compile(
    r"from_parts\(\s*(" + _INT + r")\s*,\s*(" + _INT + r")\s*\)"
    r"(?:\s*\.saturating_mul\(\s*(\w+)\.into\(\)\s*\))?"
)
_ARG = re.compile(r"(\w+)\s*:\s*u32")
_ENV_HOST = re.compile(r"//! HOSTNAME: `([^`]*)`, CPU: `([^`]*)`")
_ENV_DATE = re.compile(r"//! DATE: ([0-9-]+)")
_ENV_SAMPLING = re.compile(r"STEPS: `(\d+)`, REPEAT: `(\d+)`")


def parse_bench_env(text: str | None) -> dict[str, str | None] | None:
    """Extracts the bench machine header of a generated weights file."""
    if not text:
        return None
    host = _ENV_HOST.search(text)
    date = _ENV_DATE.search(text)
    sampling = _ENV_SAMPLING.search(text)
    if host is None and date is None:
        return None
    return {
        "cpu": host.group(2) if host else None,
        "hostname": host.group(1) if host else None,
        "date": date.group(1) if date else None,
        "sampling": f"STEPS: {sampling.group(1)}, REPEAT: {sampling.group(2)}" if sampling else None,
    }


def _db_terms(verb: str, body: str):
    """Yield (count, component_or_None) for `.reads(..)` / `.writes(..)`."""
    mul = re.compile(
        r"\." + verb + r"\(\(\s*(" + _INT + r")_u64\s*\)\.saturating_mul\(\s*(\w+)\.into\(\)\s*\)\)"
    )
    plain = re.compile(r"\." + verb + r"\((" + _INT + r")(?:_u64)?\)")
    for m in mul.finditer(body):
        yield _num(m.group(1)), m.group(2)
    for m in plain.finditer(body):
        # Skip the multiply form, whose inner `(` means `plain` won't match it.
        yield _num(m.group(1)), None


def _split_functions(text: str):
    """Yield (name, args_str, range_map, storage_map, body) for each weight fn.

    `range_map` / `storage_map` come from the `///` doc block that precedes the
    function (the benchmark generator emits them there)."""
    lines = text.splitlines()
    pending_ranges: dict[str, int] = {}
    pending_storage: dict[str, tuple[int, int]] = {}
    i = 0
    while i < len(lines):
        line = lines[i]
        for rm in _RANGE.finditer(line):
            pending_ranges[rm.group(1)] = int(rm.group(3))
        for sm in _STORAGE.finditer(line):
            pending_storage[sm.group(1)] = (int(sm.group(2)), int(sm.group(3)))
        m = _FN_HEADER.search(line)
        if not m:
            i += 1
            continue
        # Accumulate the body by brace counting from this line.
        depth = 0
        started = False
        body_lines = []
        j = i
        while j < len(lines):
            l = lines[j]
            body_lines.append(l)
            depth += l.count("{") - l.count("}")
            if "{" in l:
                started = True
            if started and depth <= 0:
                break
            j += 1
        yield m.group(1), m.group(2), dict(pending_ranges), dict(pending_storage), "\n".join(body_lines)
        pending_ranges = {}
        pending_storage = {}
        i = j + 1


def parse_weights(text: str) -> dict[str, WeightFn]:
    out: dict[str, WeightFn] = {}
    for name, args, ranges, storage, body in _split_functions(text):
        fn = WeightFn(name=name)
        fn.components = _ARG.findall(args)
        fn.storage = storage
        for c in fn.components:
            if c in ranges:
                fn.comp_max[c] = ranges[c]
            else:
                fn.missing_ranges.append(c)

        for m in _FROM_PARTS.finditer(body):
            ref, proof, comp = _num(m.group(1)), _num(m.group(2)), m.group(3)
            if comp is None:
                fn.ref_const += ref
                fn.proof_const += proof
            else:
                fn.ref_slope[comp] = fn.ref_slope.get(comp, 0) + ref
                fn.proof_slope[comp] = fn.proof_slope.get(comp, 0) + proof

        for count, comp in _db_terms("reads", body):
            if comp is None:
                fn.reads_const += count
            else:
                fn.reads_slope[comp] = fn.reads_slope.get(comp, 0) + count
        for count, comp in _db_terms("writes", body):
            if comp is None:
                fn.writes_const += count
            else:
                fn.writes_slope[comp] = fn.writes_slope.get(comp, 0) + count

        out[name] = fn
    return out


# --- git helpers -------------------------------------------------------------

def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def file_at_ref(ref: str, path: str) -> str | None:
    try:
        return subprocess.run(
            ["git", "show", f"{ref}:{path}"], check=True, capture_output=True, text=True
        ).stdout
    except subprocess.CalledProcessError:
        return None  # file did not exist at that ref


def current_text(path: str, current_ref: str | None) -> str | None:
    if current_ref is None:
        try:
            with open(path, encoding="utf-8") as fh:
                return fh.read()
        except OSError:
            return None
    return file_at_ref(current_ref, path)


def all_weight_files() -> list[str]:
    files = []
    for pat in WEIGHT_PATHS:
        for p in glob.glob(pat):
            if p.rsplit("/", 1)[-1] not in SKIP_BASENAMES:
                files.append(p)
    return sorted(set(files))


def changed_weight_files(base: str, current_ref: str | None) -> list[str]:
    rng = [base] if current_ref is None else [base, current_ref]
    out = git("diff", "--name-only", *rng, "--", *WEIGHT_PATHS)
    files = [
        p for p in out.splitlines()
        if p and p.rsplit("/", 1)[-1] not in SKIP_BASENAMES
    ]
    return sorted(set(files))


# --- reporting ---------------------------------------------------------------

def fmt(n: int) -> str:
    return f"{n:,}"


def pct_val(base: int, cur: int) -> float:
    """Signed % change; inf when growing from zero, 0 when both zero."""
    if base == 0:
        return 0.0 if cur == 0 else float("inf")
    return (cur - base) / base * 100


def pct(base: int, cur: int) -> str:
    if base == 0:
        return "n/a" if cur == 0 else "new"  # % change from zero is undefined
    return f"{pct_val(base, cur):+.1f}%"


def signed_fmt(n: int) -> str:
    sign = "+" if n >= 0 else "-"
    return f"{sign}{fmt(abs(n))}"


def fmt_ms(ps: int) -> str:
    return f"{ps / 1_000_000_000:.2f} ms"


def signed_ms(ps: int) -> str:
    sign = "+" if ps >= 0 else "-"
    return f"{sign}{abs(ps) / 1_000_000_000:.2f} ms"


def plural(n: int, singular: str, plural_form: str | None = None) -> str:
    return singular if n == 1 else (plural_form or f"{singular}s")


def _max_str(fn: WeightFn, overrides: dict[str, int]) -> str:
    """Per-extrinsic component maxima actually used, e.g. ``p=5, r=3`` (``none`` if none)."""
    if not fn.components:
        return "none"
    parts = []
    for c in fn.components:
        v = overrides.get(c, fn.comp_max.get(c))
        parts.append(f"{c}={fmt(v)}" if v is not None else f"{c}=?")
    return ", ".join(parts)


def storage_diff(b: WeightFn, c: WeightFn) -> list[dict]:
    """Per-item storage access changes, sorted by impact (largest r/w delta first)."""
    recs = []
    for item in set(b.storage) | set(c.storage):
        bv, cv = b.storage.get(item), c.storage.get(item)
        if bv == cv:
            continue
        if bv is None:
            kind, impact = "add", max(cv)
        elif cv is None:
            kind, impact = "remove", max(bv)
        else:
            kind, impact = "change", max(abs(cv[0] - bv[0]), abs(cv[1] - bv[1]))
        recs.append({"kind": kind, "item": item, "base": bv, "cur": cv, "impact": impact})
    recs.sort(key=lambda r: (-r["impact"], r["item"]))
    return recs


def report_file(path, base_text, cur_text, db_read, db_write, overrides, threshold, outliers, envs):
    """Append high-variance extrinsics to `outliers` and bench headers to `envs`."""
    base_fns = parse_weights(base_text) if base_text else {}
    cur_fns = parse_weights(cur_text) if cur_text else {}
    envs[path] = (parse_bench_env(base_text), parse_bench_env(cur_text))

    names = sorted(set(base_fns) | set(cur_fns))
    for name in names:
        b, c = base_fns.get(name), cur_fns.get(name)
        if b is None or c is None:
            continue
        # Each extrinsic is evaluated at its own per-component maxima.
        br, bp, breads, bwrites = b.evaluate(db_read, db_write, overrides)
        cr, cp, creads, cwrites = c.evaluate(db_read, db_write, overrides)
        if br == cr and bp == cp:
            continue  # identical worst-case weight

        ref_dev, proof_dev = pct_val(br, cr), pct_val(bp, cp)
        bmax, cmax = _max_str(b, overrides), _max_str(c, overrides)
        b_db_ref = breads * db_read + bwrites * db_write
        c_db_ref = creads * db_read + cwrites * db_write
        if max(abs(ref_dev), abs(proof_dev)) >= threshold:
            outliers.append({
                "path": path, "name": name,
                "br": br, "cr": cr, "ref_dev": ref_dev,
                "bp": bp, "cp": cp, "proof_dev": proof_dev,
                "breads": breads, "creads": creads,
                "bwrites": bwrites, "cwrites": cwrites,
                "b_db_ref": b_db_ref, "c_db_ref": c_db_ref,
                "b_exec_ref": br - b_db_ref, "c_exec_ref": cr - c_db_ref,
                "bmax": bmax, "cmax": cmax,
                "storage": storage_diff(b, c),
            })


def print_bench_env(base_env, cur_env):
    """Bench machine comparison block for one weights file, base vs current."""
    if base_env is None and cur_env is None:
        return

    def field(env, key):
        return (env or {}).get(key) or "unknown"

    print()
    print("   bench machine (base -> current)")
    for label, key in (("CPU", "cpu"), ("hostname", "hostname"),
                       ("date", "date"), ("sampling", "sampling")):
        bv, cv = field(base_env, key), field(cur_env, key)
        if bv == cv:
            print(f"     {label:<9} {bv}   (same)")
        else:
            print(f"     {label:<9} {bv} -> {cv}")
    b_cpu = (base_env or {}).get("cpu")
    c_cpu = (cur_env or {}).get("cpu")
    if b_cpu and c_cpu and b_cpu != c_cpu:
        print("     [!] CPU differs; ref_time deltas may reflect hardware, not code")


def print_high_variance(outliers, threshold, envs):
    """Threshold report for extrinsics whose worst-case weight moved a lot."""
    if not outliers:
        return

    def pallet(path):
        return path.rsplit("/", 1)[-1][:-3]  # strip dir + .rs

    def file_label(path):
        m = re.match(r"runtimes/([^/]+)/src/weights/([^/]+)$", path)
        if m:
            return f"runtimes/{m.group(1)} · weights/{m.group(2)}"
        return path

    def section_rule(path):
        label = f"── {file_label(path)} "
        return label + ("─" * max(0, 80 - len(label)))

    def ratio_label(base, cur):
        if base == 0:
            return "new cost" if cur else "unchanged"
        ratio = cur / base
        if ratio >= 1:
            return f"{ratio:.1f}× slower"
        return f"{base / cur:.1f}× faster" if cur else "removed cost"

    def rw(t):  # (reads, writes) -> "r:N w:M"
        return f"r:{t[0]} w:{t[1]}"

    def base_str(r):  # absent base => item didn't exist before (newly added)
        return rw(r["base"]) if r["base"] is not None else "new"

    def cur_str(r):  # absent cur => item is gone (removed)
        return rw(r["cur"]) if r["cur"] is not None else "removed"

    def storage_note(r):
        if r["base"] is None:
            return "added"
        if r["cur"] is None:
            return ""
        notes = []
        br, bw = r["base"]
        cr, cw = r["cur"]
        if br and cr / br >= 10 and cr % br == 0:
            notes.append(f"reads ×{cr // br}")
        elif br != cr:
            notes.append(f"reads {signed_fmt(cr - br)}")
        if bw and cw / bw >= 10 and cw % bw == 0:
            notes.append(f"writes ×{cw // bw}")
        return ", ".join(notes)

    def trigger_label(o):
        """Names the metric(s) whose deviation crossed the threshold."""
        parts = []
        if abs(o["ref_dev"]) >= threshold:
            part = f"ref_time {pct(o['br'], o['cr'])}"
            ratio = ratio_label(o["br"], o["cr"])
            if not ratio.startswith("1.0×"):
                part += f" ({ratio})"
            parts.append(part)
        if abs(o["proof_dev"]) >= threshold:
            parts.append(f"proof_size {pct(o['bp'], o['cp'])}")
        return " · ".join(parts)

    def delta_cell(delta_str, base, cur, triggered=False):
        """DELTA column cell: signed delta, percent and a trigger marker."""
        cell = "unchanged" if cur == base else f"{delta_str:<11} ({pct(base, cur)})"
        return cell + ("  ◀" if triggered else "")

    rows = sorted(outliers, key=lambda o: max(abs(o["ref_dev"]), abs(o["proof_dev"])), reverse=True)
    print(f"\n[!] {len(rows)} {plural(len(rows), 'extrinsic')} above threshold "
          f"(|Δ| ≥ {threshold:g}% in ref_time or proof_size)")

    previous_path = None
    for i, o in enumerate(rows):
        if o["path"] != previous_path:
            print(f"\n{section_rule(o['path'])}")
            print_bench_env(*envs.get(o["path"], (None, None)))
            print()
            previous_path = o["path"]
        # A rise in either flagging metric marks the entry as a regression.
        marker = "🔺" if o["cr"] > o["br"] or o["cp"] > o["bp"] else "🔻"
        label = f"{pallet(o['path'])}::{o['name']}"
        print(f"{marker} {label:<62} {trigger_label(o)}")
        print()
        print("   METRIC       BASE       CURRENT       DELTA")
        print(f"   ref_time     {fmt_ms(o['br']):<9} -> {fmt_ms(o['cr']):<12} "
              f"{delta_cell(signed_ms(o['cr'] - o['br']), o['br'], o['cr'], abs(o['ref_dev']) >= threshold)}")
        print(f"   proof_size   {fmt(o['bp']):<9} -> {fmt(o['cp']):<12} "
              f"{delta_cell(signed_fmt(o['cp'] - o['bp']), o['bp'], o['cp'], abs(o['proof_dev']) >= threshold)}")
        print(f"   db reads     {fmt(o['breads']):<9} -> {fmt(o['creads']):<12} "
              f"{delta_cell(signed_fmt(o['creads'] - o['breads']), o['breads'], o['creads'])}")
        print(f"   db writes    {fmt(o['bwrites']):<9} -> {fmt(o['cwrites']):<12} "
              f"{delta_cell(signed_fmt(o['cwrites'] - o['bwrites']), o['bwrites'], o['cwrites'])}")
        print()
        print("   time breakdown")
        print(f"     computation   {signed_ms(o['c_exec_ref'] - o['b_exec_ref'])}")
        print(f"     db access     {signed_ms(o['c_db_ref'] - o['b_db_ref'])}")

        recs = o["storage"]  # already sorted by impact (largest r/w delta first)
        if not recs:
            print()
            print("   storage (base -> current)")
            print("   (no storage-access change)")
            if i != len(rows) - 1:
                print()
            continue

        item_w = max(len(r["item"]) for r in recs)
        base_w = max(len(base_str(r)) for r in recs)
        print()
        print("   storage (base -> current)")
        for r in recs:
            note = storage_note(r)
            suffix = f"    {note}" if note else ""
            print(f"   • {r['item'].ljust(item_w)}  {base_str(r).ljust(base_w)}  ->  {cur_str(r)}{suffix}")
        if i != len(rows) - 1:
            print()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base", default="origin/main",
                    help="base branch (default: origin/main); the merge-base of this branch and "
                         "the current ref is used, so changes made on the base branch after the "
                         "current branch forked are ignored")
    ap.add_argument("--current", default=None,
                    help="current git ref (default: working tree, i.e. HEAD + uncommitted)")
    ap.add_argument("--no-merge-base", action="store_true",
                    help="compare against --base directly instead of its merge-base with current")
    ap.add_argument("--all", action="store_true",
                    help="scan every weight file, not just those changed vs base")
    ap.add_argument("--pallet", action="append", default=[],
                    help="restrict to weight files whose name contains this substring (repeatable)")
    ap.add_argument("--max", action="append", default=[], metavar="NAME=VALUE",
                    help="override a component's max value, e.g. --max p=15 (repeatable)")
    ap.add_argument("--db-read-ref", type=int, default=DEFAULT_DB_READ_PS,
                    help=f"ref_time ps per storage read (default {DEFAULT_DB_READ_PS})")
    ap.add_argument("--db-write-ref", type=int, default=DEFAULT_DB_WRITE_PS,
                    help=f"ref_time ps per storage write (default {DEFAULT_DB_WRITE_PS})")
    ap.add_argument("--threshold", type=float, default=10.0,
                    help="flag deviations whose magnitude is >= this %% (default 10)")
    args = ap.parse_args()

    overrides: dict[str, int] = {}
    for item in args.max:
        k, _, v = item.partition("=")
        overrides[k.strip()] = int(v)

    # Resolve the base to the merge-base with the current ref, so changes landed
    # on the base branch after this branch forked are not attributed to it.
    if args.no_merge_base:
        base_ref = args.base
        base_desc = args.base
    else:
        try:
            base_ref = git("merge-base", args.base, args.current or "HEAD").strip()
        except subprocess.CalledProcessError:
            print(f"error: no merge-base between {args.base} and "
                  f"{args.current or 'HEAD'}", file=sys.stderr)
            return 1
        base_desc = f"{args.base} @ {base_ref[:12]}  (merge-base)"

    files = all_weight_files() if args.all else changed_weight_files(base_ref, args.current)
    if args.pallet:
        files = [f for f in files if any(p in f for p in args.pallet)]

    if args.current is None:
        has_tracked_edits = bool(files) and subprocess.run(
            ["git", "diff", "--quiet", "HEAD", "--", *files]
        ).returncode != 0
        cur_desc = "working tree" if has_tracked_edits else "HEAD"
    else:
        cur_desc = args.current

    print(f"Base:    {base_desc}\nCurrent: {cur_desc}")

    if not files:
        print(f"\nNo affected weight files between {base_desc} and {cur_desc}.")
        return 0

    outliers: list[dict] = []
    envs: dict[str, tuple] = {}
    printed = 0
    for path in files:
        base_text = file_at_ref(base_ref, path)
        cur_text = current_text(path, args.current)
        if base_text is None and cur_text is None:
            continue
        report_file(path, base_text, cur_text, args.db_read_ref,
                    args.db_write_ref, overrides, args.threshold, outliers, envs)
        printed += 1

    if printed == 0:
        print("No worst-case weight differences found.")
        return 0

    print_high_variance(outliers, args.threshold, envs)
    print(f"\n{len(files)} {plural(len(files), 'file')} scanned · "
          f"{len(outliers)} {plural(len(outliers), 'extrinsic')} above {args.threshold:g}% threshold")
    print("measured at documented component maxima"
          + (f" (overridden: {', '.join(args.max)})" if args.max else "")
          + " - pass --max NAME=VALUE for charge-site maxima")
    return 2 if outliers else 0


if __name__ == "__main__":
    sys.exit(main())
