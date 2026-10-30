ALTER TABLE edinet_docs ADD COLUMN financial_quality_json TEXT CHECK(financial_quality_json IS NULL OR json_valid(financial_quality_json));
