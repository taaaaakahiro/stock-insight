"""Verify real SQLite migrations and the guarded publication statement."""
import json
import pathlib
import sqlite3
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class ReitScoringTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        for path in sorted((ROOT / "migrations").glob("*.sql")):
            self.db.executescript(path.read_text())
        statements = ROOT / "crates/adapter-batch/src/reit_scoring"
        self.publish = (statements / "publish.sql").read_text()
        self.insert_score = (statements / "insert_score.sql").read_text()
        self.insert_components = (statements / "insert_components.sql").read_text()
        self.db.execute(
            "INSERT INTO indicators_latest(code,run_id,fetched_at,price,price_date,dividend_ttm,valuation)"
            " VALUES('8951','daily',0,100000,'2026-11-06',5100,'neutral')"
        )

    def stage(self, run, count):
        self.db.execute(
            "INSERT INTO reit_scoring_runs VALUES (?, '2026-11-08', 'jreit-v1', ?,"
            " 'JPX', NULL, 'staging', 'fixture')",
            (run, count),
        )

    def score(self, run, ticker):
        self.db.execute(
            "INSERT INTO reit_scores VALUES(?, ?, NULL, 0, 0, '[]', '{}', '{}', '{}')",
            (run, ticker),
        )

    def component(self, run, ticker, key):
        self.db.execute(
            "INSERT INTO reit_score_components VALUES(?, ?, ?, NULL, NULL, 5, NULL, 'missing', 0, 'null')",
            (run, ticker, key),
        )

    def latest(self):
        return self.db.execute(
            "SELECT run_id FROM reit_scoring_runs WHERE status='ready'"
            " ORDER BY as_of_date DESC, completed_at DESC LIMIT 1"
        ).fetchone()[0]

    def test_incomplete_recomputation_cannot_replace_last_complete_run(self):
        self.stage("previous", 1)
        self.score("previous", "8951")
        self.component("previous", "8951", "ltv")
        self.db.execute(self.publish, (1, "previous", 1))
        self.stage("new", 2)
        self.score("new", "8951")
        self.component("new", "8951", "ltv")
        self.db.execute(self.publish, (2, "new", 1))
        self.assertEqual(self.latest(), "previous")
        self.score("new", "8952")
        self.db.execute(self.publish, (3, "new", 1))
        self.assertEqual(self.latest(), "previous")
        self.component("new", "8952", "ltv")
        self.db.execute(self.publish, (4, "new", 1))
        self.db.execute(self.publish, (5, "new", 1))  # retry cannot republish
        self.assertEqual(self.latest(), "new")
        self.assertEqual(self.db.execute(
            "SELECT completed_at FROM reit_scoring_runs WHERE run_id='new'"
        ).fetchone(), (4,))
        self.assertEqual(self.db.execute(
            "SELECT run_id,dividend_ttm FROM indicators_latest WHERE code='8951'"
        ).fetchone(), ("daily", 5100))

    def test_saved_detail_preserves_null_zero_and_sources_on_retry(self):
        self.stage("run", 1)
        summary = {
            "ticker": "8951", "total_score": None, "coverage_pct": 0,
            "rank_eligible": False, "risks": [], "positives": [], "weaknesses": [],
        }
        detail = {"components": [
            {"metric_key": "floating_ratio", "metric_value": 0.0, "weight": 6,
             "percentile": None, "weighted_points": None, "status": "insufficient_peers",
             "benchmark_size": 1, "source_ref": [{"source_document_id": "fixture"}]},
            {"metric_key": "ltv", "metric_value": None, "weight": 5,
             "percentile": None, "weighted_points": None, "status": "missing",
             "benchmark_size": 0, "source_ref": None},
        ]}
        for _ in range(2):
            self.db.execute(self.insert_score, {"1": "run", "2": json.dumps(summary), "3": json.dumps(detail)})
            self.db.execute(self.insert_components, ("run", "8951"))
        self.db.execute(self.publish, (1, "run", 2))
        self.assertEqual(self.latest(), "run")
        self.assertEqual(self.db.execute(
            "SELECT metric_key, metric_value, status, json_extract(source_ref, '$[0].source_document_id')"
            " FROM reit_score_components ORDER BY metric_key"
        ).fetchall(), [
            ("floating_ratio", 0.0, "insufficient_peers", "fixture"),
            ("ltv", None, "missing", None),
        ])
        self.assertEqual(self.db.execute(
            "SELECT count(*) FROM reit_scores WHERE run_id='run' AND ticker='0000'"
        ).fetchone(), (0,))


if __name__ == "__main__":
    unittest.main()
