import copy
import contextlib
import io
import subprocess
from unittest.mock import patch
from pathlib import Path
import tempfile
import unittest

from chain_test_report import MARKER, api, compose, list_artifacts, report


RUN = {
    "id": 123, "run_number": 10, "run_attempt": 1, "event": "pull_request",
    "conclusion": "success", "head_sha": "a" * 40,
    "html_url": "https://github.com/owner/repo/actions/runs/123",
    "pull_requests": [{"number": 7}],
}


def job(name, conclusion="success", steps=None):
    return {"name": f"chain-tests / {name}", "conclusion": conclusion,
            "html_url": "https://github.com/owner/repo/actions/runs/123/job/456",
            "steps": steps or [], "started_at": "2026-10-05T10:00:00Z",
            "completed_at": "2026-10-05T11:00:00Z"}


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.run = copy.deepcopy(RUN)
        self.jobs = [job("plan"), job("gate (previewnet · target)", "failure"),
                     job("gate (paseo-next-v2 · target)")]
        self.pull = {"state": "open", "head": {"sha": RUN["head_sha"], "repo": {"full_name": "owner/repo"}},
                     "base": {"repo": {"full_name": "owner/repo"}}}
        self.artifacts = []
        self.old_jobs = []
        self.download_outcome = "success"
        self.comments = []
        self.writes = []
        self.latest = {"run_attempt": 1, "status": "completed"}

    def request(self, endpoint, *, method="GET", body=None, paginate=False):
        if method != "GET":
            self.writes.append((method, endpoint, body["body"]))
            return {}
        if endpoint == "repos/owner/repo/actions/runs/123":
            return self.latest
        if "/artifacts?" in endpoint:
            return [{"artifacts": [{"name": "unrelated", "expired": False},
                                    {"name": "release-gate-test-results-expired", "expired": True}]},
                    {"artifacts": self.artifacts}]
        if "/jobs?" in endpoint:
            jobs = self.jobs if "filter=latest" in endpoint else self.old_jobs + self.jobs
            return [{"jobs": jobs[:1]}, {"jobs": jobs[1:]}]
        if "/commits/" in endpoint:
            return [[{"number": 7}]]
        if "/comments?" in endpoint:
            return [[], self.comments]
        if endpoint == "repos/owner/repo/pulls/7":
            return self.pull
        self.fail(f"Unexpected request: {endpoint}")

    def existing(self, number=9, attempt=1, login="github-actions[bot]", user_type="Bot"):
        self.comments = [{"id": 8, "body": f"{MARKER}\n<!-- chain-tests-run:{number}:{attempt} -->\nOld warning",
                          "user": {"login": login, "type": user_type}}]

    def execute(self):
        report(self.run, "owner/repo", self.directory, self.request,
               artifacts=list_artifacts(self.run, "owner/repo", self.request),
               download_outcome=self.download_outcome)

    def xml(self, name, content, network="previewnet", candidate="target"):
        artifact = f"release-gate-test-results-{network}-{candidate}"
        if not any(item["name"] == artifact for item in self.artifacts):
            if len(self.artifacts) == 1:
                files = list(self.directory.iterdir())
                destination = self.directory / self.artifacts[0]["name"]
                destination.mkdir()
                for file in files:
                    file.rename(destination / file.name)
            self.artifacts.append({"name": artifact, "id": len(self.artifacts) + 1,
                                   "created_at": "2026-10-05T10:59:00Z", "expired": False})
        root = self.directory if len(self.artifacts) == 1 else self.directory / artifact
        root.mkdir(exist_ok=True)
        path = root / name
        path.write_bytes(content if isinstance(content, bytes) else content.encode())

    def test_failure_creates_comment_even_when_workflow_passes(self):
        self.xml("tests.xml", '<testsuites><testsuite failures="1" name="aliases"><testcase name="claim"><failure/></testcase></testsuite></testsuites>')
        self.execute()
        self.assertEqual(len(self.writes), 1)
        method, endpoint, body = self.writes[0]
        self.assertEqual(method, "POST")
        self.assertEqual(endpoint, "repos/owner/repo/issues/7/comments")
        self.assertIn("previewnet-target: aliases / claim", body)
        self.assertIn("do not block this PR", body)
        self.assertIn("/attempts/1", body)

    def test_repeated_failure_updates_one_comment(self):
        self.existing()
        self.execute()
        self.assertEqual(len(self.writes), 1)
        self.assertEqual(self.writes[0][:2], ("PATCH", "repos/owner/repo/issues/comments/8"))

    def test_all_network_failures_share_one_comment_across_reruns(self):
        self.jobs[2]["conclusion"] = "failure"
        self.xml("tests.xml", '<testsuite name="identity"><testcase name="register"><failure/></testcase></testsuite>')
        self.xml("tests.xml", '<testsuite name="allowances"><testcase name="claim"><failure/></testcase></testsuite>', network="paseo-next-v2")
        self.execute()
        self.assertEqual(len(self.writes), 1)
        self.assertEqual(self.writes[0][0], "POST")
        body = self.writes[0][2]
        self.assertIn("previewnet-target: identity / register", body)
        self.assertIn("paseo-next-v2-target: allowances / claim", body)
        self.assertIn("chain-tests / gate (previewnet", body)
        self.assertIn("chain-tests / gate (paseo-next-v2", body)

        self.existing(number=10)
        self.comments[0]["body"] = body
        self.run["run_attempt"] = 2
        self.latest["run_attempt"] = 2
        self.execute()
        self.assertEqual([write[0] for write in self.writes], ["POST", "PATCH"])
        self.assertEqual(self.writes[1][1], "repos/owner/repo/issues/comments/8")

    def test_only_latest_attempt_jobs_are_reported(self):
        self.run["run_attempt"] = self.latest["run_attempt"] = 2
        self.old_jobs = [job("gate (obsolete · target)", "failure")]
        self.jobs[1]["steps"] = [{"name": "Current failure", "conclusion": "failure"}]
        self.execute()
        body = self.writes[0][2]
        self.assertIn("Current failure", body)
        self.assertNotIn("obsolete", body)

    def test_passing_gate_and_other_candidate_artifacts_add_nothing(self):
        self.xml("junit.xml", '<testsuite name="passing gate old failure" failures="1"/>', network="paseo-next-v2")
        self.xml("junit.xml", '<testsuite name="wrong candidate" failures="1"/>', candidate="other")
        self.execute()
        body = self.writes[0][2]
        self.assertNotIn("passing gate old failure", body)
        self.assertNotIn("wrong candidate", body)

    def test_rerun_setup_failure_does_not_reuse_old_artifact(self):
        self.xml("junit.xml", '<testsuite name="old failure" failures="1"/>')
        self.run["run_attempt"] = self.latest["run_attempt"] = 2
        self.jobs[1].update(started_at="2026-10-06T10:00:00Z", completed_at="2026-10-06T11:00:00Z",
                            steps=[{"name": "Start fork", "conclusion": "failure"}])
        self.execute()
        body = self.writes[0][2]
        self.assertIn("Start fork", body)
        self.assertNotIn("old failure", body)

    def test_partial_rerun_keeps_retained_job_report(self):
        self.xml("junit.xml", '<testsuite name="retained failure" failures="1"/>')
        self.run["run_attempt"] = self.latest["run_attempt"] = 2
        self.run["run_started_at"] = "2026-10-06T10:00:00Z"
        self.jobs[1]["run_attempt"] = 2
        self.execute()
        self.assertIn("previewnet-target: retained failure", self.writes[0][2])

    def test_missing_or_outside_job_times_do_not_attribute_tests(self):
        self.xml("junit.xml", '<testsuite name="unverified failure" failures="1"/>')
        for timestamp in (None, "invalid", "2026-10-05T11:01:00Z"):
            with self.subTest(timestamp=timestamp):
                self.jobs[1]["started_at"] = timestamp
                self.execute()
                self.assertNotIn("unverified failure", self.writes[-1][2])

    def test_nested_suite_does_not_duplicate_failure(self):
        self.xml("junit.xml", '<testsuite name="parent" failures="1"><testsuite name="leaf" failures="1"><testcase name="broken"><failure/></testcase></testsuite></testsuite>')
        self.execute()
        body = self.writes[0][2]
        self.assertIn("leaf / broken", body)
        self.assertNotIn("parent", body)

    def test_utf16_entity_reports_are_rejected(self):
        xml = '<?xml version="1.0" encoding="UTF-16"?><!DOCTYPE testsuite [<!ENTITY x "entity failure">]><testsuite name="&x;" failures="1"/>'
        for encoding in ("utf-16", "utf-16-le", "utf-16-be"):
            with self.subTest(encoding=encoding):
                self.xml("junit.xml", xml.encode(encoding))
                self.execute()
                self.assertNotIn("entity failure", self.writes[-1][2])
                self.assertIn("No readable", self.writes[-1][2])

    def test_fork_pr_is_ignored(self):
        self.pull["head"]["repo"]["full_name"] = "fork/repo"
        self.execute()
        self.assertEqual(self.writes, [])

    def test_download_failure_is_explicit(self):
        self.xml("junit.xml", '<testsuite name="partial download" failures="1"/>')
        self.download_outcome = "failure"
        self.execute()
        body = self.writes[0][2]
        self.assertIn("test reports could not be downloaded", body)
        self.assertNotIn("partial download", body)
        self.assertNotIn("may have failed during setup", body)

    def test_api_failure_prints_stderr_without_credentials(self):
        error = subprocess.CalledProcessError(1, ["gh", "api"], stderr="HTTP 403: denied secret-token")
        output = io.StringIO()
        with patch("chain_test_report.subprocess.run", side_effect=error), \
                patch.dict("os.environ", {"GH_TOKEN": "secret-token"}), contextlib.redirect_stderr(output):
            with self.assertRaises(subprocess.CalledProcessError):
                api("repos/owner/repo")
        self.assertIn("HTTP 403: denied", output.getvalue())
        self.assertNotIn("secret-token", output.getvalue())

    def test_initial_success_stays_silent(self):
        self.jobs[1]["conclusion"] = "success"
        self.execute()
        self.assertEqual(self.writes, [])

    def test_recovery_updates_existing_warning(self):
        self.jobs[1]["conclusion"] = "success"
        self.existing()
        self.execute()
        self.assertIn("warning is resolved", self.writes[0][2])

    def test_other_ci_failure_is_not_reported(self):
        self.jobs[1]["conclusion"] = "success"
        self.jobs.append({"name": "test-all", "conclusion": "failure"})
        self.run["conclusion"] = "failure"
        self.execute()
        self.assertEqual(self.writes, [])

    def test_setup_failure_names_step_without_junit(self):
        self.jobs[1]["steps"] = [{"name": "Start the fork", "conclusion": "failure"}]
        self.execute()
        self.assertIn("Start the fork", self.writes[0][2])
        self.assertIn("No readable failing test report", self.writes[0][2])

    def test_plan_failure_is_reported_without_matrix(self):
        self.jobs = [job("plan", "failure")]
        self.execute()
        self.assertIn("chain-tests / plan", self.writes[0][2])

    def test_timeout_is_reported(self):
        self.jobs[1]["conclusion"] = "timed_out"
        self.execute()
        self.assertEqual(len(self.writes), 1)

    def test_skipped_canceled_or_missing_gates_cannot_resolve_warning(self):
        self.existing()
        for conclusion in ("skipped", "cancelled", None):
            with self.subTest(conclusion=conclusion):
                self.jobs[1]["conclusion"] = conclusion
                self.execute()
                self.assertEqual(self.writes, [])
        self.jobs = [job("plan")]
        self.execute()
        self.assertEqual(self.writes, [])

    def test_canceled_run_and_non_pr_run_stay_silent(self):
        self.run["conclusion"] = "cancelled"
        self.execute()
        self.run["conclusion"] = "failure"
        self.run["event"] = "push"
        self.execute()
        self.assertEqual(self.writes, [])

    def test_stale_commit_closed_pr_and_wrong_repository_stay_silent(self):
        original = copy.deepcopy(self.pull)
        for changed in ({"head": {"sha": "b" * 40}}, {"state": "closed"},
                        {"base": {"repo": {"full_name": "someone/else"}}}):
            self.pull = {**original, **changed}
            self.execute()
            self.assertEqual(self.writes, [])

    def test_older_run_or_attempt_cannot_overwrite_newer_comment(self):
        for number, attempt in ((11, 1), (10, 2)):
            self.existing(number, attempt)
            self.execute()
            self.assertEqual(self.writes, [])

    def test_newer_or_in_progress_rerun_supersedes_completed_event(self):
        for latest in ({"run_attempt": 2, "status": "completed"},
                       {"run_attempt": 1, "status": "in_progress"}):
            self.latest = latest
            self.execute()
            self.assertEqual(self.writes, [])

    def test_missing_pr_association_uses_commit_lookup(self):
        self.run["pull_requests"] = []
        self.execute()
        self.assertEqual(len(self.writes), 1)

    def test_marker_from_human_is_not_overwritten(self):
        self.existing(login="contributor", user_type="User")
        self.execute()
        self.assertEqual(self.writes[0][0], "POST")

    def test_identical_report_does_not_write_again(self):
        self.existing()
        self.comments[0]["body"] = compose(self.run, self.jobs, self.directory)[1]
        self.execute()
        self.assertEqual(self.writes, [])

    def test_junit_errors_suite_only_failures_and_malformed_files(self):
        self.xml("errors.xml", '<testsuite name="identity"><testcase name="register"><error/></testcase></testsuite>')
        self.xml("suite.xml", '<testsuite errors="2" name="setup"/>')
        self.xml("broken.xml", '<not-xml')
        self.xml("encoding.xml", '<?xml version="1.0" encoding = "ISO-8859-1"?><testsuite name="ignored encoding" failures="1"/>')
        self.xml("oversize.xml", '<testsuite name="ignored oversize" failures="1">' + ' ' * 2_000_000 + '</testsuite>')
        self.xml("dtd.xml", '<!DOCTYPE a [<!ENTITY x "no">]><testsuite name="ignored" failures="1"/>')
        self.execute()
        body = self.writes[0][2]
        self.assertIn("previewnet-target: identity / register", body)
        self.assertIn("previewnet-target: setup", body)
        self.assertNotIn("ignored", body)

    def test_artifact_names_are_escaped_and_mentions_disabled(self):
        self.xml("unsafe.xml", '<testsuite name="&lt;img&gt; @team" failures="1"/>')
        self.execute()
        body = self.writes[0][2]
        self.assertNotIn("<img>", body)
        self.assertNotIn("@team", body)
        self.assertIn("&lt;img&gt; &#64;team", body)

    def test_comment_includes_more_than_thirty_failures(self):
        cases = ''.join(f'<testcase name="failure-{i}"><failure/></testcase>' for i in range(40))
        self.xml("many.xml", f'<testsuite name="suite">{cases}</testsuite>')
        self.execute()
        body = self.writes[0][2]
        for i in range(40):
            self.assertIn(f"suite / failure-{i}</code>", body)
        self.assertNotIn("more failures", body)
        self.assertEqual(len(self.writes), 1)

    def test_failures_from_all_report_files_are_included(self):
        for i in range(101):
            self.xml(f"{i:03}.xml", f'<testsuite name="suite-{i}" failures="1"/>')
        self.execute()
        self.assertIn("suite-100</code>", self.writes[0][2])
        self.assertEqual(self.writes[0][2].count("previewnet-target:"), 101)

    def test_oversized_report_links_to_artifacts_without_extra_comments(self):
        cases = ''.join(f'<testcase name="{i}-' + 'x' * 200 + '"><failure/></testcase>' for i in range(400))
        self.xml("many.xml", f'<testsuite name="suite">{cases}</testsuite>')
        self.execute()
        body = self.writes[0][2]
        self.assertEqual(len(self.writes), 1)
        self.assertIn("more failures exceed this comment's size budget", body)
        self.assertIn(f"[full test artifacts]({RUN['html_url']})", body)
        self.assertLess(len(body.encode("utf-8")), 60_000)

    def test_artifact_markdown_cannot_create_links(self):
        self.xml("markdown.xml", '<testsuite name="[click](https://example.com) `code` *bold*" failures="1"/>')
        self.execute()
        body = self.writes[0][2]
        self.assertNotIn("[click]", body)
        self.assertNotIn("`code`", body)
        self.assertNotIn("*bold*", body)
        self.assertIn("&#91;click&#93;", body)


if __name__ == "__main__":
    unittest.main()
