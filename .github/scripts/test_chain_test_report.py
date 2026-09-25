import copy
from pathlib import Path
import tempfile
import unittest

from chain_test_report import MARKER, compose, report, test_failures


RUN = {
    "id": 123, "run_number": 10, "run_attempt": 1, "event": "pull_request",
    "conclusion": "success", "head_sha": "a" * 40,
    "html_url": "https://github.com/owner/repo/actions/runs/123",
    "pull_requests": [{"number": 7}],
}


def job(name, conclusion="success", steps=None):
    return {"name": f"chain-tests / {name}", "conclusion": conclusion,
            "html_url": "https://github.com/owner/repo/actions/runs/123/job/456",
            "steps": steps or []}


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.run = copy.deepcopy(RUN)
        self.jobs = [job("plan"), job("gate (previewnet · target)", "failure"),
                     job("gate (paseo-next-v2 · target)")]
        self.pull = {"state": "open", "head": {"sha": RUN["head_sha"]},
                     "base": {"repo": {"full_name": "owner/repo"}}}
        self.comments = []
        self.writes = []
        self.latest = {"run_attempt": 1, "status": "completed"}

    def request(self, endpoint, *, method="GET", body=None, paginate=False):
        if method != "GET":
            self.writes.append((method, endpoint, body["body"]))
            return {}
        if endpoint == "repos/owner/repo/actions/runs/123":
            return self.latest
        if "/jobs?" in endpoint:
            self.assertTrue(paginate)
            self.assertIn("filter=latest", endpoint)
            return [{"jobs": self.jobs[:1]}, {"jobs": self.jobs[1:]}]
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
        report(self.run, "owner/repo", self.directory, self.request)

    def xml(self, name, content):
        path = self.directory / "release-gate-test-results-previewnet-target" / name
        path.parent.mkdir(exist_ok=True)
        path.write_text(content)

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
        self.xml("dtd.xml", '<!DOCTYPE a [<!ENTITY x "no">]><testsuite name="ignored" failures="1"/>')
        self.assertEqual(test_failures(self.directory),
                         ["previewnet-target: identity / register", "previewnet-target: setup"])

    def test_artifact_names_are_escaped_and_mentions_disabled(self):
        self.xml("unsafe.xml", '<testsuite name="&lt;img&gt; @team" failures="1"/>')
        self.execute()
        body = self.writes[0][2]
        self.assertNotIn("<img>", body)
        self.assertNotIn("@team", body)
        self.assertIn("&lt;img&gt; &#64;team", body)

    def test_long_reports_are_bounded(self):
        cases = ''.join(f'<testcase name="failure-{i}"><failure/></testcase>' for i in range(40))
        self.xml("many.xml", f'<testsuite name="suite">{cases}</testsuite>')
        self.execute()
        self.assertIn("10 more", self.writes[0][2])
        self.assertLess(len(self.writes[0][2]), 20000)

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
