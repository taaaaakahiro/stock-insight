-- 株式分割・併合（J-Quants の日足の AdjFactor が 1 でない日）。D1（SQLite）
-- 指標の計算で、分割をまたぐ終値と1株当たりの値（配当・EPS・BPS）を最新の株数の基準にそろえるのに使う
CREATE TABLE IF NOT EXISTS splits (
  code       TEXT NOT NULL,
  -- 権利落ち日
  date       TEXT NOT NULL,
  -- AdjFactor（1:2 の分割なら 0.5）
  factor     REAL NOT NULL,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (code, date)
);
