-- 会社予想の配当。配当利回りは、その時点でいちばん新しい開示の予想で出し、予想がなければ実績で出す。D1（SQLite）

-- 開示1件ごとの年間配当予想（1株、REIT は1口）。決算短信は予想がなくても行を作り（dps = NULL は「予想なし」）、
-- 配当予想の修正は予想がある行だけを入れる
CREATE TABLE IF NOT EXISTS dividend_forecasts (
  code            TEXT NOT NULL,
  doc_id          TEXT NOT NULL,
  disclosed       TEXT NOT NULL,
  -- 予想の対象の事業年度の終了日（分からなければ NULL）
  fiscal_year_end TEXT,
  dps             REAL,
  fetched_at      INTEGER NOT NULL,
  PRIMARY KEY (code, doc_id)
);

-- 利回りに使った配当が予想（forecast）か実績（actual）か。配当がなければ NULL
-- （2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE indicators_latest ADD COLUMN dividend_basis TEXT;
