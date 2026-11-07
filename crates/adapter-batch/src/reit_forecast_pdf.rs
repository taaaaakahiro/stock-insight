//! 分配予想を公式開示のサマリー表から読む。日付と列が照合できない表は採用しない。

use domain::reit_distribution::{ForecastDetails, ForecastPeriod};
use domain::{Date, StockCode};
use serde::Deserialize;
use std::collections::{BTreeSet, HashSet};
use std::io::Write;
use std::process::{Command, Stdio};
use usecase::fins::ForecastReport;

#[derive(Debug, Deserialize)]
pub struct Disclosure {
    pub code: String,
    pub doc_id: String,
    pub disclosed: String,
    pub time: Option<String>,
}

pub const MAX_PDF_BYTES: usize = 32 * 1024 * 1024;

pub fn from_tdnet(row: &serde_json::Value, reit_codes: &HashSet<String>) -> Option<Disclosure> {
    let code = crate::jquants::code_from_jquants(row["Code"].as_str()?)?;
    let title = compact(&normalize(row["Title"].as_str()?));
    let relevant = title.contains("決算短信(REIT)")
        || (title.contains("分配") && title.contains("予想") && title.contains("修正"));
    if !reit_codes.contains(code.as_str())
        || !relevant
        || row["DiscStatus"].as_str() == Some("delete")
        || !row["Docs"]
            .as_array()?
            .iter()
            .any(|doc| matches!(doc.as_str(), Some("g" | "s")))
    {
        return None;
    }
    Some(Disclosure {
        code: code.as_str().to_owned(),
        doc_id: row["DiscNo"].as_str()?.to_owned(),
        disclosed: row["DiscDate"].as_str()?.to_owned(),
        time: row["DiscTime"].as_str().map(str::to_owned),
    })
}

pub fn pdf_text(pdf: &[u8]) -> Result<String, String> {
    if pdf.len() > MAX_PDF_BYTES || !pdf.starts_with(b"%PDF-") {
        return Err("invalid or oversized PDF".into());
    }
    let mut child = Command::new("pdftotext")
        .args(["-f", "1", "-l", "3", "-layout", "-enc", "UTF-8", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("pdftotext (poppler-utils): {e}"))?;
    let mut input = child.stdin.take().expect("piped stdin");
    let (output, write) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(pdf));
        (
            child.wait_with_output(),
            writer.join().expect("PDF input writer"),
        )
    });
    let output = output.map_err(|e| format!("pdftotext: {e}"))?;
    if !output.status.success() {
        return Err(format!("pdftotext failed ({})", output.status));
    }
    write.map_err(|e| format!("pdftotext input: {e}"))?;
    String::from_utf8(output.stdout).map_err(|_| "PDF text is not UTF-8".into())
}

fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
            '〜' => '~',
            _ => c,
        })
        .collect()
}

fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn jp_date(s: &str) -> Option<(Date, &str)> {
    let (y, rest) = s.split_once('年')?;
    let (m, rest) = rest.split_once('月')?;
    let (d, rest) = rest.split_once('日')?;
    if y.len() != 4 || !(1..=2).contains(&m.len()) || !(1..=2).contains(&d.len()) {
        return None;
    }
    let date = Date::new(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    ((1900..=2200).contains(&date.y)
        && (1..=12).contains(&date.m)
        && (1..=31).contains(&date.d)
        && Date::from_days(date.to_days()) == date)
        .then_some((date, rest))
}

fn date_ranges(s: &str) -> BTreeSet<(Date, Date)> {
    s.match_indices('年')
        .filter_map(|(i, _)| {
            let start_index = i.checked_sub(4)?;
            let (start, rest) = jp_date(s.get(start_index..)?)?;
            let (end, _) = jp_date(
                rest.strip_prefix('~')
                    .or_else(|| rest.strip_prefix("から"))?,
            )?;
            (end >= start && end.to_days() - start.to_days() < 370).then_some((start, end))
        })
        .collect()
}

fn period_label(s: &str) -> Option<((i32, u32), &str)> {
    let (year, rest) = s.split_once('年')?;
    let (month, rest) = rest.trim_start().split_once("月期")?;
    (year.len() == 4).then_some(((year.parse().ok()?, month.trim().parse().ok()?), rest))
}

fn yen(cell: &str) -> Option<u64> {
    let token = cell.strip_suffix('円').unwrap_or(cell);
    let mut groups = token.split(',');
    let first = groups.next()?;
    if first.is_empty() || !first.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if token.contains(',')
        && (first.len() > 3
            || groups.any(|part| part.len() != 3 || !part.bytes().all(|c| c.is_ascii_digit())))
    {
        return None;
    }
    token
        .replace(',', "")
        .parse::<u64>()
        .ok()
        .filter(|&n| n <= u32::MAX.into())
}

fn amount(cells: &[&str]) -> Option<f64> {
    let excess = |cell| {
        if matches!(cell, "-" | "―" | "–" | "−") {
            Some(0)
        } else {
            yen(cell)
        }
    };
    let normal = match cells {
        [normal, _excess] => yen(normal)?,
        [a, b, c] => {
            // 実際の開示には「合計、分配金、超過分」と「分配金、超過分、合計」がある。
            let normal = yen(a)
                .zip(yen(b))
                .zip(excess(c))
                .filter(|&((total, normal), extra)| normal + extra == total)
                .map(|((_, normal), _)| normal);
            let alternate = yen(a)
                .zip(excess(b))
                .zip(yen(c))
                .filter(|&((normal, extra), total)| normal + extra == total)
                .map(|((normal, _), _)| normal);
            if normal.zip(alternate).is_some_and(|(a, b)| a != b) {
                return None;
            }
            normal.or(alternate)?
        }
        _ => return None,
    };
    Some(normal as f64)
}

fn table_units(line: &str) -> Option<(usize, usize)> {
    let cols: Vec<_> = line
        .split_whitespace()
        .map(|s| s.trim_matches(['(', ')']))
        .collect();
    let (money, unit) =
        if cols.starts_with(&["百万円", "%", "百万円", "%", "百万円", "%", "百万円", "%"])
        {
            (8, &cols[8..])
        } else if cols.starts_with(&["百万円"; 4]) {
            (4, &cols[4..])
        } else {
            return None;
        };
    ((2..=3).contains(&unit.len()) && unit.iter().all(|u| *u == "円"))
        .then_some((money, unit.len()))
}

fn table_amount(row: &str, (money, units): (usize, usize)) -> Option<f64> {
    let cells: Vec<_> = row.split_whitespace().collect();
    (cells.len() == money + units)
        .then(|| amount(&cells[money..]))
        .flatten()
}

fn find_period(
    ranges: &BTreeSet<(Date, Date)>,
    month: (i32, u32),
    amount: f64,
) -> Option<ForecastPeriod> {
    let mut matches = ranges.iter().filter(|(_, end)| (end.y, end.m) == month);
    let &(start, end) = matches.next()?;
    matches.next().is_none().then_some(ForecastPeriod {
        start: Some(start),
        end,
        amount,
    })
}

fn financial_results(lines: &[&str]) -> Option<Vec<ForecastPeriod>> {
    let section = lines
        .iter()
        .enumerate()
        .find(|(i, line)| {
            let label = compact(line);
            label
                .split_once('.')
                .is_some_and(|(number, after)| number == "2" && period_label(after).is_some())
                && compact(&lines[*i..lines.len().min(*i + 3)].join("")).contains("運用状況の予想")
        })?
        .0;
    let window = &lines[section..lines.len().min(section + 20)];
    let (unit_row, units) = window
        .iter()
        .enumerate()
        .find_map(|(i, line)| table_units(line).map(|u| (i, u)))?;
    let header = compact(&window[..unit_row].join(""));
    if !header.contains("利益超過")
        || !header.contains("分配金")
        || (units.1 == 2 && !header.contains("含まな"))
    {
        return None;
    }
    let ranges = date_ranges(&header);
    let periods: Vec<_> = window[unit_row + 1..]
        .iter()
        .skip_while(|line| line.trim().is_empty())
        .map_while(|line| period_label(line.trim_start()))
        .map(|(month, row)| find_period(&ranges, month, table_amount(row, units)?))
        .collect::<Option<_>>()?;
    (!periods.is_empty() && periods.len() <= 2).then_some(periods)
}

fn revisions(lines: &[&str]) -> Option<Vec<ForecastPeriod>> {
    let ranges = date_ranges(&compact(&lines.join("")));
    let mut target = None;
    let mut result = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let compact_line = compact(line);
        let heading = compact_line
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .strip_prefix('.')
            .unwrap_or(&compact_line);
        if compact_line.contains("運用状況")
            && compact_line.contains("分配")
            && compact_line.contains("予想")
            && compact_line.contains("修正")
        {
            if let Some((month, rest)) = period_label(heading) {
                if !rest.contains("月期") {
                    target = Some(month);
                }
            }
        }
        if compact_line.starts_with("今回修正予想") || compact_line.starts_with("今回発表予想")
        {
            let header = compact(&lines[i.saturating_sub(15)..i].join(""));
            if ![
                "営業収益",
                "営業利益",
                "経常利益",
                "純利益",
                "利益超過",
                "分配金",
            ]
            .iter()
            .all(|label| header.contains(label))
                || header.matches("百万円").count() < 4
            {
                return None;
            }
            let row = match line.split_once(')') {
                Some((_, after)) => after,
                None => {
                    // 縦に折り返された行ラベルでは「(B)」の直前に数値の行がある。
                    if compact(lines.get(i + 2)?).as_str() != "(B)" {
                        return None;
                    }
                    lines.get(i + 1)?
                }
            };
            let cells: Vec<_> = row.split_whitespace().collect();
            let distributions = cells.get(4..)?;
            if distributions.len() == 2 && header.contains("含む") && !header.contains("含まな")
            {
                return None;
            }
            let value = amount(distributions)?;
            result.push(find_period(&ranges, target?, value)?);
        }
    }
    (!result.is_empty() && result.len() <= 2).then_some(result)
}

