//! Synthetic integration corpus. These cases verify wiring, not real-model quality.
use adapter_batch::{ixbrl, reit_metrics, reit_rag};
use domain::Date;
use serde_json::{json, Value};

fn document() -> String {
    let mut html = String::from(
        r#"<html><body>
<xbrli:context id="current"><xbrli:period><xbrli:instant>2026-06-30</xbrli:instant></xbrli:period></xbrli:context>
<xbrli:unit id="JPY"><xbrli:measure>iso4217:JPY</xbrli:measure></xbrli:unit>
<p>借入金の金利見直しと返済状況</p>
<table><tr><th>項目</th><th>金額（百万円）</th></tr>"#,
    );
    for (tag, amount) in [
        ("Assets", 1000),
        ("Liabilities", 500),
        ("NetAssets", 500),
        ("ShortTermLoansPayable", 0),
        ("LongTermLoansPayable", 400),
        ("CurrentPortionOfLongTermLoansPayable", 0),
        ("InvestmentCorporationBondINV", 50),
        ("CurrentPortionOfInvestmentCorporationBondsINV", 0),
    ] {
        html.push_str(&format!(r#"<tr><td>{tag}</td><td><ix:nonFraction name="x:{tag}" contextRef="current" unitRef="JPY" scale="6">{amount}</ix:nonFraction></td></tr>"#));
    }
    html.push_str("</table><p>固定金利には金利スワップによる固定化を含みます。</p></body></html>");
    html
}

fn metrics(html: String) -> Value {
    let period = Date::parse("2026-06-30").unwrap();
    let mut numbers = reit_metrics::extract(&[("synthetic.htm".into(), html)], period);
    numbers["ladder"] = json!([{"item":"借入金と投資法人債","amounts":[50_000_000,80_000_000,80_000_000,80_000_000,80_000_000,80_000_000]}]);
    let profile = json!({"period_end":"2026-06-30","short_loans":0,"long_loans":400_000_000,"bonds":50_000_000,
        "checks":"{\"loans\":\"一致\"}","reprice_1y":50_000_000,"reprice_1y_max":50_000_000});
    reit_metrics::evaluate(&numbers, &profile, Date::parse("2026-10-26").unwrap())
}

#[test]
fn source_to_verified_numbers_and_cited_assessment() {
    let html = document();
    let verified = metrics(html.clone());
    assert_eq!(verified["ltv"], 0.45);
    assert_eq!(verified["positive_allowed"], true);
    let chunks = reit_rag::chunks("synthetic-document", &ixbrl::blocks(&html));
    let evidence = reit_rag::retrieve(&chunks, &["固定金利", "スワップ"]);
    let quote = "固定金利には金利スワップによる固定化を含みます。";
    let source = evidence
        .iter()
        .find(|c| c["body"].as_str().unwrap().contains(quote))
        .unwrap();
    let extraction = json!({"facts":[{"topic":"debt","chunk_id":source["id"],"quote":quote}]});
    let facts = reit_rag::validate_extraction(&extraction, &evidence).unwrap();
    assert_eq!(facts[0]["numeric_status"], "not_promoted");
    let unavailable = json!({"status":"unavailable","reason":"数値未検証","evidence_ids":[]});
    let assessment = json!({"axes":{"income":unavailable,"value":unavailable,"debt":{
        "status":"positive","reason":"検証済みの借入指標は設定範囲内です。",
        "evidence_ids":["metric:ltv","metric:maturing_ratio","metric:reprice_ratio",source["id"]]}}});
    assert!(reit_rag::validate_assessment_with_metrics(&assessment, &facts, &verified).is_ok());
    let mut fabricated = extraction.clone();
    fabricated["facts"][0]["quote"] = json!("固定金利の比率は百パーセントです。");
    assert!(reit_rag::validate_extraction(&fabricated, &evidence).is_err());
    fabricated["facts"][0]["quote"] = json!(quote);
    fabricated["facts"][0]["chunk_id"] = json!("another-issuer-document");
    assert!(reit_rag::validate_extraction(&fabricated, &evidence).is_err());
}

#[test]
fn corrupted_source_cannot_become_verified_ltv() {
    let valid = document();
    for invalid in [
        valid.replace("iso4217:JPY", "iso4217:USD"),
        valid.replace("2026-06-30", "2025-06-30"),
        valid.replace("scale=\"6\">0", "scale=\"6\">－"),
        valid.replace(
            "x:CurrentPortionOfLongTermLoansPayable",
            "x:UnknownComponent",
        ),
    ] {
        let result = metrics(invalid);
        assert!(result["ltv"].is_null());
        assert_ne!(result["positive_allowed"], true);
    }
}
