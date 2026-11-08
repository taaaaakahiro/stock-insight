//! Same-period EDINET operating cash flows and rental disclosures.
use super::inputs::{make, unit};
use super::{strict_date, Observation};
use crate::edinet::ReitRecord;
use crate::ixbrl::{self, VerifiedFact};
use domain::{reit::Block, Date};

pub const METHOD: &str = "edinet-income-v1";

#[derive(PartialEq, Clone)]
struct Amount {
    value: f64,
    location: String,
}

fn unique<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

fn fact<'a>(facts: &'a [VerifiedFact], names: &[&str]) -> Option<&'a VerifiedFact> {
    let mut matches = facts.iter().filter(|f| names.contains(&f.name.as_str()));
    let first = matches.next()?;
    matches
        .all(|f| f.value == first.value && f.period_start == first.period_start)
        .then_some(first)
}

fn tagged(f: &VerifiedFact) -> Amount {
    Amount {
        value: f.value,
        location: format!("{} {} {}", f.location, f.context, f.name),
    }
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn label(row: &[String]) -> String {
    row.first().map_or_else(String::new, |s| compact(s))
}

fn number(text: &str) -> Option<f64> {
    let text = compact(text);
    let text = text.trim_end_matches(['円', '口']);
    let (digits, sign) = text
        .strip_prefix(['△', '▲', '−', '-'])
        .map_or((text, 1.0), |rest| (rest, -1.0));
    let digits = digits.replace(',', "");
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse::<f64>().ok().map(|n| n * sign))
        .flatten()
}

fn japanese_date(text: &str) -> Option<Date> {
    let text = compact(text);
    let (y, tail) = text.split_once('年')?;
    let y = y.rsplit(|c: char| !c.is_ascii_digit()).next()?;
    let (m, tail) = tail.split_once('月')?;
    let (d, _) = tail.split_once('日')?;
    strict_date(&format!(
        "{y}-{:02}-{:02}",
        m.parse::<u32>().ok()?,
        d.parse::<u32>().ok()?
    ))
}

struct Table {
    rows: Vec<Vec<String>>,
    columns: Vec<usize>,
    location: String,
}

impl Table {
    fn matches<'a>(&'a self, labels: &'a [&str]) -> impl Iterator<Item = &'a [String]> + 'a {
        self.rows.iter().filter_map(|row| {
            let text = label(row);
            let name = text.split('(').next().unwrap_or("");
            labels.contains(&name).then_some(row.as_slice())
        })
    }

    fn value(&self, names: &[&str]) -> Option<f64> {
        unique(self.matches(names).filter_map(|row| self.value_in(row)))
    }

    fn value_in(&self, row: &[String]) -> Option<f64> {
        unique(
            self.columns
                .iter()
                .filter_map(|&column| number(row.get(column)?)),
        )
    }

    fn evidence(&self, name: &str, value: f64) -> Amount {
        Amount {
            value,
            location: format!("{} {}", self.location, name),
        }
    }
}

fn tables(documents: &[(String, String)], start: Date, end: Date) -> Vec<Table> {
    let mut tables = Vec::new();
    for (file, html) in documents.iter().filter(|(file, _)| file.starts_with("02")) {
        for (i, block) in ixbrl::blocks(html).into_iter().enumerate() {
            let Block::Table(rows, _) = block else {
                continue;
            };
            let headers: Vec<_> = rows.iter().take(4).collect();
            let width = headers.iter().map(|row| row.len()).max().unwrap_or(0);
            let columns: Vec<_> = (1..width)
                .filter(|&col| {
                    let text = headers
                        .iter()
                        .filter_map(|row| row.get(col))
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .join(" ");
                    let Some((from, through)) = text
                        .split_once('自')
                        .and_then(|(_, dates)| dates.split_once('至'))
                    else {
                        return false;
                    };
                    japanese_date(from) == Some(start) && japanese_date(through) == Some(end)
                })
                .collect();
            if !columns.is_empty() {
                tables.push(Table {
                    rows,
                    columns,
                    location: format!("{file}#block:{}", i + 1),
                });
            }
        }
    }
    tables
}

