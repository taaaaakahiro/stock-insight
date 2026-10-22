-- AIレコメンド（F7）。1日10行（順位ごと）。date は株価の日付（indicators_latest.price_date）。D1（SQLite）
CREATE TABLE IF NOT EXISTS recommendations (
  date       TEXT NOT NULL,
  rank       INTEGER NOT NULL,
  code       TEXT NOT NULL,
  reason     TEXT NOT NULL,
  model      TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (date, rank)
);
