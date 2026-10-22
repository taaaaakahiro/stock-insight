-- EDINET の有価証券報告書・半期報告書（銘柄・種類ごとに最新の1通だけ）と、前年同期比の伸び率。D1（SQLite）

-- 文章そのものは R2（r2_key）に置き、ここには配当方針の手がかりだけを持つ。
-- 新しい書類で上書きし、新しい有報で古くなった半期報告書と、上場していない銘柄の行は消す
CREATE TABLE IF NOT EXISTS edinet_docs (
  code                 TEXT NOT NULL,
  -- annual（有価証券報告書）/ semiannual（半期報告書）
  doc_kind             TEXT NOT NULL,
  doc_id               TEXT NOT NULL,
  submitted            TEXT NOT NULL,
  period_end           TEXT,
  r2_key               TEXT NOT NULL,
  progressive_dividend INTEGER NOT NULL,
  no_dividend_cut      INTEGER NOT NULL,
  dividend_raise       INTEGER NOT NULL,
  doe_target           INTEGER NOT NULL,
  -- 配当性向の目標（0.3 = 30%。範囲なら下限）
  payout_target        REAL,
  -- 手がかりの言葉を含む文（JSON の文字列配列）
  phrases              TEXT NOT NULL,
  fetched_at           INTEGER NOT NULL,
  PRIMARY KEY (code, doc_kind)
);

-- 売上高・営業利益の前年同期比（2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE indicators_latest ADD COLUMN sales_growth REAL;
ALTER TABLE indicators_latest ADD COLUMN op_growth REAL;
