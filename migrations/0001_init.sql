-- 技術検証（PoC）用の初期スキーマ。D1（SQLite）

CREATE TABLE IF NOT EXISTS batch_runs (
  run_id      TEXT PRIMARY KEY,
  started_at  INTEGER NOT NULL,
  finished_at INTEGER,
  target      INTEGER NOT NULL DEFAULT 0,
  succeeded   INTEGER NOT NULL DEFAULT 0,
  failed      INTEGER NOT NULL DEFAULT 0,
  status      TEXT NOT NULL
);

-- 一覧用の最新指標（1銘柄1行）。比率は小数（0.035 = 3.5%）
CREATE TABLE IF NOT EXISTS indicators_latest (
  code                  TEXT PRIMARY KEY,
  run_id                TEXT NOT NULL,
  fetched_at            INTEGER NOT NULL,
  price                 REAL NOT NULL,
  price_date            TEXT NOT NULL,
  dividend_ttm          REAL NOT NULL,
  dividend_yield        REAL,
  yield_percentile      REAL,
  pbr                   REAL,
  per                   REAL,
  payout_ratio          REAL,
  doe                   REAL,
  consecutive_increases INTEGER,
  operating_margin      REAL,
  no_loss_years         INTEGER,
  profit_uptrend        INTEGER,
  valuation             TEXT NOT NULL,
  fin_doc_id            TEXT,
  fin_submitted         TEXT
);

CREATE INDEX IF NOT EXISTS idx_ind_yield  ON indicators_latest (dividend_yield);
CREATE INDEX IF NOT EXISTS idx_ind_pbr    ON indicators_latest (pbr);
CREATE INDEX IF NOT EXISTS idx_ind_payout ON indicators_latest (payout_ratio);
CREATE INDEX IF NOT EXISTS idx_ind_val    ON indicators_latest (valuation);

CREATE TABLE IF NOT EXISTS financials_annual (
  code             TEXT NOT NULL,
  period_end       TEXT NOT NULL,
  doc_id           TEXT NOT NULL,
  net_sales        REAL,
  operating_income REAL,
  ordinary_income  REAL,
  net_income       REAL,
  equity           REAL,
  eps              REAL,
  bps              REAL,
  dps              REAL,
  operating_cf     REAL,
  fetched_at       INTEGER NOT NULL,
  PRIMARY KEY (code, period_end)
);