/// For displayed currency amounts, prove the multiplier against an exact tagged figure.
fn scale(table_value: f64, tagged_value: f64) -> Option<f64> {
    if table_value == 0.0 {
        return None;
    }
    unique(
        [1.0, 1_000.0, 1_000_000.0]
            .into_iter()
            .filter(|unit| table_value * unit == tagged_value),
    )
}

fn note_units(tables: &[Table], net: &VerifiedFact) -> Option<Amount> {
    let mut notes = tables.iter().filter_map(|table| {
        let observed = table.value(&["当期純利益", "当期純損失", "当期純損失(△)"])?;
        scale(observed, net.value)?;
        let units = table.value(&["期中平均投資口数", "日数加重平均投資口数"])?;
        (units > 0.0).then_some((&table.location, units))
    });
    let (table, units) = notes.next()?;
    if notes.any(|(_, other)| other != units) {
        return None;
    }
    Some(Amount {
        value: units,
        location: format!("{table} 期中平均投資口数"),
    })
}

fn rental_note(tables: &[Table], rent_expenses: &VerifiedFact) -> Option<(Amount, Amount)> {
    unique(tables.iter().filter_map(|table| {
        let expense = table.value(&["不動産賃貸事業費用合計", "賃貸事業費用合計"])?;
        let unit = scale(expense, rent_expenses.value)?;
        let revenue = table.value(&["不動産賃貸事業収益合計", "賃貸事業収益合計"])?;
        let depreciation = table.value(&["減価償却費"])?;
        if revenue < 0.0 || !(0.0..=expense).contains(&depreciation) {
            return None;
        }
        Some((
            table.evidence("賃貸事業の減価償却費", depreciation * unit),
            table.evidence(
                "賃貸事業収益合計 − 費用合計 ＋ 減価償却費",
                (revenue - expense + depreciation) * unit,
            ),
        ))
    }))
}

const SALE_GAIN: &[&str] = &[
    "GainOnSalesOfRealEstatePropertiesORINV",
    "GainOnTransferOfRealEstatePropertiesORINV",
];
const SALE_LOSS: &[&str] = &[
    "LossOnSalesOfRealEstatePropertiesOEINV",
    "LossOnTransferOfRealEstatePropertiesOEINV",
];

fn sale_amount(
    tables: &[Table],
    net: &VerifiedFact,
    operating: &VerifiedFact,
    gain: bool,
) -> Option<Amount> {
    let labels: &[&str] = if gain {
        &["不動産等売却益", "不動産売却益", "不動産等譲渡益"]
    } else {
        &["不動産等売却損", "不動産売却損", "不動産等譲渡損"]
    };
    // Require a complete, reconciled P&L before treating an absent disposal
    // line as zero. An existing line with no verified amount stays missing.
    unique(tables.iter().filter_map(|table| {
        let profit = table.value(&["当期純利益"])?;
        let unit = scale(profit, net.value)?;
        if table.value(&["営業利益"])? * unit != operating.value {
            return None;
        }
        let revenue = table.value(&["営業収益合計"])?;
        let expenses = table.value(&["営業費用合計"])?;
        // Display totals can differ by one currency unit due to truncation.
        if (revenue - expenses - operating.value / unit).abs() > 1.0 {
            return None;
        }
        let relevant: Vec<_> = table
            .rows
            .iter()
            .filter(|row| {
                let l = label(row);
                let disposal = ["売却", "譲渡"].iter().any(|word| l.contains(word));
                disposal
                    && (l.contains("損益")
                        || if gain {
                            l.contains('益')
                        } else {
                            l.contains('損')
                        })
            })
            .collect();
        if relevant.is_empty() {
            return Some(table.evidence("照合済み損益計算書に該当する売却損益の科目なし", 0.0));
        }
        if relevant.len() != 1 || !labels.contains(&label(relevant[0]).as_str()) {
            return None;
        }
        // An amount without a standard taxonomy tag requires an unambiguous
        // visible value. A dash alone has no numeric meaning here.
        let value = table.value_in(relevant[0])? * unit;
        (value >= 0.0).then(|| table.evidence("不動産売却損益", value))
    }))
}

