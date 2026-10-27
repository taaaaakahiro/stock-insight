import copy
import unittest
from unittest.mock import Mock
from reit_workflow_gate import select_run

REPO = "taaaaakahiro/stock-insight"


class ReitGateTest(unittest.TestCase):
    def setUp(self):
        self.event = {"workflow_run": {"name": "daily-batch", "conclusion": "success",
                      "head_branch": "main", "head_repository": {"full_name": REPO},
                      "id": 123, "run_attempt": 2}}
        self.jobs = Mock(return_value=[{"name": "jquants", "conclusion": "success"}])

    def test_successful_daily_starts_separate_full_run(self):
        self.assertEqual(select_run("workflow_run", self.event, REPO, True, self.jobs), (True, "full", ""))
        self.jobs.assert_called_once_with(123, 2)

    def test_failed_cancelled_or_untrusted_sources_do_not_start(self):
        for key, value in [("conclusion", "failure"), ("conclusion", "cancelled"),
                           ("head_branch", "feature"), ("name", "other"),
                           ("head_repository", {"full_name": "fork/repo"})]:
            event = copy.deepcopy(self.event)
            event["workflow_run"][key] = value
            self.assertFalse(select_run("workflow_run", event, REPO, True, self.jobs)[0])
        self.jobs.assert_not_called()

    def test_noop_and_probe_do_not_start(self):
        for jobs in [[], [{"name": "jquants", "conclusion": "skipped"}],
                     [{"name": "probe", "conclusion": "success"}]]:
            self.assertFalse(select_run("workflow_run", self.event, REPO, True, lambda *_: jobs)[0])

    def test_manual_analysis_and_resume_do_not_read_daily_run(self):
        for mode in ["full", "analyze", "resume", "diagnose", "ai-diagnose"]:
            run_id = "1793061956-snapshot" if mode == "resume" else ""
            event = {"inputs": {"mode": mode, "rag_run_id": run_id}}
            self.assertEqual(select_run("workflow_dispatch", event, REPO, True, self.jobs), (True, mode, run_id))
        self.jobs.assert_not_called()

    def test_disabled_ai_still_allows_reit_fetch_and_diagnostics(self):
        for mode in ["full", "diagnose", "ai-diagnose"]:
            self.assertTrue(select_run("workflow_dispatch", {"inputs": {"mode": mode}}, REPO, False, self.jobs)[0])
        with self.assertRaises(ValueError):
            select_run("workflow_dispatch", {"inputs": {"mode": "analyze"}}, REPO, False, self.jobs)

    def test_invalid_resume_and_mode_are_rejected(self):
        cases = [{"mode": "unknown"}, {"mode": "full", "rag_run_id": "run"},
                 {"mode": "resume", "rag_run_id": ""},
                 {"mode": "resume", "rag_run_id": "run\nmode=full"},
                 {"mode": "resume", "rag_run_id": "../other"}]
        for inputs in cases:
            with self.assertRaises(ValueError):
                select_run("workflow_dispatch", {"inputs": inputs}, REPO, True, self.jobs)
        self.jobs.assert_not_called()


if __name__ == "__main__":
    unittest.main()
