"""EDINET/J-Quants年度・金額照合、オンライン/バッチSQLの一致を確認する。"""
import pathlib
import re
import sqlite3

root = pathlib.Path(__file__).resolve().parent.parent
c = sqlite3.connect(':memory:')
c.row_factory = sqlite3.Row
for p in sorted((root / 'migrations').glob('*.sql')):
    c.executescript(p.read_text())
c.execute("INSERT INTO stocks (code,name,kind,market,size_class,as_of,updated_at) VALUES ('4716','test','stock','プライム','large','2026-10-29',0)")
c.execute("INSERT INTO financials_annual (code,period_end,doc_id,disclosed,dps,fetched_at) VALUES ('4716','2026-05-31','JQ1','2026-06-25',858,0)")
c.execute("INSERT INTO dividend_forecasts VALUES ('4716','JQ2','2026-09-25','2027-05-31',900,0)")
c.execute("""INSERT INTO edinet_docs (code,doc_kind,doc_id,submitted,period_end,r2_key,progressive_dividend,no_dividend_cut,dividend_raise,doe_target,phrases,fetched_at,dividend_total,dividend_special,dividend_commemorative,dividend_extraction_version)
VALUES ('4716','annual','S100TEST','2026-08-21','2026-05-31','test',0,0,0,0,'[]',0,858,660,0,1)""")
source = (root / 'crates/adapter-cf/src/query.rs').read_text()
actual = re.search(r'pub const ANNUALS_BY_CODE: &str =\s*"(.*?)";', source, re.S)[1]
forecast = re.search(r'pub const FORECASTS_BY_CODE: &str =\s*"(.*?)";', source, re.S)[1]
row = c.execute(actual, ('4716',)).fetchone()
assert row['breakdown_special'] == 660
assert row['breakdown_known_since'] == '2026-08-21'
assert row['breakdown_source'] == 'https://disclosure2dl.edinet-fsa.go.jp/searchdocument/pdf/S100TEST.pdf'
assert c.execute(forecast, ('4716',)).fetchone()['breakdown_special'] is None
# Do not apply an annual report's actual breakdown to forecasts or another year/amount.
for column, value in [('period_end','2027-05-31'), ('dps',900)]:
    c.execute('SAVEPOINT test')
    c.execute(f'UPDATE financials_annual SET {column}=?',(value,))
    assert c.execute(actual,('4716',)).fetchone()['breakdown_special'] is None
    c.execute('ROLLBACK TO test')
    c.execute('RELEASE test')
batch = (root / 'crates/adapter-batch/src/d1.rs').read_text()
query_batch = re.search(r'pub async fn load_annuals.*?sql: "(.*?)"', batch, re.S)[1]
assert dict(c.execute(query_batch,(0,)).fetchone()) == dict(row)
c.execute('UPDATE edinet_docs SET dividend_total=NULL,dividend_special=NULL,dividend_commemorative=NULL')
assert c.execute(actual,('4716',)).fetchone()['breakdown_special'] is None
# Unverified rows still have the J-Quants total; absence is never a zero deduction.
assert c.execute(actual,('4716',)).fetchone()['dps'] == 858
c.execute("UPDATE stocks SET kind='reit'")
assert c.execute(forecast,('4716',)).fetchone()['breakdown_special'] == 0
print('EDINET dividend database checks passed')
