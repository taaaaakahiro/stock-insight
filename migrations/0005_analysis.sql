-- 5年分の分析（四半期の損益・配当の継続・バリュー判定）。D1（SQLite）

-- 決算短信の四半期行（期首からの累計値）。四半期単独の損益は計算時に差分で出す
CREATE TABLE IF NOT EXISTS financials_quarterly (
  code             TEXT NOT NULL,
  fiscal_year_end  TEXT NOT NULL,
  period_type      TEXT NOT NULL,
  period_end       TEXT NOT NULL,
  doc_id           TEXT NOT NULL,
  disclosed        TEXT,
  net_sales        REAL,
  operating_income REAL,
  ordinary_income  REAL,
  net_income       REAL,
  fetched_at       INTEGER NOT NULL,
  PRIMARY KEY (code, fiscal_year_end, period_type)
);

-- 一覧のバッジ（2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE indicators_latest ADD COLUMN dividend_every_year INTEGER;
ALTER TABLE indicators_latest ADD COLUMN no_quarterly_loss INTEGER;
ALTER TABLE indicators_latest ADD COLUMN is_value INTEGER;
ALTER TABLE indicators_latest ADD COLUMN data_years INTEGER;
