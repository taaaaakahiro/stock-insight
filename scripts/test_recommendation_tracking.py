"""Run the actual Worker SQL against SQLite; no network or production data.
Usage: python3 scripts/test_recommendation_tracking.py
"""
import json
from pathlib import Path
import re
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parent.parent
SOURCE = (ROOT / "crates/adapter-cf/src/d1_recommend.rs").read_text()


def sql(prefix, source=SOURCE):
    # Rust's backslash-newline continuation, used by the Worker SQL literals.
    literals = re.findall(r'"((?:[^"\\]|\\.)*)"', source.replace("\\\n", ""), re.S)
    return next(s for s in literals if s.startswith(prefix))


class TrackingSql(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        for path in sorted((ROOT / "migrations").glob("*.sql")):
            if path.name < "0021":
                self.db.executescript(path.read_text())
        self.db.execute("INSERT INTO recommendations(date,rank,code,reason,model,created_at) VALUES ('2026-10-22',1,'7203','旧理由','old',1)")
        self.migration = (ROOT / "migrations/0021_recommendation_tracking.sql").read_text()
        self.db.executescript(self.migration)

    def test_latest_assessment_comes_from_matching_published_snapshot(self):
        query_source = (ROOT / "crates/adapter-cf/src/query.rs").read_text().replace("\\\n", "")
        expr = re.search(r"\(SELECT json_extract\(c.snapshot_json, '\$\.recommendation_quality'\).*?LIMIT 1\)", query_source, re.S)[0]
        run = self.begin()
        self.db.execute("UPDATE recommendation_candidates SET snapshot_json=? WHERE run_id=?", (json.dumps({"recommendation_quality": {"total": 7}}), run))
        self.publish(run)
        self.begin(price=999)  # pending newer data must not change the published assessment
        query = f"SELECT {expr} FROM recommendations r WHERE r.date='2026-10-23'"
        self.assertEqual(json.loads(self.db.execute(query).fetchone()[0]), {"total": 7})
        self.db.execute("UPDATE recommendation_candidates SET snapshot_json='{}' WHERE run_id=?", (run,))
        self.assertIsNone(self.db.execute(query).fetchone()[0])

    def begin(self, price=100, date="2026-10-23"):
        with self.db:
            run = self.db.execute(sql("INSERT INTO recommendation_runs (price_date"),
                (date, "batch", "model", "prompt-v1", "rule-v1", "test", '{"input":1}', "manual", 2)).fetchone()[0]
            self.db.execute(sql("INSERT INTO recommendation_candidates (run_id,code,rule_rank,snapshot_json)"),
                (json.dumps([{"code": "7203", "rule_rank": 1, "stock": {"price": price}}]),))
        return run

    def publish(self, run):
        with self.db:
            self.db.execute(sql("UPDATE recommendation_candidates SET rank="), (1, "理由", "strong", run, "7203", run))
            self.db.execute(sql("UPDATE recommendation_runs SET status='published'"), ('{"picks":[]}', 3, run))
            self.db.execute(sql("DELETE FROM recommendations WHERE date="), (run,))
            self.db.execute(sql("INSERT INTO recommendations(date,rank,code,reason,model,created_at,grade,source)"), (run,))

    def test_legacy_name_uses_master_without_fabricating_historical_metrics(self):
        self.db.execute("INSERT INTO stocks(code,name,kind,market,size_class,as_of,updated_at) VALUES ('7203','トヨタ自動車','stock','Prime','TOPIX Large70','2026-10-25',1)")
        self.db.row_factory = sqlite3.Row
        query = sql("SELECT c.code,c.rule_rank,c.rank,c.reason,c.grade,c.snapshot_json,s.name AS current_name")
        row = self.db.execute(query, (1,)).fetchone()
        self.assertEqual(row["current_name"], "トヨタ自動車")
        self.assertIsNone(row["snapshot_json"])
        self.db.execute("DELETE FROM stocks WHERE code='7203'")
        row = self.db.execute(query, (1,)).fetchone()
        self.assertEqual(row["code"], "7203")
        self.assertIsNone(row["current_name"])

    def test_regeneration_keeps_input_and_first_publication(self):
        first = self.begin(100)
        self.publish(first)
        self.assertEqual(self.db.execute("SELECT grade,source FROM recommendations WHERE date='2026-10-23'").fetchone(), ("strong", "manual"))
        second = self.begin(110)
        self.publish(second)
        self.assertEqual(self.db.execute("SELECT id,is_primary FROM recommendation_runs WHERE status='published' ORDER BY id").fetchall(), [(first,1),(second,0)])
        self.assertEqual(self.db.execute("SELECT json_extract(snapshot_json,'$.price') FROM recommendation_candidates WHERE run_id IN (?,?) ORDER BY run_id", (first,second)).fetchall(), [(100,),(110,)])
        self.assertEqual(self.db.execute("SELECT count(*) FROM recommendations WHERE date='2026-10-23'").fetchone()[0], 1)

    def test_migration_does_not_reimport_cache_on_next_daily_run(self):
        first = self.begin()
        self.publish(first)
        self.db.executescript(self.migration)
        self.assertEqual(self.db.execute("SELECT count(*) FROM recommendation_runs WHERE status='legacy_incomplete'").fetchone()[0],1)
        self.assertEqual(self.db.execute("SELECT snapshot_json FROM recommendation_candidates WHERE run_id=1").fetchone()[0],None)

    def test_atomic_begin_rolls_back_invalid_candidates(self):
        count = self.db.execute("SELECT count(*) FROM recommendation_runs").fetchone()[0]
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                self.db.execute(sql("INSERT INTO recommendation_runs (price_date"),
                    ("2026-10-23", "batch", "model", "p", "r", "test", "{}", "manual", 2)).fetchone()
                self.db.execute(sql("INSERT INTO recommendation_candidates (run_id,code,rule_rank,snapshot_json)"),
                    ('[{"code":"7203"},{"code":"7203"}]',))
        self.assertEqual(self.db.execute("SELECT count(*) FROM recommendation_runs").fetchone()[0],count)

    def test_failed_attempt_does_not_replace_published_recommendation(self):
        first = self.begin()
        self.publish(first)
        failed = self.begin(200)
        self.db.execute(sql("UPDATE recommendation_runs SET status='failed'"), ("bad response", "bad json", failed))
        self.assertEqual(self.db.execute("SELECT count(*) FROM recommendation_runs WHERE status='published'").fetchone()[0],1)
        self.assertEqual(self.db.execute("SELECT reason FROM recommendations WHERE date='2026-10-23'").fetchone()[0],"理由")

    def test_history_only_exposes_complete_evaluations(self):
        run = self.begin()
        self.publish(run)
        for batch, status in [(1,"complete"),(2,"running")]:
            self.db.execute("INSERT INTO evaluation_batches(id,created_at,as_of,source_revision,calendar_json,version,status) VALUES (?,?,?,0,'[]','v1',?)",(batch,batch,"2026-10-26",status))
            self.db.execute("INSERT INTO recommendation_outcomes VALUES (?,?,0,?,?)",(run,"7203",batch,json.dumps({"price_return":batch})))
        rows=self.db.execute(sql("SELECT o.code,o.horizon,o.result_json,b.created_at AS evaluated_at"),(run,)).fetchall()
        self.assertEqual(len(rows),1)
        self.assertEqual(json.loads(rows[0][2])["price_return"],1)

    def test_fundamental_changes_are_append_only_and_timestamp_only_is_not_a_change(self):
        source = (ROOT / "crates/adapter-batch/src/recommendation_evaluation.rs").read_text()
        query = sql("INSERT OR IGNORE INTO recommendation_fundamentals(code,source,observed_at,payload)", source)
        for timestamp, payload in [(1, '{"dps":10}'), (2, '{"dps":10}'), (3, '{"dps":8}')]:
            self.db.execute(query, ("7203", "annual", timestamp, payload, "7203", "annual", payload))
        rows=self.db.execute("SELECT observed_at,payload FROM recommendation_fundamentals ORDER BY id").fetchall()
        self.assertEqual(rows, [(1, '{"dps":10}'), (3, '{"dps":8}')])

    def test_source_revision_keeps_corrected_price_and_retry_is_idempotent(self):
        query="INSERT OR IGNORE INTO evaluation_market_data(code,trade_date,close,split_factor,observed_at) VALUES (?,?,?,?,?)"
        for close, observed in [(100,1),(100,1),(101,2)]:
            self.db.execute(query,("7203","2026-10-26",close,1,observed))
        rows=self.db.execute("SELECT close FROM evaluation_market_data ORDER BY id").fetchall()
        self.assertEqual(rows,[(100,),(101,)])
        first=self.db.execute("SELECT min(id) FROM evaluation_market_data").fetchone()[0]
        self.assertEqual(self.db.execute("SELECT close FROM evaluation_market_data WHERE id<=? ORDER BY id DESC LIMIT 1",(first,)).fetchone()[0],100)


if __name__ == "__main__":
    unittest.main()
