INSERT INTO reit_scores (
  run_id,ticker,total_score,coverage_pct,rank_eligible,risks_json,explanations_json,summary_json,detail_json
)
SELECT ?1,?2->>'ticker',?2->>'total_score',?2->>'coverage_pct',?2->>'rank_eligible',
       ?2->>'risks',json_object('positives',?2->'positives','weaknesses',?2->'weaknesses'),?2,?3
ON CONFLICT(run_id,ticker) DO NOTHING
