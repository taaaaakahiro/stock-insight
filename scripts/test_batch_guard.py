"""Exercise the daily guard's actual SQL with interrupted reruns."""
from pathlib import Path
import re
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parent.parent
SOURCE = (ROOT / "crates/adapter-batch/src/d1.rs").read_text()
QUERY = re.search(r'sql: "(SELECT COUNT\(\*\) AS n FROM \(SELECT status[^"\n]+)"', SOURCE)[1]


class BatchGuard(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.executescript((ROOT / "migrations/0001_init.sql").read_text())

    def add(self, name, time, status):
        self.db.execute("INSERT INTO batch_runs(run_id,started_at,status) VALUES(?,?,?)", (name, time, status))

    def succeeded(self):
        return bool(self.db.execute(QUERY, ("daily-%", 100)).fetchone()[0])

    def test_latest_attempt_controls_skip_even_after_success(self):
        self.assertFalse(self.succeeded())
        self.add("daily-old", 99, "succeeded")
        self.assertFalse(self.succeeded())
        self.add("daily-ok", 101, "succeeded")
        self.assertTrue(self.succeeded())
        self.add("daily-interrupted", 102, "running")
        self.assertFalse(self.succeeded())
        self.db.execute("UPDATE batch_runs SET status='failed' WHERE run_id='daily-interrupted'")
        self.assertFalse(self.succeeded())
        self.add("daily-recovered", 103, "succeeded")
        self.assertTrue(self.succeeded())
        self.add("other-failed", 104, "failed")
        self.assertTrue(self.succeeded())


if __name__ == "__main__":
    unittest.main()
