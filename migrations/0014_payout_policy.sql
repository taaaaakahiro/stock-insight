-- 有報・半期報告書から拾った配当性向の下限と目標の数字。D1（SQLite）
-- 比率は小数（0.3 = 30%）。書かれていなければ NULL。payout_phrase は数字を拾った文。
-- 従来の payout_target は下限（なければ目標）として残す
-- （2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE edinet_docs ADD COLUMN payout_floor REAL;
ALTER TABLE edinet_docs ADD COLUMN payout_goal REAL;
ALTER TABLE edinet_docs ADD COLUMN payout_phrase TEXT;
