-- REIT の有利子負債・物件（EDINET の有報の表から読む。docs/09_reit_debt.md）。D1（SQLite）
-- 金額は円、面積は㎡。金利の内訳の5つと bonds を足すと有利子負債の合計になる

-- 最新の有報の要約（1銘柄1行）。一覧の REIT のタブで使う
CREATE TABLE IF NOT EXISTS reit_profile (
  code              TEXT PRIMARY KEY,
  doc_id            TEXT NOT NULL,
  submitted         TEXT NOT NULL,
  period_end        TEXT NOT NULL,
  -- office / logistics / residential / hotel / retail / healthcare / diversified / infra（data/reit_sectors.csv）
  sector            TEXT,
  short_loans       INTEGER NOT NULL,
  long_loans        INTEGER NOT NULL,
  bonds             INTEGER NOT NULL,
  rate_fixed        INTEGER NOT NULL,
  rate_swap_fixed   INTEGER NOT NULL,
  rate_swap_amount  INTEGER NOT NULL,
  rate_floating     INTEGER NOT NULL,
  rate_unknown      INTEGER NOT NULL,
  swap_notional     INTEGER,
  -- 残高加重の平均利率（%）と平均残存年数
  avg_rate          REAL,
  avg_years         REAL,
  -- 1年以内に金利が見直される額の下限と上限（記載なしを全部変動とみなす）
  reprice_1y        INTEGER NOT NULL,
  reprice_1y_max    INTEGER NOT NULL,
  property_count    INTEGER NOT NULL,
  leasable_m2       REAL,
  leased_m2         REAL,
  -- 面積加重の稼働率（%）
  occupancy         REAL,
  -- 読み取りの確認（JSON: {"loans":"一致|端数|ずれ|明細なし","properties":"一致|端数|ずれ|合計行なし|物件なし"}）
  checks            TEXT NOT NULL,
  fetched_at        INTEGER NOT NULL
);

-- 最新の有報の明細（1銘柄1行の JSON: 借入・返済予定・注記の手がかり・物件）。詳細の画面で1行だけ読む
CREATE TABLE IF NOT EXISTS reit_detail (
  code        TEXT PRIMARY KEY,
  period_end  TEXT NOT NULL,
  body        TEXT NOT NULL,
  fetched_at  INTEGER NOT NULL
);

-- 決算ごとの要約（推移のグラフ用。1銘柄1期1行）
CREATE TABLE IF NOT EXISTS reit_history (
  code            TEXT NOT NULL,
  period_end      TEXT NOT NULL,
  doc_id          TEXT NOT NULL,
  loans_total     INTEGER NOT NULL,
  bonds           INTEGER NOT NULL,
  rate_floating   INTEGER NOT NULL,
  rate_unknown    INTEGER NOT NULL,
  swap_notional   INTEGER,
  avg_rate        REAL,
  property_count  INTEGER NOT NULL,
  leasable_m2     REAL,
  occupancy       REAL,
  fetched_at      INTEGER NOT NULL,
  PRIMARY KEY (code, period_end)
);
