-- ポートフォリオ（ユーザーごとの保有銘柄）。取得価格は持たない。D1（SQLite）
CREATE TABLE IF NOT EXISTS portfolio_items (
  user_id  TEXT NOT NULL,
  code     TEXT NOT NULL,
  added_at INTEGER NOT NULL,
  PRIMARY KEY (user_id, code)
);
