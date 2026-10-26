"""SQLite contract checks for staged evidence and all-or-nothing RAG publication."""
import json
import sqlite3
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

class RagStorageTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        self.db.executescript((ROOT / "migrations/0022_reit_rag.sql").read_text())
        self.db.executescript((ROOT / "migrations/0023_reit_rag_facts.sql").read_text())

    def run_row(self, ident, cutoff, status):
        self.db.execute("INSERT INTO reit_rag_runs(id,cutoff,price_date,model,version,input_hash,input_key,status) VALUES(?,?,'2026-10-26','test','v1',?,'private/input',?)", (ident, cutoff, ident, status))

    def test_failed_attempt_does_not_replace_published_snapshot(self):
        self.run_row("old", 100, "ready")
        self.run_row("new", 200, "failed")
        self.db.execute("INSERT INTO reit_rag_publication VALUES(1,'old',100)")
        self.db.execute("INSERT INTO reit_rag_results VALUES('old','8951','stock',?)", (json.dumps({"version": "old"}),))
        self.db.execute("INSERT INTO reit_rag_results VALUES('new','8951','stock',?)", (json.dumps({"version": "partial"}),))
        body = self.db.execute("SELECT body FROM reit_rag_results r JOIN reit_rag_publication p ON p.run_id=r.run_id WHERE p.singleton=1").fetchone()[0]
        self.assertEqual(json.loads(body)["version"], "old")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO reit_rag_results VALUES('old','8951','stock','{}')")

    def test_older_run_cannot_roll_back_publication(self):
        self.run_row("old",100,"ready")
        self.run_row("new",200,"ready")
        self.db.execute("INSERT INTO reit_rag_publication VALUES(1,'new',200)")
        self.db.execute("INSERT INTO reit_rag_publication VALUES(1,'old',100) ON CONFLICT(singleton) DO UPDATE SET run_id=excluded.run_id,cutoff=excluded.cutoff WHERE excluded.cutoff>reit_rag_publication.cutoff")
        self.assertEqual(self.db.execute("SELECT run_id FROM reit_rag_publication").fetchone()[0], "new")

    def test_fts_document_scope_prevents_other_issuer_evidence(self):
        for ident, code in [("a", "8951"),("b","8952")]:
            self.db.execute("INSERT INTO reit_rag_documents VALUES(?,?,?,'2026-06-30','2026-08-01',100,'hash','private/raw','https://example.invalid','v1','ready')",(ident,code,ident))
            self.db.execute("INSERT INTO reit_rag_chunks VALUES(?,?,0,'table:1','固定金利比率','固定 定金 金利 利比 比率')",(ident,ident))
            self.db.execute("INSERT INTO reit_rag_fts VALUES(?,'固定 定金 金利 利比 比率')",(ident,))
        rows=self.db.execute("SELECT c.id FROM reit_rag_fts JOIN reit_rag_chunks c ON c.id=reit_rag_fts.chunk_id WHERE reit_rag_fts MATCH ? AND c.document_id=? ORDER BY bm25(reit_rag_fts) LIMIT 20", ('"固定" OR "金利"', "a")).fetchall()
        self.assertEqual(rows, [("a",)])

    def test_migration_is_repeatable(self):
        self.db.executescript((ROOT / "migrations/0022_reit_rag.sql").read_text())
        self.db.executescript((ROOT / "migrations/0023_reit_rag_facts.sql").read_text())

if __name__ == "__main__":
    unittest.main()
