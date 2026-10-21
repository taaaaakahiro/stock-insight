-- J-Quants（無料プラン）への切り替え。D1（SQLite）

-- 株価の履歴。J-Quants の日足の終値（分割調整前）を1銘柄1行にまとめる: "YYYYMMDD,終値;YYYYMMDD,終値;..."
CREATE TABLE IF NOT EXISTS price_history (
  code       TEXT PRIMARY KEY,
  closes     TEXT NOT NULL,
  last_date  TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 取り込みの進み具合（どの日付まで取ったか）
CREATE TABLE IF NOT EXISTS sync_state (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- 年次財務の開示日（年間配当をいつから使えるか）。2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす
ALTER TABLE financials_annual ADD COLUMN disclosed TEXT;
