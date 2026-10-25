-- 推薦入力・公開履歴。従来の recommendations は最新表示用の互換キャッシュ。
CREATE TABLE IF NOT EXISTS recommendation_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  legacy_key TEXT UNIQUE,
  price_date TEXT NOT NULL,
  batch_id TEXT,
  model TEXT NOT NULL,
  prompt_version TEXT,
  rule_version TEXT,
  app_version TEXT,
  request_json TEXT,
  response_json TEXT,
  status TEXT NOT NULL CHECK (status IN ('pending', 'published', 'failed', 'legacy_incomplete')),
  trigger_mode TEXT NOT NULL,
  is_primary INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  published_at INTEGER,
  error TEXT
);
CREATE INDEX IF NOT EXISTS recommendation_runs_history ON recommendation_runs(id DESC);
CREATE UNIQUE INDEX IF NOT EXISTS recommendation_primary_date ON recommendation_runs(price_date) WHERE is_primary = 1;
CREATE TABLE IF NOT EXISTS recommendation_candidates (
  run_id INTEGER NOT NULL REFERENCES recommendation_runs(id),
  code TEXT NOT NULL,
  rule_rank INTEGER,
  snapshot_json TEXT,
  rank INTEGER,
  reason TEXT,
  grade TEXT,
  PRIMARY KEY (run_id, code),
  UNIQUE (run_id, rank)
);
CREATE INDEX IF NOT EXISTS recommendation_candidates_code ON recommendation_candidates(code, run_id);
CREATE TABLE IF NOT EXISTS recommendation_tracking_meta (key TEXT PRIMARY KEY);
-- 以前の指標や公開時刻は推定せず、不明のまま移す。毎回 migrate しても二重に取り込まない。
INSERT OR IGNORE INTO recommendation_runs
  (legacy_key, price_date, model, status, trigger_mode, created_at)
SELECT 'legacy-' || date, date, MAX(model), 'legacy_incomplete', 'legacy', MAX(created_at)
FROM recommendations WHERE NOT EXISTS (SELECT 1 FROM recommendation_tracking_meta WHERE key='legacy_imported') GROUP BY date;
INSERT OR IGNORE INTO recommendation_candidates (run_id, code, rank, reason, grade)
SELECT h.id, r.code, r.rank, r.reason, r.grade FROM recommendations r
JOIN recommendation_runs h ON h.legacy_key = 'legacy-' || r.date
WHERE NOT EXISTS (SELECT 1 FROM recommendation_tracking_meta WHERE key='legacy_imported');
INSERT OR IGNORE INTO recommendation_tracking_meta(key) VALUES ('legacy_imported');
-- 元データの訂正も追記する。id 以下の最新観測を使えば過去の計算を再現できる。
CREATE TABLE IF NOT EXISTS evaluation_market_data (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  code TEXT NOT NULL,
  trade_date TEXT NOT NULL,
  close REAL NOT NULL CHECK (close > 0),
  split_factor REAL NOT NULL CHECK (split_factor > 0),
  observed_at INTEGER NOT NULL,
  UNIQUE(code, trade_date, observed_at, close, split_factor)
);
CREATE INDEX IF NOT EXISTS evaluation_market_lookup ON evaluation_market_data(code, trade_date, id DESC);
CREATE TABLE IF NOT EXISTS evaluation_batches (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at INTEGER NOT NULL,
  as_of TEXT NOT NULL,
  source_revision INTEGER NOT NULL,
  calendar_json TEXT NOT NULL,
  report_json TEXT,
  version TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'running'
);
CREATE TABLE IF NOT EXISTS recommendation_outcomes (
  run_id INTEGER NOT NULL REFERENCES recommendation_runs(id),
  code TEXT NOT NULL,
  horizon INTEGER NOT NULL,
  batch_id INTEGER NOT NULL REFERENCES evaluation_batches(id),
  result_json TEXT NOT NULL,
  PRIMARY KEY (run_id, code, horizon, batch_id)
);
CREATE INDEX IF NOT EXISTS recommendation_outcome_latest ON recommendation_outcomes(run_id, code, horizon, batch_id DESC);
-- 配当予想・財務・配当方針は既存テーブルの変更を観測して追記する。
-- observed_at は取得時刻。実際に情報が公開された日時は payload 内の disclosed/submitted。
CREATE TABLE IF NOT EXISTS recommendation_fundamentals (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  code TEXT NOT NULL,
  source TEXT NOT NULL,
  observed_at INTEGER NOT NULL,
  payload TEXT NOT NULL,
  UNIQUE(code, source, observed_at, payload)
);
CREATE INDEX IF NOT EXISTS recommendation_fundamentals_latest ON recommendation_fundamentals(code, source, id DESC);
