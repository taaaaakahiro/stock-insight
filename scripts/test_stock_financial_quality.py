"""Check actual recommendation evidence SQL against all migrations (offline)."""
import json
import re
import sqlite3
from pathlib import Path

root = Path(__file__).resolve().parent.parent
conn = sqlite3.connect(':memory:')
for path in sorted((root / 'migrations').glob('*.sql')):
    conn.executescript(path.read_text())
source = (root / 'crates/adapter-cf/src/query.rs').read_text()
expr = re.search(r'\(SELECT e\.financial_quality_json .*?LIMIT 1\)', source)[0]
conn.execute("INSERT INTO stocks(code,name,kind,market,size_class,as_of,updated_at) VALUES('1234','test','stock','プライム','large','2026-10-29',0)")
conn.execute("INSERT INTO indicators_latest(code,run_id,price,price_date,dividend_ttm,valuation,fetched_at) VALUES('1234','test',100,'2026-10-29',1,'cheap',0)")
conn.execute("INSERT INTO edinet_docs(code,doc_kind,doc_id,submitted,period_end,r2_key,progressive_dividend,no_dividend_cut,dividend_raise,doe_target,phrases,fetched_at,financial_quality_json) VALUES('1234','annual','S1','2026-06-20','2026-03-31','test',0,0,0,0,'[]',0,?)", (json.dumps({'cash': 100, 'debt': None}),))
query = f'SELECT {expr} FROM indicators_latest i'
assert json.loads(conn.execute(query).fetchone()[0])['debt'] is None
for field, value in [('submitted', '2026-10-30'), ('period_end', '2024-03-31'), ('period_end', '2027-03-31')]:
    conn.execute('SAVEPOINT test')
    conn.execute(f'UPDATE edinet_docs SET {field}=?', (value,))
    assert conn.execute(query).fetchone()[0] is None
    conn.execute('ROLLBACK TO test')
    conn.execute('RELEASE test')
conn.execute('UPDATE edinet_docs SET financial_quality_json=NULL')
assert conn.execute(query).fetchone()[0] is None
print('Stock financial quality SQL checks passed')
