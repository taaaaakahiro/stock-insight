-- 注目テーマ（管理者が登録）と、有報の「事業の内容」との照合。D1（SQLite）

-- 「事業の内容」の文（先頭3000文字。項目がなければ空文字。この列を足す前に取り込んだ書類は NULL で、
-- sync-edinet --refetch-business で埋める）（2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE edinet_docs ADD COLUMN business_text TEXT;

-- テーマ。words は照合に使う言葉（JSON の文字列配列）
CREATE TABLE IF NOT EXISTS themes (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL UNIQUE,
  words      TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 照合の結果（テーマを保存するたびに作り直す）。matched_word は当たった言葉
CREATE TABLE IF NOT EXISTS stock_themes (
  theme_id     INTEGER NOT NULL,
  code         TEXT NOT NULL,
  matched_word TEXT NOT NULL,
  PRIMARY KEY (theme_id, code)
);
CREATE INDEX IF NOT EXISTS idx_stock_themes_code ON stock_themes (code);
