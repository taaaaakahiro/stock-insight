-- 自動取得した終値の基準日。既存の手入力単価は維持する。
ALTER TABLE portfolio_items ADD COLUMN acquisition_price_date TEXT;
