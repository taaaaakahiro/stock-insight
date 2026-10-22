-- 有報・半期報告書から拾った DOE（株主資本配当率）の下限と目標の数字。D1（SQLite）
-- 比率は小数（0.035 = 3.5%）。書かれていなければ NULL。doe_phrase は数字を拾った文
-- （2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE edinet_docs ADD COLUMN doe_floor REAL;
ALTER TABLE edinet_docs ADD COLUMN doe_goal REAL;
ALTER TABLE edinet_docs ADD COLUMN doe_phrase TEXT;
