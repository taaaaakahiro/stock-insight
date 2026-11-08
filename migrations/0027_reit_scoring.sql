-- J-REIT quantitative scoring. Staging is invisible until a complete run is marked ready.
CREATE TABLE IF NOT EXISTS reit_metric_observations (
  ticker TEXT NOT NULL,
  metric_key TEXT NOT NULL,
  metric_value REAL,
  unit TEXT NOT NULL,
  period_start TEXT,
  period_end TEXT NOT NULL,
  fiscal_period_end TEXT NOT NULL,
  as_of_date TEXT NOT NULL,
  source_url TEXT NOT NULL,
  source_document_id TEXT,
  source_page INTEGER,
  disclosure_published_at TEXT NOT NULL,
  retrieved_at INTEGER NOT NULL,
  data_kind TEXT NOT NULL CHECK(data_kind IN ('actual','forecast','derived','estimated')),
  method_version TEXT NOT NULL,
  normalization_flags TEXT NOT NULL,
  quality_flags TEXT NOT NULL,
  revision_id TEXT NOT NULL,
  PRIMARY KEY(ticker, metric_key, fiscal_period_end, revision_id)
);
CREATE INDEX IF NOT EXISTS reit_metric_cutoff ON reit_metric_observations(ticker, disclosure_published_at);

CREATE TABLE IF NOT EXISTS reit_scoring_runs (
  run_id TEXT PRIMARY KEY,
  as_of_date TEXT NOT NULL,
  score_version TEXT NOT NULL,
  universe_count INTEGER NOT NULL CHECK(universe_count > 0),
  universe_definition TEXT NOT NULL,
  completed_at INTEGER,
  status TEXT NOT NULL CHECK(status IN ('staging','ready')),
  source_watermark TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS reit_scoring_latest ON reit_scoring_runs(score_version,status,as_of_date DESC,completed_at DESC,run_id DESC);

CREATE TABLE IF NOT EXISTS reit_scores (
  run_id TEXT NOT NULL REFERENCES reit_scoring_runs(run_id),
  ticker TEXT NOT NULL,
  total_score REAL,
  coverage_pct INTEGER NOT NULL,
  rank_eligible INTEGER NOT NULL,
  risks_json TEXT NOT NULL,
  explanations_json TEXT NOT NULL,
  summary_json TEXT NOT NULL,
  detail_json TEXT NOT NULL,
  PRIMARY KEY(run_id,ticker)
);

CREATE TABLE IF NOT EXISTS reit_score_components (
  run_id TEXT NOT NULL,
  ticker TEXT NOT NULL,
  metric_key TEXT NOT NULL,
  metric_value REAL,
  percentile REAL,
  weight INTEGER NOT NULL,
  weighted_points REAL,
  status TEXT NOT NULL,
  benchmark_size INTEGER NOT NULL,
  source_ref TEXT NOT NULL,
  PRIMARY KEY(run_id,ticker,metric_key),
  FOREIGN KEY(run_id,ticker) REFERENCES reit_scores(run_id,ticker)
);
