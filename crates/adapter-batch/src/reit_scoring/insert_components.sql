INSERT INTO reit_score_components (
  run_id,ticker,metric_key,metric_value,percentile,weight,weighted_points,status,benchmark_size,source_ref
)
SELECT run_id,ticker,value->>'metric_key',value->>'metric_value',value->>'percentile',value->>'weight',
       value->>'weighted_points',value->>'status',value->>'benchmark_size',value->'source_ref'
FROM reit_scores,json_each(detail_json,'$.components')
WHERE run_id=? AND ticker=?
ON CONFLICT(run_id,ticker,metric_key) DO NOTHING
