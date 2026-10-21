"""マイグレーションと出力SQLを SQLite（D1と同じエンジン）に流し、結果を表示する。
使い方: python3 scripts/verify_sql.py target/poc_offline.sql
"""
import sqlite3, sys, pathlib

root = pathlib.Path(__file__).resolve().parent.parent
db = sqlite3.connect(":memory:")
db.executescript((root / "migrations/0001_init.sql").read_text())
db.executescript(pathlib.Path(sys.argv[1]).read_text())
for row in db.execute("SELECT run_id, target, succeeded, failed, status FROM batch_runs"):
    print("batch_runs:", row)
cols = "code, price, price_date, round(dividend_yield*100,2), pbr, per, round(payout_ratio*100,1), round(doe*100,2), consecutive_increases, no_loss_years, valuation, fin_doc_id"
for row in db.execute(f"SELECT {cols} FROM indicators_latest ORDER BY code"):
    print("indicators:", row)
print("financials_annual rows:", db.execute("SELECT count(*) FROM financials_annual").fetchone()[0])
