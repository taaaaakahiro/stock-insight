-- 減配（直近の実績が前期より少ない、または会社予想が直近の実績より少ない）。ポートフォリオの参考の売りサインに使う。D1（SQLite）
-- 1 = 減配、0 = 減配でない、NULL = 比べる配当がない（次の日次バッチで埋まる）
-- （2回目以降は「列がすでにある」で失敗するが、migrate が読み飛ばす）
ALTER TABLE indicators_latest ADD COLUMN dividend_cut INTEGER;
