//! REITの有報から、同一期末の賃貸等不動産とNAVを取得する。

use crate::ixbrl;
use domain::{reit::Block, Date};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReitAssets {
    pub property_book_value: i64,
    pub property_fair_value: i64,
    pub nav_per_unit: Option<f64>,
}

pub fn extract(
    documents: &[(String, String)],
    fund: &[Block],
    accounts: &[Block],
    period: Date,
) -> Option<ReitAssets> {
    let (property_book_value, property_fair_value) = property_values(accounts, period)?;
    let nav_per_unit = issued_units(fund, period).and_then(|units| {
        let facts = ixbrl::verified_reit_instant_facts(documents, period);
        let net_assets = same_value(
            facts
                .iter()
                .filter(|fact| fact.name == "NetAssets")
                .map(|fact| fact.value),
        )?;
        let nav = net_assets + (property_fair_value - property_book_value) as f64;
        Some(nav / units as f64)
    });
    Some(ReitAssets {
        property_book_value,
        property_fair_value,
        nav_per_unit,
    })
}

fn same_value<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn amount(text: &str) -> Option<i64> {
    let digits = compact(text).replace(',', "");
    digits.parse::<i64>().ok().filter(|value| *value > 0)
}

fn japanese_date(text: &str) -> Option<(i32, u32, Option<u32>)> {
    let (before_year, rest) = text.rsplit_once('年')?;
    let year = before_year
        .rsplit(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    let (month, rest) = rest.split_once('月')?;
    let day = rest.split_once('日').and_then(|(day, _)| day.parse().ok());
    Some((year, month.parse().ok()?, day))
}

fn period_column(rows: &[Vec<String>], period: Date, month_only: bool) -> Option<usize> {
    let headers = rows.get(..rows.len().min(3))?;
    let width = headers.iter().map(Vec::len).max()?;
    let mut columns = (0..width).filter(|&index| {
        let date = headers.iter().rev().find_map(|row| {
            let header = compact(row.get(index)?);
            let date = japanese_date(&header)?;
            // 「自…」だけのセルは開始日。期間が一つのセルなら末尾の日付を使う。
            (!header.starts_with('自') || header.contains('至')).then_some(date)
        });
        date.is_some_and(|(year, month, day)| {
            (year, month) == (period.y, period.m)
                && (day == Some(period.d) || (month_only && day.is_none()))
        })
    });
    let column = columns.next()?;
    columns.next().is_none().then_some(column)
}

fn issued_units(fund: &[Block], period: Date) -> Option<i64> {
    same_value(fund.iter().filter_map(|block| {
        let Block::Table(rows, _) = block else {
            return None;
        };
        let column = period_column(rows, period, true)?;
        same_value(rows.iter().filter_map(|row| {
            let labels: Vec<_> = row
                .get(..column)?
                .iter()
                .map(|cell| compact(cell))
                .collect();
            let label = labels.iter().find(|cell| {
                matches!(
                    cell.split('(').next(),
                    Some("発行済投資口の総口数" | "発行済投資口数" | "発行済投資口総数")
                )
            })?;
            let in_label = label.contains("(口)");
            let in_column = labels
                .iter()
                .any(|cell| matches!(cell.as_str(), "口" | "(口)"));
            (in_label || in_column)
                .then(|| amount(row.get(column)?))
                .flatten()
        }))
    }))
}

fn monetary_unit(text: &str) -> Option<i64> {
    let (_, unit) = text.rsplit_once("単位:")?;
    match unit.split_once(')').map_or(unit, |(unit, _)| unit) {
        "百万円" => Some(1_000_000),
        "千円" => Some(1_000),
        "円" => Some(1),
        _ => None,
    }
}

fn property_values(accounts: &[Block], period: Date) -> Option<(i64, i64)> {
    let mut in_note = false;
    let mut scale = None;
    let mut values = vec![];
    for block in accounts {
        match block {
            Block::Text(text) => {
                let text = compact(text);
                if let Some(start) = [
                    "賃貸等不動産に関する注記",
                    "賃貸等不動産関係",
                    "賃貸等不動産の時価に関する注記",
                ]
                .iter()
                .filter_map(|title| text.rfind(title))
                .max()
                {
                    in_note = true;
                    scale = monetary_unit(&text[start..]);
                } else if in_note {
                    scale = monetary_unit(&text).or(scale);
                }
            }
            Block::Table(rows, _) if in_note => {
                if rows.len() == 1 {
                    scale = rows[0]
                        .iter()
                        .find_map(|cell| monetary_unit(&compact(cell)))
                        .or(scale);
                    continue;
                }
                let header_units = rows.iter().take(3).flatten().filter_map(|cell| {
                    let text = compact(cell);
                    text.contains("単位:").then(|| monetary_unit(&text))
                });
                let unit = same_value(scale.map(Some).into_iter().chain(header_units)).flatten();
                if let Some(value) = unit.and_then(|unit| property_table(rows, period, unit)) {
                    values.push(value);
                }
                in_note = false;
                scale = None;
            }
            _ => {}
        }
    }
    same_value(values.into_iter())
}

fn property_table(rows: &[Vec<String>], period: Date, scale: i64) -> Option<(i64, i64)> {
    let column = period_column(rows, period, false)?;
    let read_row = |labels: &[&str]| {
        let candidates: Vec<_> = rows
            .iter()
            .filter_map(|row| {
                let head: Vec<_> = row
                    .get(..column)?
                    .iter()
                    .map(|cell| compact(cell))
                    .collect();
                head.iter()
                    .any(|cell| labels.contains(&cell.as_str()))
                    .then_some((head, row))
            })
            .collect();
        let (head, row) = match candidates.as_slice() {
            [single] => single,
            _ => {
                let mut totals = candidates.iter().filter(|(head, _)| {
                    head.iter()
                        .any(|cell| matches!(cell.as_str(), "合計" | "総計"))
                });
                let total = totals.next()?;
                if totals.next().is_some() {
                    return None;
                }
                total
            }
        };
        let is_total = head
            .iter()
            .any(|cell| matches!(cell.as_str(), "合計" | "総計"));
        Some((amount(row.get(column)?)?.checked_mul(scale)?, is_total))
    };
    let (book, book_total) = read_row(&["期末残高", "期末帳簿価額"])?;
    let (fair, fair_total) = read_row(&["期末時価", "期末の時価", "期末現在の時価"])?;
    (book_total == fair_total).then_some((book, fair))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Date = Date {
        y: 2026,
        m: 6,
        d: 30,
    };
    const FUND: &str = r#"
        <table><tr><td>決算年月</td><td>単位</td><td>2026年６月</td><td>2025年12月</td></tr>
        <tr><td>発行済投資口の総口数 (c)</td><td>口</td><td>10,000</td><td>5,000</td></tr></table>
        <table><tr><td>発行済投資口の総口数</td><td>12,000口</td></tr></table>"#;
    const ACCOUNTS: &str = r#"
        <p>（賃貸等不動産に関する注記）</p><p>（単位：千円）</p>
        <table><tr><td></td><td></td><td>前期</td><td>当期</td></tr>
        <tr><td></td><td></td><td>至2025年12月31日</td><td>至2026年６月30日</td></tr>
        <tr><td rowspan="2">オフィス</td><td>期末残高</td><td>700</td><td>600</td></tr>
        <tr><td>期末時価</td><td>800</td><td>900</td></tr>
        <tr><td rowspan="2">合計</td><td>期末残高</td><td>900</td><td>1,000</td></tr>
        <tr><td>期末時価</td><td>1,100</td><td>1,400</td></tr></table>"#;
    const FINANCIALS: &str = r#"
        <xbrli:context id="standalone">
          <xbrli:period><xbrli:instant>2026-06-30</xbrli:instant></xbrli:period>
          <xbrli:scenario><xbrldi:explicitMember dimension="jppfs_cor:ConsolidatedOrNonConsolidatedAxis">
            jppfs_cor:NonConsolidatedMember
          </xbrldi:explicitMember></xbrli:scenario>
        </xbrli:context>
        <xbrli:unit id="JPY"><xbrli:measure>iso4217:JPY</xbrli:measure></xbrli:unit>
        <ix:nonFraction name="jppfs_cor:NetAssets" contextRef="standalone" unitRef="JPY" scale="3">800</ix:nonFraction>"#;

    #[test]
    fn nav_uses_same_period_units_and_property_totals() {
        let docs = [("02.htm".into(), FINANCIALS.into())];
        let assets = extract(
            &docs,
            &ixbrl::blocks(FUND),
            &ixbrl::blocks(ACCOUNTS),
            PERIOD,
        )
        .unwrap();
        assert_eq!(assets.property_book_value, 1_000_000);
        assert_eq!(assets.property_fair_value, 1_400_000);
        assert_eq!(assets.nav_per_unit, Some(120.0));
    }

    #[test]
    fn accepts_units_in_table_headers_and_rejects_conflicting_units() {
        let fund = FUND.replace("(c)</td><td>口", "（口）</td><td>");
        let accounts = ACCOUNTS.replace("<p>（単位：千円）</p>", "").replace(
            "<td></td><td></td><td>前期",
            "<td>（単位：千円）</td><td></td><td>前期",
        );
        assert_eq!(issued_units(&ixbrl::blocks(&fund), PERIOD), Some(10_000));
        assert_eq!(
            issued_units(&ixbrl::blocks(&fund.replace("（口）", "（千口）")), PERIOD),
            None
        );
        assert_eq!(
            property_values(&ixbrl::blocks(&accounts), PERIOD),
            Some((1_000_000, 1_400_000))
        );
        let conflict = accounts.replace("</p>", "</p><p>（単位：百万円）</p>");
        assert!(property_values(&ixbrl::blocks(&conflict), PERIOD).is_none());
    }

    #[test]
    fn missing_or_conflicting_inputs_withhold_only_the_affected_values() {
        for financials in [
            FINANCIALS.replace("2026-06-30", "2026-05-31"),
            FINANCIALS.replace(
                "jppfs_cor:NonConsolidatedMember",
                "jppfs_cor:ConsolidatedMember",
            ),
            format!("{FINANCIALS}{}", FINANCIALS.replace(">800<", ">801<")),
        ] {
            let docs = [("02.htm".into(), financials)];
            let assets = extract(
                &docs,
                &ixbrl::blocks(FUND),
                &ixbrl::blocks(ACCOUNTS),
                PERIOD,
            )
            .unwrap();
            assert_eq!(assets.nav_per_unit, None);
        }
        for accounts in [
            ACCOUNTS.replace("千円", "ドル"),
            ACCOUNTS.replace("合計", "住宅"),
            ACCOUNTS.replace("1,400", "－"),
            ACCOUNTS.replace("６月30日", "６月29日"),
        ] {
            assert!(property_values(&ixbrl::blocks(&accounts), PERIOD).is_none());
        }
    }
}
