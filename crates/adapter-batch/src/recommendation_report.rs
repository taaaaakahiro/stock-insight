//! 同じ推薦日のAIとルールを対にした比較。再生成版と未満期を勝率の母数に入れない。
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Group {
    model: String,
    prompt: String,
    rule: String,
    horizon: u64,
    total: usize,
    pending: usize,
    missing: usize,
    ai: Vec<f64>,
    baseline: Vec<f64>,
    diffs: Vec<f64>,
    codes: BTreeSet<String>,
    months: BTreeMap<String, Vec<f64>>,
}
fn mean(xs: &[f64]) -> Option<f64> {
    (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64)
}

/// cohorts は初回公開版のみ。summary は同じrunの同じ期限のAI・ルールの評価。
pub fn build(cohorts: &[Value]) -> Value {
    let mut groups = BTreeMap::<String, Group>::new();
    for cohort in cohorts {
        for pair in cohort["summary"].as_array().into_iter().flatten() {
            let horizon = pair["horizon"].as_u64().unwrap_or(0);
            if horizon == 0 {
                continue;
            }
            let key = json!([
                cohort["model"],
                cohort["prompt_version"],
                cohort["rule_version"],
                horizon
            ])
            .to_string();
            let group = groups.entry(key).or_insert_with(|| Group {
                model: cohort["model"].as_str().unwrap_or("unknown").into(),
                prompt: cohort["prompt_version"]
                    .as_str()
                    .unwrap_or("unknown")
                    .into(),
                rule: cohort["rule_version"].as_str().unwrap_or("unknown").into(),
                horizon,
                ..Group::default()
            });
            group.total += 1;
            let ai = &pair["ai"];
            let rule = &pair["rule"];
            if [ai, rule].iter().any(|r| {
                matches!(
                    r["status"].as_str(),
                    Some("data_missing" | "entry_unavailable")
                )
            }) {
                group.missing += 1;
                continue;
            }
            if ai["status"] != "complete" || rule["status"] != "complete" {
                group.pending += 1;
                continue;
            }
            let values = ai["price_return"]
                .as_f64()
                .zip(rule["price_return"].as_f64());
            let Some((a, b)) = values.filter(|(a, b)| a.is_finite() && b.is_finite()) else {
                group.missing += 1;
                continue;
            };
            if ai["entry_date"].is_null()
                || ai["end_date"].is_null()
                || ai["entry_date"] != rule["entry_date"]
                || ai["end_date"] != rule["end_date"]
            {
                group.missing += 1;
                continue;
            }
            group.ai.push(a);
            group.baseline.push(b);
            group.diffs.push(a - b);
            for code in cohort["selected_codes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                group.codes.insert(code.into());
            }
            let month = cohort["price_date"]
                .as_str()
                .and_then(|d| d.get(..7))
                .unwrap_or("unknown");
            group.months.entry(month.into()).or_default().push(a - b);
        }
    }
    let rows:Vec<_>=groups.into_values().map(|mut g| {
        g.diffs.sort_by(f64::total_cmp);
        let n=g.diffs.len();
        let median=if n==0 { None } else if n%2==1 { Some(g.diffs[n/2]) } else { Some((g.diffs[n/2-1]+g.diffs[n/2])/2.0) };
        let wins=g.diffs.iter().filter(|d|**d>1e-12).count();
        let ties=g.diffs.iter().filter(|d|d.abs()<=1e-12).count();
        let months:Vec<_>=g.months.into_iter().map(|(month,diffs)|json!({"month":month,"pairs":diffs.len(),"mean_excess":mean(&diffs)})).collect();
        json!({"model":g.model,"prompt_version":g.prompt,"rule_version":g.rule,"horizon":g.horizon,
            "total":g.total,"paired":n,"pending":g.pending,"missing":g.missing,
            "mean_ai":mean(&g.ai),"mean_rule":mean(&g.baseline),"mean_excess":mean(&g.diffs),
            "median_excess":median,"beat_rate":if n>0 {Some(wins as f64/n as f64)} else {None},
            "ties":ties,"unique_stocks":g.codes.len(),"months":months})
    }).collect();
    json!({"metric":"price_return","baseline":"rule_top10","groups":rows,
        "adoption":"manual_review","primary_horizon":126})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cohort(status: &str, a: Option<f64>, b: Option<f64>) -> Value {
        let result = |value| json!({"status":status,"entry_date":"2026-01-05","end_date":"2026-07-06","price_return":value});
        json!({"model":"m","prompt_version":"p","rule_version":"r","price_date":"2026-01-04","selected_codes":["7203"],
            "summary":[{"horizon":126,"ai":result(a),"rule":result(b)}]})
    }
    #[test]
    fn matched_means_count_pending_missing_ties_and_repeated_stocks() {
        let cohorts = [
            cohort("complete", Some(0.2), Some(0.1)),
            cohort("complete", Some(0.0), Some(0.0)),
            cohort("pending", None, None),
            cohort("data_missing", None, Some(0.0)),
        ];
        let report = build(&cohorts);
        let g = &report["groups"][0];
        assert_eq!(g["paired"], 2);
        assert_eq!(g["pending"], 1);
        assert_eq!(g["missing"], 1);
        assert_eq!(g["ties"], 1);
        assert_eq!(g["unique_stocks"], 1);
        assert_eq!(g["beat_rate"], 0.5);
        assert!((g["mean_excess"].as_f64().unwrap() - 0.05).abs() < 1e-12);
    }
    #[test]
    fn different_versions_and_unmatched_dates_are_not_pooled() {
        let a = cohort("complete", Some(0.2), Some(0.1));
        let mut b = a.clone();
        b["prompt_version"] = json!("p2");
        b["summary"][0]["rule"]["end_date"] = json!("2026-07-07");
        let r = build(&[a, b]);
        assert_eq!(r["groups"].as_array().unwrap().len(), 2);
        assert_eq!(r["groups"][1]["paired"], 0);
        assert_eq!(r["groups"][1]["missing"], 1);
        assert!(r["groups"][1]["beat_rate"].is_null());
    }
}
