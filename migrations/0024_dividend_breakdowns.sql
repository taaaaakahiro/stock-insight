-- EDINETの配当政策から機械的に確認できた当期の内訳。未抽出をゼロ扱いしない。
ALTER TABLE edinet_docs ADD COLUMN dividend_total REAL;
ALTER TABLE edinet_docs ADD COLUMN dividend_special REAL;
ALTER TABLE edinet_docs ADD COLUMN dividend_commemorative REAL;
ALTER TABLE edinet_docs ADD COLUMN dividend_phrase TEXT;
ALTER TABLE edinet_docs ADD COLUMN dividend_extraction_version INTEGER;
ALTER TABLE indicators_latest ADD COLUMN dividend_total REAL;
ALTER TABLE indicators_latest ADD COLUMN dividend_special REAL;
ALTER TABLE indicators_latest ADD COLUMN dividend_commemorative REAL;
ALTER TABLE indicators_latest ADD COLUMN dividend_source TEXT;