pub fn forecast_report(disclosure: &Disclosure, text: &str) -> Option<ForecastReport> {
    let code = StockCode::parse(&disclosure.code)?;
    let disclosed = Date::parse(&disclosure.disclosed)?;
    if disclosed.to_string() != disclosure.disclosed
        || disclosure.doc_id.is_empty()
        || !disclosure
            .doc_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let normalized = normalize(text);
    let lines: Vec<_> = normalized.lines().collect();
    let opening: Vec<_> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .take(18)
        .map(|l| compact(l))
        .collect();
    let opening_date = opening
        .iter()
        .find_map(|l| jp_date(l).and_then(|(date, rest)| rest.is_empty().then_some(date)))?;
    let code_matches = opening.iter().any(|line| {
        ["コード番号", "証券コード"].iter().any(|label| {
            line.split_once(label).is_some_and(|(_, rest)| {
                rest.trim_start_matches([':', '('])
                    .strip_prefix(code.as_str())
                    .is_some_and(|suffix| !suffix.starts_with(|c: char| c.is_ascii_digit()))
            })
        })
    });
    if opening_date != disclosed || !code_matches {
        return None;
    }
    let mut periods = if opening.first()?.contains("決算短信(REIT)") {
        financial_results(&lines)?
    } else if opening.iter().any(|l| {
        l.contains("分配") && l.contains("予想") && l.contains("修正") && l.contains("お知らせ")
    }) {
        revisions(&lines)?
    } else {
        return None;
    };
    periods.sort_by_key(|p| p.end);
    let primary = *periods.first()?;
    let other = periods.get(1).copied();
    if primary.end <= disclosed
        || other.is_some_and(|p| p.start.is_none_or(|start| start <= primary.end))
    {
        return None;
    }
    let time = disclosure.time.as_deref().and_then(|s| {
        let (h, tail) = s.split_once(':')?;
        let (m, sec) = tail.split_once(':').unwrap_or((tail, "00"));
        if h.len() == 2
            && m.len() == 2
            && sec.len() == 2
            && h.parse::<u8>().ok()? < 24
            && m.parse::<u8>().ok()? < 60
            && sec.parse::<u8>().ok()? < 60
        {
            Some(format!("{h}:{m}:{sec}"))
        } else {
            None
        }
    });
    Some(ForecastReport {
        code,
        // 同じ開示時刻・対象期にサマリーの値もあれば、構造化データを優先する。
        doc_id: format!("0-pdf-{}", disclosure.doc_id),
        disclosed,
        fiscal_year_end: Some(primary.end),
        dps: Some(primary.amount),
        reit: ForecastDetails {
            start: primary.start,
            other,
            disclosed_time: time,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disclosure() -> Disclosure {
        Disclosure {
            code: "1234".into(),
            doc_id: "20260617123456".into(),
            disclosed: "2026-06-17".into(),
            time: Some("15:30".into()),
        }
    }

    #[test]
    fn results_match_wrapped_period_dates_and_exclude_excess_distribution() {
        let text = "2026年4月期 決算短信（ＲＥＩＴ）
2026年6月17日
コ ー ド 番 号 1234
２．2026年10月期（2026年5月1日～2026年10月31日）及び2027年4月期（2026年11
月1日～2027年4月30日）の運用状況の予想
営業収益 営業利益 経常利益 当期純利益 分配金 利益超過分配金
百万円 ％ 百万円 ％ 百万円 ％ 百万円 ％ 円 円 円
2026年10月期 4,000 1.0 3,000 1.0 2,000 1.0 2,000 1.0 1,390 1,290 100
2027年 4月期 4,100 1.0 3,100 1.0 2,100 1.0 2,100 1.0 1,300 1,225 75";
        let report = forecast_report(&disclosure(), text).unwrap();
        assert_eq!(report.dps, Some(1290.0));
        assert_eq!(report.reit.disclosed_time.as_deref(), Some("15:30:00"));
        let next = report.reit.other.unwrap();
        assert_eq!(next.start, Date::parse("2026-11-01"));
        assert_eq!(next.end, Date::new(2027, 4, 30));
        assert_eq!(next.amount, 1225.0);
        assert!(forecast_report(
            &disclosure(),
            &text.replace("1,300 1,225 75", "1,301 1,225 75")
        )
        .is_none());
        assert!(forecast_report(&disclosure(), &text.replace("号 1234", "号 1235")).is_none());
    }

    #[test]
    fn revisions_use_the_new_amount_and_its_explicit_period() {
        let text = "2026年6月17日
（コード番号 1234）
2027年4月期の運用状況および分配予想の修正のお知らせ
対象（2026年11月1日～2027年4月30日）
営業収益 営業利益 経常利益 当期純利益 1口当たり分配金 1口当たり利益超過分配金
（百万円） （百万円） （百万円） （百万円） （円） （円）
前回発表予想（A） 9,000 5,000 3,000 3,000 1,225 0
今回修正予想（B） 9,100 5,100 3,100 3,100 1,300 0
増減額（B-A） 100 100 100 100 75 -";
        let report = forecast_report(&disclosure(), text).unwrap();
        assert_eq!(report.dps, Some(1300.0));
        assert_eq!(report.fiscal_year_end, Date::parse("2027-04-30"));
    }
}
