-- 銘柄マスタ（JPX「東証上場銘柄一覧」）。国内株式・REIT 等・ETF/ETN。D1（SQLite）

CREATE TABLE IF NOT EXISTS stocks (
  code        TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  -- stock / reit / etf（一覧のタブ）
  kind        TEXT NOT NULL,
  market      TEXT NOT NULL,
  sector33    TEXT,
  size_class  TEXT NOT NULL,
  as_of       TEXT NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_stocks_kind ON stocks (kind);
