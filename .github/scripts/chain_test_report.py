"""Report advisory chain tests using trusted workflow metadata and inert JUnit data."""

import html
import json
import os
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET


MARKER = "<!-- individuality-chain-tests -->"
ORDER = re.compile(r"<!-- chain-tests-run:(\d+):(\d+) -->")
FAILURES = {"failure", "timed_out", "action_required", "startup_failure"}


def api(endpoint, *, body=None, method="GET", paginate=False):
    command = ["gh", "api", endpoint, "--method", method]
    if paginate:
        command += ["--paginate", "--slurp"]
    if body is not None:
        command += ["--input", "-"]
    result = subprocess.run(
        command, input=json.dumps(body) if body is not None else None,
        text=True, capture_output=True, check=True, timeout=60,
    )
    return json.loads(result.stdout)


def code(value):
    # Artifact text must not create HTML, links or mention notifications.
    value = " ".join(str(value).split())[:240]
    value = html.escape(value).replace("@", "&#64;")
    value = re.sub(r"[\\`*_\[\]~!]", lambda match: f"&#{ord(match[0])};", value)
    return "<code>" + value + "</code>"


def test_failures(directory):
    failures = set()
    for path in sorted(directory.rglob("*.xml"))[:100]:
        # Treat downloaded reports as data. Do not follow symlinks or parse DTDs.
        if path.is_symlink() or path.stat().st_size > 2_000_000:
            continue
        data = path.read_bytes()
        if b"<!DOCTYPE" in data or b"<!ENTITY" in data:
            continue
        try:
            root = ET.fromstring(data)
        except ET.ParseError:
            continue
        artifact = path.relative_to(directory).parts[0]
        network = artifact.removeprefix("release-gate-test-results-")
        for suite in root.iter("testsuite"):
            cases = [case for case in suite.findall("testcase")
                     if case.find("failure") is not None or case.find("error") is not None]
            for case in cases:
                label = " / ".join(filter(None, [suite.get("name"), case.get("name")]))
                failures.add(f"{network}: {label}")
            if not cases and any(suite.get(key, "0").isdigit() and int(suite.get(key, "0")) > 0
                                 for key in ("failures", "errors")):
                failures.add(f"{network}: {suite.get('name', 'Unnamed suite')}")
    return sorted(failures)


def compose(run, jobs, directory):
    chain_jobs = [job for job in jobs if job["name"].startswith("chain-tests / ")]
    failed = [job for job in chain_jobs if job["conclusion"] in FAILURES]
    gates = [job for job in chain_jobs if job["name"].startswith("chain-tests / gate (")]
    if not failed and (not gates or any(job["conclusion"] != "success" for job in chain_jobs)):
        # A skipped, canceled or absent gate cannot resolve an earlier warning.
        return None

    lines = [MARKER, f"<!-- chain-tests-run:{run['run_number']}:{run['run_attempt']} -->",
             "### Chain compatibility tests", ""]
    if failed:
        lines += ["⚠️ Chain tests found problems. These results are advisory and do not block this PR.", ""]
        for job in failed:
            lines.append(f"- [{code(job['name'])}]({job['html_url']})")
            for step in job.get("steps", []):
                if step["conclusion"] in FAILURES:
                    lines.append(f"  - Failed step: {code(step['name'])}")
        failures = test_failures(directory)
        if failures:
            lines += ["", "**Failed tests or suites:**", ""]
            lines += [f"- {code(name)}" for name in failures[:30]]
            if len(failures) > 30:
                lines.append(f"- {len(failures) - 30} more; see the run artifacts.")
        else:
            lines += ["", "No readable failing test report is available. The gate may have failed during setup or runtime upgrade; see the failed steps above."]
        lines += ["", "Check compatibility with downstream dependencies before release."]
    else:
        lines += ["✅ Chain tests passed. The previously reported warning is resolved."]
    lines += ["", f"Commit {code(run['head_sha'][:12])} · [Workflow run, attempt {run['run_attempt']}]({run['html_url']}/attempts/{run['run_attempt']})"]
    return bool(failed), "\n".join(lines)


def report(run, repository, directory, request=api):
    if run["event"] != "pull_request" or run["conclusion"] in {"cancelled", "skipped"}:
        return
    prefix = f"repos/{repository}"
    latest = request(f"{prefix}/actions/runs/{run['id']}")
    if latest["run_attempt"] != run["run_attempt"] or latest["status"] != "completed":
        return
    pages = request(f"{prefix}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100", paginate=True)
    result = compose(run, [job for page in pages for job in page["jobs"]], directory)
    if result is None:
        return
    failed, body = result
    pulls = run["pull_requests"]
    if not pulls:
        pages = request(f"{prefix}/commits/{run['head_sha']}/pulls?per_page=100", paginate=True)
        pulls = [pull for page in pages for pull in page]
    for associated in pulls:
        number = associated["number"]
        pull = request(f"{prefix}/pulls/{number}")
        if (pull["state"] != "open" or pull["head"]["sha"] != run["head_sha"]
                or pull["base"]["repo"]["full_name"] != repository):
            continue
        pages = request(f"{prefix}/issues/{number}/comments?per_page=100", paginate=True)
        comments = [comment for page in pages for comment in page
                    if comment["user"]["login"] == "github-actions[bot]"
                    and comment["user"]["type"] == "Bot"
                    and comment.get("body", "").startswith(MARKER)]
        existing = comments[-1] if comments else None
        if existing:
            order = ORDER.search(existing["body"])
            if order and tuple(map(int, order.groups())) > (run["run_number"], run["run_attempt"]):
                continue
            if existing["body"] != body:
                request(f"{prefix}/issues/comments/{existing['id']}", method="PATCH", body={"body": body})
        elif failed:
            request(f"{prefix}/issues/{number}/comments", method="POST", body={"body": body})


if __name__ == "__main__":
    with open(os.environ["GITHUB_EVENT_PATH"]) as event_file:
        event = json.load(event_file)
    report(event["workflow_run"], os.environ["GITHUB_REPOSITORY"], Path(os.environ["RESULTS_DIR"]))
