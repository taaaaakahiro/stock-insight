-- 直近の四半期（単独）の業績の質。AI レコメンドで「増益が本物か」を見る。D1（SQLite）
-- op_growth_q / net_growth_q: 直近の四半期単独の営業利益・純利益の前年同期比（0.1 = 10% 増）
-- op_growth_ttm: 直近4四半期の営業利益の合計と、その前の4四半期の合計の比 − 1
-- profit_one_off: 1 = 利益が一過性の疑い（純利益の伸びが営業利益の伸びを 30pt 以上上回る、または直近の四半期は
--   増益なのに4四半期の合計では減益）、0 = 疑いなし、NULL = 判定できない（次の日次バッチで埋まる）
-- （2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE indicators_latest ADD COLUMN op_growth_q REAL;
ALTER TABLE indicators_latest ADD COLUMN net_growth_q REAL;
ALTER TABLE indicators_latest ADD COLUMN op_growth_ttm REAL;
ALTER TABLE indicators_latest ADD COLUMN profit_one_off INTEGER;