pub(super) fn observations(
    record: &ReitRecord,
    documents: &[(String, String)],
    now: i64,
    source_url: &str,
) -> Vec<Observation> {
    let end = record.period_end;
    let facts = ixbrl::verified_reit_duration_facts(documents, end);
    let start = unique(facts.iter().filter_map(|f| f.period_start));
    let net = fact(&facts, &["ProfitLoss"]);
    let operating = fact(&facts, &["OperatingIncome"]);
    let interest = fact(&facts, &["InterestExpensesOpeCF"]).filter(|f| f.value >= 0.0);
    let tables = start.map_or_else(Vec::new, |start| tables(documents, start, end));
    let sale = |gain| {
        let names = if gain { SALE_GAIN } else { SALE_LOSS };
        if facts.iter().any(|f| names.contains(&f.name.as_str())) {
            // Standard disposal tags identify the amount even when the visible
            // summary table is split across files or uses a different layout.
            return fact(&facts, names).filter(|f| f.value >= 0.0).map(tagged);
        }
        sale_amount(&tables, net?, operating?, gain)
    };
    let gain = sale(true);
    let loss = sale(false);
    let rent =
        fact(&facts, &["ExpensesRelatedToRentBusinessOEINV"]).and_then(|f| rental_note(&tables, f));
    let depreciation = rent.as_ref().map(|(dep, _)| dep.clone());
    let noi = rent.map(|(_, noi)| noi);
    let ebitda = (|| {
        let operating = operating?;
        let depreciation = depreciation.as_ref()?;
        let gain = gain.as_ref()?;
        let loss = loss.as_ref()?;
        Some(Amount {
            value: operating.value + depreciation.value - gain.value + loss.value,
            location: format!(
                "{} ＋ {} − {} ＋ {}",
                tagged(operating).location,
                depreciation.location,
                gain.location,
                loss.location
            ),
        })
    })();
    let mut rows = Vec::new();
    for (key, value) in [
        ("net_income", net.map(tagged)),
        ("ffo_units", net.and_then(|f| note_units(&tables, f))),
        ("property_sale_gain", gain),
        ("property_sale_loss", loss),
        ("depreciation", depreciation),
        ("period_interest", interest.map(tagged)),
        ("period_noi", noi),
        ("period_ebitda", ebitda),
    ] {
        // Missing later filings must not resurrect a value from an older revision.
        let doc = &record.doc;
        let mut observation = make(
            &record.code,
            key,
            value.as_ref().map(|v| v.value),
            unit(key).unwrap_or_default(),
            start,
            end,
            end,
            &doc.submitted.to_string(),
            &doc.doc_id,
            now,
            source_url,
        );
        observation.method_version = METHOD.into();
        if matches!(key, "period_noi" | "period_ebitda") {
            observation.data_kind = "derived".into();
        }
        if let Some(value) = value {
            observation.normalization_flags.push(value.location);
        }
        if observation.validate() {
            rows.push(observation);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edinet::{reit_record, ReitBalance, ReitDocRef, ReitReport};

    fn d(value: &str) -> Date {
        strict_date(value).unwrap()
    }

    const CONTEXTS: &str = r#"
<xbrli:context id="CurrentYearDuration_NonConsolidatedMember">
  <xbrli:period><xbrli:startDate>2026-02-01</xbrli:startDate><xbrli:endDate>2026-07-31</xbrli:endDate></xbrli:period>
  <xbrli:scenario><xbrldi:explicitMember dimension="jppfs_cor:ConsolidatedOrNonConsolidatedAxis">jppfs_cor:NonConsolidatedMember</xbrldi:explicitMember></xbrli:scenario>
</xbrli:context>
<xbrli:context id="CurrentYearDuration"><xbrli:period>
  <xbrli:startDate>2026-02-01</xbrli:startDate><xbrli:endDate>2026-07-31</xbrli:endDate>
</xbrli:period></xbrli:context>
<xbrli:unit id="JPY"><xbrli:measure>iso4217:JPY</xbrli:measure></xbrli:unit>"#;

    fn tag(name: &str, amount: i64) -> String {
        format!(
            r#"<ix:nonFraction name="jppfs_cor:{name}" contextRef="CurrentYearDuration_NonConsolidatedMember" unitRef="JPY" scale="3">{amount}</ix:nonFraction>"#
        )
    }

    fn fixture(sale_row: &str, end: Date) -> Vec<Observation> {
        let profit = tag("ProfitLoss", 5_000);
        let operating = tag("OperatingIncome", 6_000);
        let gain = tag("GainOnTransferOfRealEstatePropertiesORINV", 1_000);
        let interest = tag("InterestExpensesOpeCF", 800);
        let expense = tag("ExpensesRelatedToRentBusinessOEINV", 3_000);
        let header = r#"<tr><td></td><td>前期 自 2025年 8月 1日 至 2026年 1月 31日</td>
            <td>当期 自 2026年 2月 1日 至 2026年 7月 31日</td></tr>"#;
        let html = format!(
            r#"
{CONTEXTS}
<table>{header}
<tr><td>営業収益合計</td><td>555</td><td>11000</td></tr>
<tr><td>不動産等譲渡益</td><td>99999</td><td>※1 {gain}</td></tr>
<tr><td>営業費用合計</td><td>555</td><td>5000</td></tr>
{sale_row}
<tr><td>営業利益</td><td>555</td><td>{operating}</td></tr>
<tr><td>当期純利益</td><td>555</td><td>{profit}</td></tr>
</table>
<table>{header}
<tr><td>当期純利益（千円）</td><td>8888</td><td>5000</td></tr>
<tr><td>期中平均投資口数（口）</td><td>99999</td><td>2000</td></tr>
</table>
<table>{header}
<tr><td>不動産賃貸事業収益合計</td><td>99999</td><td>10000</td></tr>
<tr><td>不動産賃貸事業費用合計</td><td>99999</td><td>{expense}</td></tr>
<tr><td>減価償却費</td><td>99999</td><td>1500</td></tr>
</table>
{interest}
<ix:nonFraction name="jppfs_cor:InterestExpensesOpeCF" contextRef="CurrentYearDuration" unitRef="JPY" scale="3">99999</ix:nonFraction>
"#
        );
        let r = reit_record(
            ReitDocRef {
                doc_id: "test-doc".into(),
                edinet_code: "test".into(),
                filer_name: "test".into(),
                submitted: end.add_days(45),
                period_end: Some(end),
            },
            ReitReport {
                code: Some("8951".into()),
                balance: ReitBalance::default(),
                debt: Default::default(),
                properties: vec![],
                property_total: None,
                assets: None,
            },
        )
        .unwrap();
        observations(
            &r,
            &[("0205-test.htm".into(), html)],
            0,
            "https://example.com/test.pdf",
        )
    }

    #[test]
    fn financial_notes_are_tied_to_the_same_statement_period_and_scope() {
        let observations = fixture("", d("2026-07-31"));
        let value = |key: &str| {
            observations
                .iter()
                .find(|o| o.metric_key == key)
                .unwrap()
                .metric_value
        };
        assert_eq!(value("net_income"), Some(5_000_000.0));
        assert_eq!(value("ffo_units"), Some(2000.0));
        assert_eq!(value("depreciation"), Some(1_500_000.0));
        assert_eq!(value("property_sale_gain"), Some(1_000_000.0));
        assert_eq!(value("property_sale_loss"), Some(0.0));
        assert_eq!(value("period_interest"), Some(800_000.0));
        assert_eq!(value("period_noi"), Some(8_500_000.0));
        assert_eq!(value("period_ebitda"), Some(6_500_000.0));
        assert!(observations
            .iter()
            .all(|o| o.period_start.as_deref() == Some("2026-02-01")
                && !o.normalization_flags.is_empty()));
        assert!(fixture("", d("2026-06-30"))
            .iter()
            .all(|o| o.metric_value.is_none()));
    }

    #[test]
    fn a_disclosed_but_unreadable_sale_loss_does_not_become_zero() {
        let rows = fixture(
            "<tr><td>不動産等売却損</td><td>0</td><td>—</td></tr>",
            d("2026-07-31"),
        );
        for key in ["property_sale_loss", "period_ebitda"] {
            assert_eq!(
                rows.iter()
                    .find(|o| o.metric_key == key)
                    .unwrap()
                    .metric_value,
                None
            );
        }
    }
}
