//! Verified same-period monetary observations. No AI number is silently promoted.
use crate::ixbrl::{self, VerifiedInstantFact};
use domain::Date;
use serde_json::{json, Value};

/// Conflicting aliases or contexts are missing, not first-row-wins.
fn unique(facts: &[VerifiedInstantFact], names: &[&str]) -> Option<(f64, Vec<Value>)> {
    let found: Vec<_> = facts
        .iter()
        .filter(|f| names.contains(&f.name.as_str()))
        .collect();
    let value = found.first()?.value;
    if found.iter().any(|f| (f.value - value).abs() > 0.5) {
        return None;
    }
    Some((
        value,
        found
            .iter()
            .map(|f| json!({"tag":f.name,"context":f.context,"raw":f.raw,"location":f.location}))
            .collect(),
    ))
}

pub fn extract(documents: &[(String, String)], period: Date) -> Value {
    let facts = ixbrl::verified_instant_facts(documents, period);
    let mut observations = serde_json::Map::new();
    for (key, names) in [
        ("assets", vec!["Assets", "TotalAssets"]),
        ("liabilities", vec!["Liabilities", "TotalLiabilities"]),
        ("net_assets", vec!["NetAssets"]),
        ("short_loans", vec!["ShortTermLoansPayable"]),
        ("long_loans", vec!["LongTermLoansPayable"]),
        (
            "current_long_loans",
            vec![
                "CurrentPortionOfLongTermLoansPayable",
                "LongTermLoansRepayableWithinOneYearCL",
            ],
        ),
        ("bonds", vec!["InvestmentCorporationBondINV"]),
        (
            "current_bonds",
            vec!["CurrentPortionOfInvestmentCorporationBondsINV"],
        ),
    ] {
        if let Some((value, evidence)) = unique(&facts, &names) {
            if value >= 0.0 {
                observations.insert(key.into(),json!({"value":value,"unit":"JPY","period_end":period.to_string(),"evidence":evidence,"quality":"verified_context"}));
            }
        }
    }
    json!({"period_end":period.to_string(),"observations":observations})
}

