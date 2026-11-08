UPDATE reit_scoring_runs AS r
SET status='ready', completed_at=?
WHERE run_id=? AND status='staging'
  AND universe_count=(SELECT count(*) FROM reit_scores s WHERE s.run_id=r.run_id)
  AND universe_count*?=(SELECT count(*) FROM reit_score_components c WHERE c.run_id=r.run_id)
