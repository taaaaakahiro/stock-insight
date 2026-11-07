-- REITの分配予想は1決算期単位。同じ開示にあるもう1期と、年換算・改訂順に使う情報を保存する。
ALTER TABLE dividend_forecasts ADD COLUMN reit_period_start TEXT;
ALTER TABLE dividend_forecasts ADD COLUMN reit_other_period_start TEXT;
ALTER TABLE dividend_forecasts ADD COLUMN reit_other_period_end TEXT;
ALTER TABLE dividend_forecasts ADD COLUMN reit_other_dps REAL;
ALTER TABLE dividend_forecasts ADD COLUMN reit_disclosed_time TEXT;