pub fn evaluate(numbers: &Value, profile: &Value, as_of: Date) -> Value {
    let period = profile["period_end"].as_str().and_then(Date::parse);
    let mut out = json!({"ltv":null,"quality":"missing","reason":"総資産・負債の検証待ち","period_end":profile["period_end"],"evidence":[]});
    let Some(period) = period else {
        return out;
    };
    if numbers["period_end"] != period.to_string() {
        out["reason"] = json!("期末が一致しません");
        return out;
    }
    if as_of.to_days() - period.to_days() > 270 || period > as_of {
        out["quality"] = json!("stale");
        out["reason"] = json!("財務の基準日を確認してください");
        return out;
    }
    let obs = &numbers["observations"];
    let (Some(assets), Some(liabilities), Some(net_assets)) = (
        obs["assets"]["value"].as_f64(),
        obs["liabilities"]["value"].as_f64(),
        obs["net_assets"]["value"].as_f64(),
    ) else {
        return out;
    };
    if assets <= 0.0 || (assets - liabilities - net_assets).abs() > assets * 0.00001 {
        out["quality"] = json!("conflict");
        out["reason"] = json!("貸借対照表の合計が一致しません");
        return out;
    }
    // The legacy total may also omit a missing component. Require explicit facts
    // for every component, including zero, before reconciling each category.
    let mut total = 0.0;
    for (profile_key, keys) in [
        ("short_loans", vec!["short_loans"]),
        ("long_loans", vec!["long_loans", "current_long_loans"]),
        ("bonds", vec!["bonds", "current_bonds"]),
    ] {
        let Some(expected) = profile[profile_key]
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)
        else {
            return out;
        };
        let values: Vec<_> = keys
            .iter()
            .filter_map(|key| obs[key]["value"].as_f64())
            .collect();
        let actual: f64 = values.iter().sum();
        // Zero absent categories cannot be assumed to be zero without evidence.
        if values.len() != keys.len() || (actual - expected).abs() > expected.max(1.0) * 0.00001 {
            out["reason"] = json!("借入・債券残高を原資料と照合できません");
            return out;
        }
        total += actual;
    }
    if total > liabilities + assets * 0.00001 {
        out["quality"] = json!("conflict");
        out["reason"] = json!("有利子負債が負債合計を超えています");
        return out;
    }
    let short = profile["short_loans"].as_f64().unwrap_or(0.0);
    if total > 0.0 {
        if let Some(rows) = numbers["ladder"].as_array() {
            let mut sums = vec![];
            let mut firsts = vec![];
            for row in rows {
                if let Some(amounts) = row["amounts"].as_array().filter(|a| a.len() == 6) {
                    let values: Option<Vec<f64>> = amounts
                        .iter()
                        .map(|n| n.as_f64().filter(|v| *v >= 0.0 && v.is_finite()))
                        .collect();
                    if let Some(values) = values {
                        sums.push(values.iter().sum::<f64>());
                        firsts.push(values[0]);
                    }
                }
            }
            let sum: f64 = sums.iter().sum();
            let first: f64 = firsts.iter().sum();
            if !rows.is_empty() && sums.len() == rows.len() {
                let adjustment = if (sum - total).abs() <= total * 0.00001 {
                    Some(0.0)
                } else if (sum + short - total).abs() <= total * 0.00001
                    && !rows
                        .iter()
                        .any(|r| r["item"].as_str().is_some_and(|s| s.contains("短期")))
                {
                    Some(short)
                } else {
                    None
                };
                if let Some(add) = adjustment {
                    out["maturing_ratio"] = json!((first + add) / total);
                }
            }
        }
        let checks: Value = profile["checks"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(Value::Null);
        if matches!(checks["loans"].as_str(), Some("一致" | "端数")) {
            if let (Some(lo), Some(hi)) = (
                profile["reprice_1y"].as_f64(),
                profile["reprice_1y_max"].as_f64(),
            ) {
                if lo >= 0.0 && hi >= lo && hi <= total {
                    out["reprice_ratio"] = json!(lo / total);
                    out["reprice_max_ratio"] = json!(hi / total);
                }
            }
        }
    }
    let maturity = out["maturing_ratio"].as_f64();
    let repricing = out["reprice_ratio"]
        .as_f64()
        .zip(out["reprice_max_ratio"].as_f64());
    out["assessment_ready"] = json!(
        maturity.is_some()
            && repricing.is_some_and(|(lo, hi)| hi <= 0.2 || lo >= 0.4 || (lo > 0.2 && hi < 0.4))
    );
    out["positive_allowed"] = json!(
        total / assets <= 0.45
            && maturity.is_some_and(|m| m <= 0.2)
            && repricing.is_some_and(|(_, hi)| hi <= 0.2)
    );
    out["ltv"] = json!(total / assets);
    out["quality"] = json!("verified");
    out["reason"] = json!("同一期末の貸借対照表で検証済み");
    out["evidence"] = json!([obs["assets"], obs["liabilities"], obs["net_assets"]]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn numbers() -> Value {
        let mut v = json!({"period_end":"2026-06-30","observations":{}});
        for (k, x) in [
            ("assets", 1000.),
            ("liabilities", 500.),
            ("net_assets", 500.),
            ("short_loans", 0.),
            ("long_loans", 400.),
            ("current_long_loans", 0.),
            ("bonds", 50.),
            ("current_bonds", 0.),
        ] {
            v["observations"][k] = json!({"value":x});
        }
        v
    }
    fn profile() -> Value {
        json!({"period_end":"2026-06-30","short_loans":0.,"long_loans":400.,"bonds":50.})
    }
    #[test]
    fn reconcile_before_ltv() {
        let now = Date::parse("2026-10-26").unwrap();
        assert_eq!(evaluate(&numbers(), &profile(), now)["ltv"], 0.45);
        let mut bad = numbers();
        bad["observations"]["net_assets"]["value"] = json!(800.);
        assert!(evaluate(&bad, &profile(), now)["ltv"].is_null());
    }
    #[test]
    fn missing_zero_is_not_confirmed_zero() {
        let mut n = numbers();
        n["observations"]
            .as_object_mut()
            .unwrap()
            .remove("short_loans");
        assert!(evaluate(&n, &profile(), Date::parse("2026-10-26").unwrap())["ltv"].is_null());
    }
    #[test]
    fn missing_current_debt_component_is_not_confirmed_zero() {
        for key in ["current_long_loans", "current_bonds"] {
            let mut n = numbers();
            n["observations"].as_object_mut().unwrap().remove(key);
            let result = evaluate(&n, &profile(), Date::parse("2026-10-26").unwrap());
            assert!(result["ltv"].is_null(), "missing {key} must withhold LTV");
            assert_ne!(result["positive_allowed"], true);
        }
    }
    #[test]
    fn wrong_period_and_stale_are_not_evaluated() {
        let mut n = numbers();
        n["period_end"] = json!("2025-06-30");
        assert!(evaluate(&n, &profile(), Date::parse("2026-10-26").unwrap())["ltv"].is_null());
        assert_eq!(
            evaluate(&numbers(), &profile(), Date::parse("2027-10-26").unwrap())["quality"],
            "stale"
        );
    }
    #[test]
    fn unknown_repricing_does_not_allow_positive() {
        let mut n = numbers();
        n["ladder"] = json!([{"item":"長期借入金","amounts":[50,80,80,80,60,50]},{"item":"投資法人債","amounts":[0,0,0,0,0,50]}]);
        let mut p = profile();
        p["checks"] = json!("{\"loans\":\"一致\"}");
        p["reprice_1y"] = json!(50);
        p["reprice_1y_max"] = json!(50);
        let now = Date::parse("2026-10-26").unwrap();
        let v = evaluate(&n, &p, now);
        assert_eq!(v["assessment_ready"], true);
        assert_eq!(v["positive_allowed"], true);
        p["reprice_1y_max"] = json!(400);
        assert_eq!(evaluate(&n, &p, now)["assessment_ready"], false);
    }
}
