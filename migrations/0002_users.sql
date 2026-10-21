-- ユーザーとログイン記録（F1）。D1（SQLite）

-- Clerk のユーザーID で1行。allowed は Clerk に問い合わせた結果のキャッシュ（checked_at から1時間有効）
CREATE TABLE IF NOT EXISTS users (
  id           TEXT PRIMARY KEY,
  email        TEXT NOT NULL,
  display_name TEXT NOT NULL,
  allowed      INTEGER NOT NULL,
  is_admin     INTEGER NOT NULL,
  checked_at   INTEGER NOT NULL,
  created_at   INTEGER NOT NULL
);

-- ログイン記録。拒否された記録も残す（1年保持）
CREATE TABLE IF NOT EXISTS login_events (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id     TEXT NOT NULL,
  email       TEXT,
  occurred_at INTEGER NOT NULL,
  ip_address  TEXT,
  user_agent  TEXT,
  result      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_login_events_at ON login_events (occurred_at);
