"""Report advisory chain tests using trusted workflow metadata and inert JUnit data."""

from datetime import datetime
import html
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import xml.etree.ElementTree as ET


MARKER = "<!-- individuality-chain-tests -->"
ORDER = re.compile(r"<!-- chain-tests-run:(\d+):(\d+) -->")
FAILURES = {"failure", "timed_out", "action_required", "startup_failure"}
# Leave room for the run link and an overflow notice in a single GitHub comment.
FAILURE_LIST_BYTES = 50_000


def api(endpoint, *, body=None, method="GET", paginate=False):
    command = ["gh", "api", endpoint, "--method", method]
    if paginate:
        command += ["--paginate", "--slurp"]
    if body is not None:
        command += ["--input", "-"]
    try:
        result = subprocess.run(
            command, input=json.dumps(body) if body is not None else None,
            text=True, capture_output=True, check=True, timeout=60,
        )
    except subprocess.CalledProcessError as error:
        diagnostic = error.stderr or "GitHub API request failed"
        for key, value in os.environ.items():
            if value and any(word in key.upper() for word in ("TOKEN", "SECRET", "PASSWORD", "CREDENTIAL")):
                diagnostic = diagnostic.replace(value, "[REDACTED]")
        print(diagnostic, file=sys.stderr)
        raise
    return json.loads(result.stdout)


def code(value):
    # Artifact text must not create HTML, links or mention notifications.
    value = " ".join(str(value).split())[:240]
    value = html.escape(value).replace("@", "&#64;")
    value = re.sub(r"[\\`*_\[\]~!]", lambda match: f"&#{ord(match[0])};", value)
    return "<code>" + value + "</code>"


def list_artifacts(run, repository, request=api):
    pages = request(f"repos/{repository}/actions/runs/{run['id']}/artifacts?per_page=100", paginate=True)
    return [artifact for page in pages for artifact in page["artifacts"]
            if artifact["name"].startswith("release-gate-test-results-") and not artifact["expired"]]


def belongs_to_job(artifact, job):
    match = re.fullmatch(r"chain-tests / gate \((.+) · (.+)\)", job["name"])
    if not match or artifact["name"] != f"release-gate-test-results-{match[1]}-{match[2]}":
        return False
    try:
        created = datetime.fromisoformat(artifact["created_at"].replace("Z", "+00:00"))
        started = datetime.fromisoformat(job["started_at"].replace("Z", "+00:00"))
        completed = datetime.fromisoformat(job["completed_at"].replace("Z", "+00:00"))
        return started <= created <= completed
    except (AttributeError, KeyError, TypeError, ValueError):
        return False


def test_failures(directory, artifacts, jobs) -> dict[str, list[str]]:
    failures = {}
    for artifact in artifacts:
        if not any(job["conclusion"] in FAILURES and belongs_to_job(artifact, job) for job in jobs):
            continue
        name = artifact["name"]
        if Path(name).name != name:
            continue
        # download-artifact v8 puts a single artifact directly in the destination.
        root_dir = directory if len(artifacts) == 1 else directory / name
        labels = set()
        for path in sorted(root_dir.rglob("*.xml")):
            ancestors = (part for part in (path, *path.parents)
                         if part == directory or directory in part.parents)
            if any(part.is_symlink() for part in ancestors) or path.stat().st_size > 2_000_000:
                continue
            try:
                data = path.read_bytes().decode("utf-8-sig")
                # Reject UTF-16/32, including BOM-less input, before parsing declarations.
                if "\x00" in data or "<!DOCTYPE" in data or "<!ENTITY" in data:
                    continue
                declaration = re.match(r'<\?xml\s+[^?]*encoding\s*=\s*[\'"]([^\'"]+)', data)
                if declaration and declaration[1].lower() != "utf-8":
                    continue
                root = ET.fromstring(data)
            except (UnicodeError, ET.ParseError):
                continue
            network = name.removeprefix("release-gate-test-results-")
            for suite in root.iter("testsuite"):
                cases = [case for case in suite.findall("testcase")
                         if case.find("failure") is not None or case.find("error") is not None]
                for case in cases:
                    label = " / ".join(filter(None, [suite.get("name"), case.get("name")]))
                    labels.add(f"{network}: {label}")
                if not cases and suite.find("testsuite") is None and any(
                        suite.get(key, "0").isdigit() and int(suite.get(key, "0")) > 0
                        for key in ("failures", "errors")):
                    labels.add(f"{network}: {suite.get('name', 'Unnamed suite')}")
        failures[name] = sorted(labels)
    return failures


def compose(run, jobs, directory, artifacts=(), download_outcome="success"):
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
        reports = test_failures(directory, artifacts, failed) if download_outcome == "success" else {}
        failures = sorted({label for labels in reports.values() for label in labels})
        if failures:
            lines += ["", "**Failed tests or suites:**", ""]
            remaining = FAILURE_LIST_BYTES
            for index, name in enumerate(failures):
                line = f"- {code(name)}"
                remaining -= len((line + "\n").encode("utf-8"))
                if remaining < 0:
                    lines.append(f"- {len(failures) - index} more failures exceed this comment's size budget; see the [full test artifacts]({run['html_url']}).")
                    break
                lines.append(line)
        elif download_outcome != "success":
            lines += ["", "The test reports could not be downloaded; see the failed jobs and steps above."]
        else:
            lines += ["", "No readable failing test report is available. The gate may have failed during setup or runtime upgrade; see the failed steps above."]
        lines += ["", "Check compatibility with downstream dependencies before release."]
    else:
        lines += ["✅ Chain tests passed. The previously reported warning is resolved."]
    lines += ["", f"Commit {code(run['head_sha'][:12])} · [Workflow run, attempt {run['run_attempt']}]({run['html_url']}/attempts/{run['run_attempt']})"]
    return bool(failed), "\n".join(lines)


def report(run, repository, directory, request=api, *, artifacts=(), download_outcome="success"):
    if run["event"] != "pull_request" or run["conclusion"] in {"cancelled", "skipped"}:
        return
    prefix = f"repos/{repository}"
    latest = request(f"{prefix}/actions/runs/{run['id']}")
    if latest["run_attempt"] != run["run_attempt"] or latest["status"] != "completed":
        return
    pages = request(f"{prefix}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100", paginate=True)
    result = compose(run, [job for page in pages for job in page["jobs"]], directory, artifacts, download_outcome)
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
                or pull["base"]["repo"]["full_name"] != repository
                or (pull["head"].get("repo") or {}).get("full_name") != repository):
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
    manifest = Path(os.environ["ARTIFACT_MANIFEST"])
    if "--list-artifacts" in sys.argv:
        artifacts = list_artifacts(event["workflow_run"], os.environ["GITHUB_REPOSITORY"])
        manifest.write_text(json.dumps(artifacts))
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            print("ids=" + ",".join(str(artifact["id"]) for artifact in artifacts), file=output)
    else:
        artifacts = json.loads(manifest.read_text()) if manifest.exists() else []
        report(event["workflow_run"], os.environ["GITHUB_REPOSITORY"], Path(os.environ["RESULTS_DIR"]),
               artifacts=artifacts, download_outcome=os.environ["DOWNLOAD_OUTCOME"])
