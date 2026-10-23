-- AIレコメンドの3段階の評価（strong = 強く推奨 / recommend = 推奨 / watch = 注目）。D1（SQLite）
-- 評価を付ける前に保存した日の行は NULL
-- （2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE recommendations ADD COLUMN grade TEXT;
