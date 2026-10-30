//! Bounded retrieval from the same annual EDINET CSV. No cross-period/scope joins.
use crate::edinet::XbrlRow;
use domain::disclosure::{normalize_text, Section};
use serde_json::{json, Value};
use usecase::edinet::DocRef;

pub fn extract(rows: &[XbrlRow], doc: &DocRef) -> Value {
    // Prefer consolidated statements. Never fill their gaps with parent-only facts.
    let scope = if rows
        .iter()
        .any(|r| r.scope == "連結" && r.context == "CurrentYearInstant")
    {
        "連結"
    } else {
        "個別"
    };
    let instant = if scope == "連結" {
        "CurrentYearInstant"
    } else {
        "CurrentYearInstant_NonConsolidatedMember"
    };
    let duration = if scope == "連結" {
        "CurrentYearDuration"
    } else {
        "CurrentYearDuration_NonConsolidatedMember"
    };
    let mut sources = Vec::new();
    let mut get = |names: &[&str], context: &str| -> Option<f64> {
        let matching: Vec<_> = rows
            .iter()
            .filter(|r| {
                r.scope == scope
                    && r.context == context
                    && r.unit == "円"
                    && names.contains(&r.element.as_str())
            })
            .collect();
        if matching.is_empty() {
            return None;
        }
        let mut value = None;
        for r in &matching {
            let v = r
                .value
                .trim()
                .replace(',', "")
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())?;
            if value.is_some_and(|old: f64| old != v) {
                return None;
            }
            value = Some(v);
        }
        for r in matching {
            sources
                .push(json!({"element":r.element,"context":r.context,"unit":r.unit,"raw":r.value}));
        }
        value
    };
    let cash = get(
        &[
            "jppfs_cor:CashAndCashEquivalents",
            "jpigp_cor:CashAndCashEquivalentsIFRS",
        ],
        instant,
    );
    let assets = get(&["jppfs_cor:Assets", "jpigp_cor:AssetsIFRS"], instant);
    let liabilities = get(
        &["jppfs_cor:Liabilities", "jpigp_cor:LiabilitiesIFRS"],
        instant,
    );
    // Equity excludes non-controlling interests; do not substitute net assets.
    let equity = get(
        &["jpigp_cor:EquityAttributableToOwnersOfParentIFRS"],
        instant,
    )
    .or_else(|| {
        let net = get(&["jppfs_cor:NetAssets"], instant)?;
        let minority = get(&["jppfs_cor:NonControllingInterests"], instant)?;
        let rights = get(&["jppfs_cor:SubscriptionRightsToShares"], instant)?;
        Some(net - minority - rights)
    });
    // Only an explicit aggregate is accepted. Summing an incomplete list of loans
    // and bonds would silently turn missing commercial paper / leases into zero.
    // Issuer extension taxonomies often provide the aggregate. Require both an
    // explicit total label and an unambiguous aggregate concept name. Do not sum
    // selected debt accounts: an omitted convertible bond could understate debt.
    let debt_names: Vec<&str> = rows
        .iter()
        .filter(|r| {
            matches!(
                r.label.as_str(),
                "有利子負債" | "有利子負債合計" | "有利子負債残高"
            ) && matches!(
                r.element.rsplit(':').next(),
                Some(
                    "InterestBearingDebt"
                        | "InterestBearingLiabilities"
                        | "InterestBearingDebtTotal"
                        | "InterestBearingLiabilitiesIFRS"
                )
            )
        })
        .map(|r| r.element.as_str())
        .collect();
    let debt = get(&debt_names, instant);
    let operating_cf = get(
        &[
            "jppfs_cor:NetCashProvidedByUsedInOperatingActivities",
            "jpigp_cor:CashFlowsFromUsedInOperatingActivitiesIFRS",
        ],
        duration,
    );
    let cf_names = [
        "jppfs_cor:NetCashProvidedByUsedInOperatingActivities",
        "jpigp_cor:CashFlowsFromUsedInOperatingActivitiesIFRS",
        "jpcrp_cor:NetCashProvidedByUsedInOperatingActivitiesSummaryOfBusinessResults",
    ];
    let cf_history: Vec<Option<f64>> = [
        "CurrentYearDuration",
        "Prior1YearDuration",
        "Prior2YearDuration",
    ]
    .iter()
    .map(|context| {
        let context = if scope == "連結" {
            context.to_string()
        } else {
            format!("{context}_NonConsolidatedMember")
        };
        get(&cf_names, &context)
    })
    .collect();
    let positive_operating_cf_3y = cf_history
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()
        .map(|values| values.iter().all(|x| *x > 0.0));
    let investing_cf = get(
        &[
            "jppfs_cor:NetCashProvidedByUsedInInvestingActivities",
            "jpigp_cor:CashFlowsFromUsedInInvestingActivitiesIFRS",
        ],
        duration,
    );
    let dividends_paid = get(
        &[
            "jppfs_cor:CashDividendsPaidFinCF",
            "jpigp_cor:DividendsPaidFinancingActivitiesIFRS",
        ],
        duration,
    )
    .map(f64::abs);
    let excerpts: Vec<_> = rows.iter().filter(|r| r.context == "CurrentYearDuration" || r.context == "FilingDateInstant" || r.context == duration)
        .filter_map(|r| Section::from_element(r.element.rsplit(':').next()? ).map(|s| (r,s)))
        .flat_map(|(r,s)| {
            let text = normalize_text(&r.value);
            text.split('。').filter(|sentence| ["配当", "設備投資", "資金需要", "返済", "借入", "減損", "特別利益", "現金"].iter().any(|k| sentence.contains(k)))
                .take(3).map(|sentence| json!({"section":s.key(),"element":r.element,"context":r.context,"text":sentence.chars().take(240).collect::<String>()})).collect::<Vec<_>>()
        }).take(6).collect();
    json!({"version":1,"source":"EDINET","doc_id":doc.doc_id,"submitted":doc.submitted.to_string(),"period_end":doc.period_end.map(|d|d.to_string()),"scope":scope,
        "cash":cash,"debt":debt,"liabilities":liabilities,"assets":assets,"equity":equity,"operating_cf":operating_cf,"investing_cf":investing_cf,"dividends_paid":dividends_paid,
        "positive_operating_cf_3y":positive_operating_cf_3y,"operating_cf_history":cf_history,"facts":sources,"excerpts":excerpts})
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{disclosure::DocKind, Date};
    fn row(element: &str, value: &str, scope: &str, context: &str) -> XbrlRow {
        XbrlRow {
            element: format!("jppfs_cor:{element}"),
            value: value.into(),
            scope: scope.into(),
            context: context.into(),
            unit: "円".into(),
            label: if element == "InterestBearingLiabilities" {
                "有利子負債".into()
            } else {
                String::new()
            },
        }
    }
    fn doc() -> DocRef {
        DocRef {
            code: "1234".into(),
            kind: DocKind::Annual,
            doc_id: "test".into(),
            submitted: Date::new(2026, 6, 20),
            period_end: Some(Date::new(2026, 3, 31)),
        }
    }
    #[test]
    fn explicit_zero_debt_is_distinct_from_missing_components() {
        let mut debt = row(
            "InterestBearingLiabilities",
            "0",
            "連結",
            "CurrentYearInstant",
        );
        debt.element = "issuer:InterestBearingDebt".into();
        let rows = vec![row("Assets", "1000", "連結", "CurrentYearInstant"), debt];
        assert_eq!(extract(&rows, &doc())["debt"], 0.0);
        let rows = vec![
            row("Assets", "1000", "連結", "CurrentYearInstant"),
            row("ShortTermLoansPayable", "0", "連結", "CurrentYearInstant"),
        ];
        assert!(extract(&rows, &doc())["debt"].is_null());
    }
    #[test]
    fn three_year_cashflow_requires_all_three_periods() {
        let mut rows = vec![row("Assets", "1000", "連結", "CurrentYearInstant")];
        for period in [
            "CurrentYearDuration",
            "Prior1YearDuration",
            "Prior2YearDuration",
        ] {
            rows.push(row(
                "NetCashProvidedByUsedInOperatingActivities",
                "100",
                "連結",
                period,
            ));
        }
        assert_eq!(extract(&rows, &doc())["positive_operating_cf_3y"], true);
        rows.pop();
        assert!(extract(&rows, &doc())["positive_operating_cf_3y"].is_null());
    }

    #[test]
    fn excludes_other_periods_scopes_and_missing_debt() {
        let rows = vec![
            row("Assets", "1000", "連結", "CurrentYearInstant"),
            row(
                "CashAndCashEquivalents",
                "100",
                "連結",
                "CurrentYearInstant",
            ),
            row(
                "InterestBearingLiabilities",
                "0",
                "個別",
                "CurrentYearInstant_NonConsolidatedMember",
            ),
            row("CashAndCashEquivalents", "900", "連結", "Prior1YearInstant"),
        ];
        let v = extract(&rows, &doc());
        assert_eq!(v["cash"], 100.0);
        assert!(v["debt"].is_null());
    }
    #[test]
    fn conflicting_or_dash_facts_are_unknown() {
        let mut rows = vec![
            row("Assets", "1000", "連結", "CurrentYearInstant"),
            row(
                "CashAndCashEquivalents",
                "100",
                "連結",
                "CurrentYearInstant",
            ),
            row(
                "CashAndCashEquivalents",
                "200",
                "連結",
                "CurrentYearInstant",
            ),
        ];
        assert!(extract(&rows, &doc())["cash"].is_null());
        rows[2].value = "－".into();
        assert!(extract(&rows, &doc())["cash"].is_null());
    }
}
