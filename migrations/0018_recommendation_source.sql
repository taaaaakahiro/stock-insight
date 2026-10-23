-- AIレコメンドの保存の元: cron（21:00 の Cron）/ manual（画面のボタンで作り直した分）。列を足す前の行は NULL。D1（SQLite）
-- 画面のボタンは日本時間で1日1回までなので、manual の created_at を数える
-- （2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE recommendations ADD COLUMN source TEXT;
