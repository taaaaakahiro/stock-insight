use super::*;
use domain::reit_distribution::{complete_actual_year, complete_forecast_year, year_from as year};
use domain::split::{distribution_unit_basis, Split};

const JQ_FIN: &str = "https://jpx-jquants.com/ja/spec/fin-summary";
const JQ_PRICE: &str = "https://jpx-jquants.com/ja/spec/eq-bars-daily";
pub(super) const EDINET: &str = "https://disclosure2dl.edinet-fsa.go.jp/searchdocument/pdf/";
const SECTORS: &str = include_str!("../../../../data/reit_sectors.csv");

pub(super) fn unit(key: &str) -> Option<&'static str> {
    if let Some(m) = METRICS.iter().find(|m| {
        m.key == key
            && matches!(
                key,
                "occupancy"
                    | "floating_ratio"
                    | "maturity_24m"
                    | "noi_diversification"
                    | "noi_concentration"
            )
    }) {
        return Some(m.unit);
    }
    Some(match key {
        "actual_dpu" | "forecast_dpu" | "one_off_dpu" | "ffo_per_unit" | "nav_per_unit" => {
            "JPY/unit"
        }
        "debt"
        | "assets"
        | "net_assets"
        | "property_fair_value"
        | "property_book_value"
        | "normalized_affo"
        | "normalized_payout"
        | "same_noi"
        | "same_noi_base"
        | "annual_noi"
        | "annual_interest"
        | "annual_ebitda"
        | "period_interest"
        | "period_noi"
        | "period_ebitda"
        | "swap_notional"
        | "fixed_maturing_12m"
        | "fixed_maturing_24m"
        | "net_income"
        | "depreciation"
        | "property_sale_gain"
        | "property_sale_loss" => "JPY",
        "ffo_units" => "units",
        "remaining_years" => "years",
        "issuer_ltv" => "ratio",
        _ => return None,
    })
}

pub(super) fn valid_raw_value(key: &str, value: f64) -> bool {
    value.is_finite()
        && match key {
            "net_income" | "normalized_affo" | "annual_ebitda" | "period_ebitda" | "period_noi" => {
                true
            }
            "ffo_per_unit" => true,
            "nav_per_unit" | "ffo_units" => value > 0.0,
            _ => rules::valid_value(key, value),
        }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ValueWithSource {
    pub(super) value: f64,
    pub(super) valid: bool,
    pub(super) sources: Vec<Observation>,
}

impl ValueWithSource {
    fn observation(o: &Observation) -> Option<Self> {
        Some(Self {
            value: o.metric_value?,
            valid: o.trusted(),
            sources: vec![o.clone()],
        })
    }

    fn derive(value: f64, inputs: &[&Self]) -> Option<Self> {
        let mut sources: Vec<_> = inputs
            .iter()
            .flat_map(|v| v.sources.iter().cloned())
            .collect();
        sources.sort_by(|a, b| {
            (&a.metric_key, &a.fiscal_period_end, &a.revision_id).cmp(&(
                &b.metric_key,
                &b.fiscal_period_end,
                &b.revision_id,
            ))
        });
        sources.dedup_by(|a, b| {
            a.metric_key == b.metric_key
                && a.fiscal_period_end == b.fiscal_period_end
                && a.revision_id == b.revision_id
        });
        value.is_finite().then(|| Self {
            value,
            valid: inputs.iter().all(|v| v.valid),
            sources,
        })
    }

    fn split_adjusted(mut self, splits: &[Split], day: Date) -> Self {
        let basis = Date::parse(&self.sources[0].as_of_date).expect("validated basis");
        for split in splits.iter().filter(|s| basis < s.date && s.date <= day) {
            self.value *= split.factor;
            let source = &self.sources[0];
            self.sources.push(make(
                &source.ticker,
                "split_factor",
                Some(split.factor),
                "ratio",
                None,
                split.date,
                split.date,
                &split.date.to_string(),
                &split.date.to_string(),
                0,
                JQ_PRICE,
            ));
        }
        self
    }

    fn stale(mut self) -> Self {
        self.valid = false;
        for source in &mut self.sources {
            source.quality_flags.push("stale".into());
        }
        self
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn make(
    ticker: &str,
    key: &str,
    value: Option<f64>,
    unit: &str,
    start: Option<Date>,
    end: Date,
    basis: Date,
    published: &str,
    id: &str,
    retrieved_at: i64,
    url: &str,
) -> Observation {
    Observation {
        ticker: ticker.into(),
        metric_key: key.into(),
        metric_value: value,
        unit: unit.into(),
        period_start: start.map(|d| d.to_string()),
        period_end: end.to_string(),
        fiscal_period_end: end.to_string(),
        as_of_date: basis.to_string(),
        source_url: url.into(),
        source_document_id: (!id.is_empty()).then(|| id.into()),
        source_page: None,
        disclosure_published_at: published.into(),
        retrieved_at,
        data_kind: if key == "forecast_dpu" {
            "forecast"
        } else {
            "actual"
        }
        .into(),
        method_version: "source-v1".into(),
        normalization_flags: vec![],
        quality_flags: vec![],
        revision_id: id.into(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn jq_actual(
    ticker: &str,
    value: Option<f64>,
    start: Option<Date>,
    end: Date,
    disclosed: Date,
    time: Option<&str>,
    doc: &str,
    now: i64,
) -> Option<Observation> {
    let day = disclosed.to_string();
    let published = time
        .map(|t| format!("{day}T{t}"))
        .filter(|s| publication_key(s).is_some())
        .unwrap_or(day);
    // Each historical value uses the fiscal period's units, including a later correction.
    let o = make(
        ticker,
        "actual_dpu",
        value.filter(|v| *v >= 0.0 && v.is_finite()),
        "JPY/unit",
        start,
        end,
        distribution_unit_basis(end),
        &published,
        doc,
        now,
        JQ_FIN,
    );
    o.validate().then_some(o)
}

pub(crate) fn pdf_forecast(
    report: &usecase::fins::ForecastReport,
    period: domain::reit_distribution::ForecastPeriod,
    page: u32,
    now: i64,
) -> Option<Observation> {
    let doc = report.doc_id.strip_prefix("0-pdf-")?;
    let day = report.disclosed.to_string();
    let published = report
        .reit
        .disclosed_time
        .as_deref()
        .map(|t| format!("{day}T{t}"))
        .filter(|s| publication_key(s).is_some())
        .unwrap_or(day);
    let mut o = make(
        report.code.as_str(),
        "forecast_dpu",
        Some(period.amount),
        "JPY/unit",
        period.start,
        period.end,
        report.disclosed.min(distribution_unit_basis(period.end)),
        &published,
        &report.doc_id,
        now,
        "https://jpx-jquants.com/ja/spec/tdnet",
    );
    o.source_document_id = Some(doc.into());
    o.source_page = Some(page);
    o.method_version = "tdnet-pdf-v1".into();
    o.validate().then_some(o)
}

fn row_str<'a>(r: &'a Value, key: &str) -> Option<&'a str> {
    r[key].as_str().filter(|s| !s.is_empty())
}
fn row_date(r: &Value, key: &str) -> Option<Date> {
    strict_date(row_str(r, key)?)
}
fn row_time(r: &Value, day: Date) -> String {
    let date = day.to_string();
    row_str(r, "reit_disclosed_time")
        .map(|s| format!("{date}T{s}"))
        .filter(|s| publication_key(s).is_some())
        .unwrap_or(date)
}

fn latest(
    observations: Vec<Observation>,
    day: Date,
) -> BTreeMap<String, BTreeMap<Date, Observation>> {
    let mut periods = BTreeMap::<String, BTreeMap<Date, Observation>>::new();
    for obs in observations
        .into_iter()
        .filter(|o| o.validate() && o.disclosed() <= day)
    {
        let entry = periods
            .entry(obs.metric_key.clone())
            .or_default()
            .entry(obs.end());
        match entry {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(obs);
            }
            std::collections::btree_map::Entry::Occupied(mut v)
                if obs.order() > v.get().order() =>
            {
                v.insert(obs);
            }
            _ => {}
        }
    }
    periods
}

fn actual_window(periods: &BTreeMap<Date, Observation>, end: Date) -> Option<Vec<&Observation>> {
    let dates: Vec<_> = periods
        .range(..=end)
        .map(|(&d, o)| (d, o.period_start.as_deref().and_then(strict_date)))
        .collect();
    if !complete_actual_year(&dates, end) {
        return None;
    }
    Some(
        periods
            .range((
                std::ops::Bound::Excluded(year(end, -1)),
                std::ops::Bound::Included(end),
            ))
            .map(|(_, o)| o)
            .collect(),
    )
}

fn forecasts(
    periods: Option<&BTreeMap<Date, Observation>>,
    values: &mut Values,
    day: Date,
    price: Option<&ValueWithSource>,
    splits: &[Split],
) -> (Value, Option<ValueWithSource>) {
    // A known price is the yield basis; on non-trading days don't incorporate forecasts
    // disclosed after that close's calendar date into the price's yield.
    let cutoff = price
        .and_then(|v| strict_date(&v.sources[0].period_end))
        .unwrap_or(day);
    let future: Vec<_> = periods
        .into_iter()
        .flat_map(|p| p.values())
        .filter(|o| o.end() > cutoff && o.end() <= year(cutoff, 1) && o.disclosed() <= cutoff)
        .filter_map(|o| ValueWithSource::observation(o).filter(|v| v.valid && v.value >= 0.0))
        .map(|v| v.split_adjusted(splits, cutoff))
        .collect();
    let periods: Vec<_> = future
        .iter()
        .map(|v| {
            let source = &v.sources[0];
            (
                source.end(),
                source.period_start.as_deref().and_then(strict_date),
            )
        })
        .collect();
    let complete = complete_forecast_year(&periods);
    let total = sum(&future);
    let yield_ = if complete {
        total.as_ref().zip(price).and_then(|(v, p)| divide(v, p))
    } else {
        None
    };
    let summary = json!({"status":if complete { "annual" } else if total.is_some() { "partial" } else { "missing" },
        "dpu":total.as_ref().map(|v|v.value),"period_count":future.len()});
    insert(values, "forecast_dpu", total);
    (summary, yield_)
}

fn sum(values: &[ValueWithSource]) -> Option<ValueWithSource> {
    if values.is_empty() {
        return None;
    }
    let refs: Vec<_> = values.iter().collect();
    ValueWithSource::derive(values.iter().map(|v| v.value).sum(), &refs)
}

fn divide(a: &ValueWithSource, b: &ValueWithSource) -> Option<ValueWithSource> {
    ValueWithSource::derive(rules::ratio(a.value, b.value)?, &[a, b])
}
fn growth(a: &ValueWithSource, b: &ValueWithSource, years: i32) -> Option<ValueWithSource> {
    ValueWithSource::derive(rules::cagr(a.value, b.value, years)?, &[a, b])
}
fn difference(a: &ValueWithSource, b: &ValueWithSource) -> Option<ValueWithSource> {
    ValueWithSource::derive(a.value - b.value, &[a, b])
}

type Periods = BTreeMap<String, BTreeMap<Date, Observation>>;
type Values = BTreeMap<&'static str, ValueWithSource>;

fn at(p: &Periods, key: &str, date: Date) -> Option<ValueWithSource> {
    ValueWithSource::observation(p.get(key)?.get(&date)?)
}

fn annual_money(p: &Periods, key: &str, end: Date) -> Option<ValueWithSource> {
    let periods = p.get(key)?;
    let rows = actual_window(periods, end)?;
    // Legacy imports may omit the start date. An annual money figure must
    // prove the individual statement periods, including their exact boundaries.
    let values = rows
        .into_iter()
        .map(|row| {
            row.period_start.as_ref()?;
            ValueWithSource::observation(row)
        })
        .collect::<Option<Vec<_>>>()?;
    sum(&values)
}

fn ffo(p: &Periods, end: Date, splits: &[Split], basis: Date) -> Option<ValueWithSource> {
    if let Some(v) = at(p, "ffo_per_unit", end) {
        return Some(v.split_adjusted(splits, basis));
    }
    let window = p
        .get("net_income")
        .filter(|periods| periods.get(&end).is_some_and(|o| o.period_start.is_some()))
        .and_then(|periods| actual_window(periods, end))
        .or_else(|| actual_window(p.get("actual_dpu")?, end))?;
    let periods: Option<Vec<_>> = window
        .iter()
        .map(|o| {
            let end = o.end();
            let net = at(p, "net_income", end)?;
            let dep = at(p, "depreciation", end)?;
            let gain = at(p, "property_sale_gain", end)?;
            let loss = at(p, "property_sale_loss", end)?;
            let units = at(p, "ffo_units", end)?;
            // Period components must match the exact statement window, if available.
            let start = o.period_start.as_ref();
            if [&net, &dep, &gain, &loss, &units].iter().any(|v| {
                v.sources[0]
                    .period_start
                    .as_ref()
                    .zip(start)
                    .is_some_and(|(actual, expected)| actual != expected)
            }) {
                return None;
            }
            if [dep.value, gain.value, loss.value].iter().any(|&v| v < 0.0) {
                return None;
            }
            let mut v = ValueWithSource::derive(
                rules::ratio(net.value + dep.value - gain.value + loss.value, units.value)?,
                &[&units, &net, &dep, &gain, &loss],
            )?;
            let unit_source = v.sources.iter().position(|s| s.metric_key == "ffo_units")?;
            v.sources.swap(0, unit_source);
            Some(v.split_adjusted(splits, basis))
        })
        .collect();
    sum(&periods?)
}

fn annual(
    p: &Periods,
    end: Date,
    splits: &[Split],
    basis: Date,
) -> (Option<ValueWithSource>, Option<ValueWithSource>) {
    let Some(window) = p.get("actual_dpu").and_then(|p| actual_window(p, end)) else {
        return (None, None);
    };
    let actuals: Option<Vec<_>> = window
        .iter()
        .map(|o| {
            ValueWithSource::observation(o)
                .filter(|v| v.valid && v.value >= 0.0)
                .map(|v| v.split_adjusted(splits, basis))
        })
        .collect();
    let total = actuals.as_deref().and_then(sum);
    let one_offs: Option<Vec<_>> = window
        .iter()
        .zip(actuals.as_deref().unwrap_or_default())
        .map(|(o, actual)| {
            let v = at(p, "one_off_dpu", o.end())?.split_adjusted(splits, basis);
            (v.value >= 0.0 && v.value <= actual.value).then_some(v)
        })
        .collect();
    (total, one_offs.as_deref().and_then(sum))
}

fn insert(values: &mut Values, key: &'static str, value: Option<ValueWithSource>) {
    if let Some(v) = value {
        if v.value.is_finite() {
            values.insert(key, v);
        }
    }
}

fn distribution(
    p: &Periods,
    values: &mut Values,
    day: Date,
    splits: &[Split],
    price: Option<&ValueWithSource>,
) -> (Vec<Value>, Value, bool) {
    // Historical forecast revisions are selected at the price date, independently of actuals.
    let cutoff = price
        .and_then(|v| strict_date(&v.sources[0].period_end))
        .unwrap_or(day);
    let Some(actuals) = p.get("actual_dpu") else {
        let (forecast, y) = forecasts(p.get("forecast_dpu"), values, day, price, splits);
        insert(values, "forecast_yield", y);
        return (vec![], forecast, false);
    };
    let latest = actuals
        .values()
        .rev()
        .find(|o| o.disclosed() <= cutoff)
        .map(Observation::end);
    let mut history = Vec::new();
    // Use the same fiscal month once per year. The sixth point is the 5-year CAGR base.
    if let Some(end) = latest {
        for offset in (-5..=0).rev() {
            let period = year(end, offset);
            let (dpu, one_off) = annual(p, period, splits, cutoff);
            let normalized = dpu
                .as_ref()
                .zip(one_off.as_ref())
                .and_then(|(d, o)| difference(d, o));
            let ffo = ffo(p, period, splits, cutoff);
            history.push(json!({"period_end":period.to_string(),"actual_dpu":dpu.as_ref().map(|v|v.value),
                "normalized_dpu":normalized.as_ref().filter(|v|v.valid).map(|v|v.value),
                "ffo_per_unit":ffo.as_ref().filter(|v|v.valid).map(|v|v.value),
                "source_ref":{"actual_dpu":dpu.as_ref().map(|v|&v.sources),
                  "normalized_dpu":normalized.as_ref().map(|v|&v.sources),"ffo_per_unit":ffo.as_ref().map(|v|&v.sources)}}));
            if offset == 0 {
                if let Some(v) = dpu {
                    let v = if cutoff.to_days() - end.to_days() > rules::DISCLOSURE_MAX_AGE {
                        v.stale()
                    } else {
                        v
                    };
                    insert(values, "actual_yield", price.and_then(|p| divide(&v, p)));
                    insert(
                        values,
                        "sale_dependence",
                        one_off.as_ref().and_then(|o| divide(o, &v)),
                    );
                    values.insert("actual_dpu", v);
                }
                insert(values, "normalized_dpu", normalized);
                insert(values, "ffo_per_unit", ffo);
            } else if offset == -5 {
                insert(
                    values,
                    "dpu_cagr",
                    values
                        .get("actual_dpu")
                        .zip(dpu.as_ref())
                        .and_then(|(a, b)| growth(a, b, 5)),
                );
                insert(
                    values,
                    "normalized_dpu_cagr",
                    values
                        .get("normalized_dpu")
                        .zip(normalized.as_ref())
                        .and_then(|(a, b)| growth(a, b, 5)),
                );
                insert(
                    values,
                    "ffo_cagr",
                    values
                        .get("ffo_per_unit")
                        .zip(ffo.as_ref())
                        .and_then(|(a, b)| growth(a, b, 5)),
                );
            }
        }
    }
    history.reverse();
    let (forecast, y) = forecasts(p.get("forecast_dpu"), values, day, price, splits);
    insert(values, "forecast_yield", y);
    (
        history,
        forecast,
        values.get("actual_dpu").is_some_and(|v| v.valid),
    )
}

fn finance(
    p: &Periods,
    values: &mut Values,
    price: Option<&ValueWithSource>,
    splits: &[Split],
    day: Date,
) -> bool {
    let financial_anchor = |k: &str| {
        matches!(
            k,
            "debt"
                | "assets"
                | "net_assets"
                | "property_fair_value"
                | "property_book_value"
                | "nav_per_unit"
                | "occupancy"
                | "annual_noi"
                | "annual_interest"
                | "period_interest"
                | "period_noi"
                | "period_ebitda"
                | "normalized_payout"
                | "same_noi"
        )
    };
    let dates = p
        .iter()
        .filter(|(k, _)| financial_anchor(k))
        .flat_map(|(_, v)| v.keys().copied());
    let Some(end) = dates.max() else {
        return false;
    };
    let read = |key| {
        if let Some(observed) = p.get(key).and_then(|periods| periods.get(&end)) {
            return ValueWithSource::observation(observed);
        }
        let period_key = match key {
            "annual_noi" => "period_noi",
            "annual_interest" => "period_interest",
            "annual_ebitda" => "period_ebitda",
            _ => return None,
        };
        annual_money(p, period_key, end)
    };
    // As-of alignment: no mixing debt, NOI, and other financial inputs from different fiscal ends.
    for key in [
        "debt",
        "floating_ratio",
        "maturity_24m",
        "remaining_years",
        "swap_notional",
        "issuer_ltv",
        "occupancy",
        "same_noi",
        "noi_diversification",
        "noi_concentration",
        "annual_noi",
        "annual_interest",
        "normalized_payout",
    ] {
        insert(values, key, read(key));
    }
    let divide_pair = |n, d| divide(&read(n)?, &read(d)?);
    insert(values, "ltv", divide_pair("debt", "assets"));
    insert(
        values,
        "interest_cover",
        divide_pair("annual_ebitda", "annual_interest"),
    );
    insert(
        values,
        "interest_noi",
        divide_pair("annual_interest", "annual_noi"),
    );
    insert(
        values,
        "affo_coverage",
        divide_pair("normalized_affo", "normalized_payout"),
    );
    if let (Some(a), Some(b)) = (read("same_noi"), read("same_noi_base")) {
        let (a_source, b_source) = (&a.sources[0], &b.sources[0]);
        if a_source.period_start.as_deref().and_then(strict_date) == Some(year(end, -3))
            && a_source.period_start == b_source.period_start
            && a_source.revision_id == b_source.revision_id
            && a_source.source_url == b_source.source_url
            && a_source.source_document_id == b_source.source_document_id
        {
            insert(values, "same_noi_cagr", growth(&a, &b, 3));
        }
    }
    if let Some(gain) = read("property_fair_value")
        .zip(read("property_book_value"))
        .and_then(|(a, b)| difference(&a, &b))
    {
        insert(
            values,
            "unrealized_equity",
            read("net_assets").and_then(|b| divide(&gain, &b)),
        );
        values.insert("unrealized", gain);
    }
    let basis = price
        .and_then(|v| strict_date(&v.sources[0].period_end))
        .unwrap_or(day);
    let nav = read("nav_per_unit").map(|v| v.split_adjusted(splits, basis));
    let old_nav = at(p, "nav_per_unit", year(end, -3)).map(|v| v.split_adjusted(splits, basis));
    insert(
        values,
        "nav_cagr",
        nav.as_ref()
            .zip(old_nav.as_ref())
            .and_then(|(a, b)| growth(a, b, 3)),
    );
    // Historical yields and valuation cannot use documents issued after the price's date.
    insert(
        values,
        "p_nav",
        price
            .zip(nav.as_ref())
            .filter(|(_, n)| n.sources.iter().all(|s| s.disclosed() <= basis))
            .and_then(|(p, n)| divide(p, n)),
    );
    insert(values, "nav_per_unit", nav);
    insert(
        values,
        "p_ffo",
        price
            .zip(values.get("ffo_per_unit"))
            .and_then(|(p, f)| divide(p, f)),
    );
    if let (Some(debt), Some(floating), Some(payout)) = (
        read("debt"),
        read("floating_ratio"),
        read("normalized_payout"),
    ) {
        for (key, shock) in [
            ("stress_50", 0.005),
            ("stress_100", 0.01),
            ("stress_200", 0.02),
        ] {
            if let Some(v) = rules::interest_stress(debt.value, floating.value, shock, payout.value)
            {
                insert(
                    values,
                    key,
                    ValueWithSource::derive(v, &[&debt, &floating, &payout]),
                );
                for (name, horizon) in match key {
                    "stress_50" => [
                        ("stress_50_y1", "fixed_maturing_12m"),
                        ("stress_50_y2", "fixed_maturing_24m"),
                    ],
                    "stress_100" => [
                        ("stress_100_y1", "fixed_maturing_12m"),
                        ("stress_100_y2", "fixed_maturing_24m"),
                    ],
                    _ => [
                        ("stress_200_y1", "fixed_maturing_12m"),
                        ("stress_200_y2", "fixed_maturing_24m"),
                    ],
                } {
                    if let Some(fixed) = read(horizon).filter(|f| {
                        f.value >= 0.0 && f.value <= debt.value * (1.0 - floating.value)
                    }) {
                        insert(
                            values,
                            name,
                            ValueWithSource::derive(
                                v + fixed.value * shock / payout.value,
                                &[&debt, &floating, &payout, &fixed],
                            ),
                        );
                    }
                }
            }
        }
    }
    day.to_days() - end.to_days() <= rules::DISCLOSURE_MAX_AGE
        && p.iter().any(|(k, dates)| {
            financial_anchor(k)
                && dates
                    .get(&end)
                    .is_some_and(|o| o.trusted() && o.metric_value.is_some())
        })
}

fn build(
    ticker: String,
    name: String,
    sector: String,
    observations: Vec<Observation>,
    price: Option<Observation>,
    splits: Vec<Split>,
    day: Date,
) -> Issuer {
    let price = price
        .as_ref()
        .and_then(ValueWithSource::observation)
        .map(|v| {
            if day.to_days() - v.sources[0].end().to_days() > rules::PRICE_MAX_AGE {
                v.stale()
            } else {
                v
            }
        });
    let cutoff = price.as_ref().map_or(day, |v| v.sources[0].end());
    let for_price = latest(observations.clone(), cutoff);
    let periods = latest(observations, day);
    let mut values = BTreeMap::new();
    let (history, forecast, actual_ready) =
        distribution(&for_price, &mut values, day, &splits, price.as_ref());
    let finance_ready = finance(&periods, &mut values, price.as_ref(), &splits, day);
    let mut provisional = Vec::new();
    if !price.as_ref().is_some_and(|v| v.valid) {
        provisional.push("missing_or_stale_price");
    }
    if !actual_ready {
        provisional.push("incomplete_or_stale_distribution");
    }
    if !finance_ready {
        provisional.push("missing_or_stale_disclosure");
    }
    for (key, v) in &mut values {
        if METRICS.iter().any(|m| m.key == *key) && !rules::valid_value(key, v.value) {
            v.valid = false;
            for s in &mut v.sources {
                s.quality_flags.push("invalid_value".into());
            }
        }
        let newest = v
            .sources
            .iter()
            .filter(|s| !matches!(s.metric_key.as_str(), "price" | "split_factor"))
            .map(Observation::end)
            .max();
        if newest.is_some_and(|end| day.to_days() - end.to_days() > rules::DISCLOSURE_MAX_AGE) {
            *v = v.clone().stale();
        }
    }
    insert(&mut values, "price", price);
    Issuer {
        ticker,
        name,
        sector,
        values,
        provisional,
        history,
        forecast,
    }
}

const JPX_REITS: &str = "https://www.jpx.co.jp/equities/products/reits/issues/index.html";

fn parse_listed_reits(html: &str, day: Date) -> Result<BTreeMap<String, String>, PortError> {
    use domain::reit::Block;
    let mut issuers = BTreeMap::new();
    let mut matched = false;
    for block in crate::ixbrl::blocks(html) {
        let Block::Table(rows, spans) = block else {
            continue;
        };
        let Some(header) = rows.iter().find(|r| {
            r.iter().any(|c| c.contains("上場日")) && r.iter().any(|c| c.contains("ISIN"))
        }) else {
            continue;
        };
        matched = true;
        let col = |s| header.iter().position(|c| c.contains(s));
        let (Some(date_col), Some(name_col), Some(code_col)) =
            (col("上場日"), col("銘柄名"), col("ISIN"))
        else {
            continue;
        };
        for (i, row) in rows.iter().enumerate() {
            if spans
                .get(i)
                .and_then(|r| r.get(date_col))
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            let Some(date) = row
                .get(date_col)
                .and_then(|s| strict_date(&s.replace('/', "-")))
            else {
                continue;
            };
            if date > day {
                continue;
            }
            let code = row
                .get(code_col)
                .and_then(|s| s.get(..4))
                .and_then(StockCode::parse)
                .ok_or_else(|| PortError("JPX REIT table: unrecognized code".into()))?;
            let name = row
                .get(name_col)
                .map(|s| s.trim_end_matches("投資証券").trim())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| PortError("JPX REIT table: missing name".into()))?;
            if issuers.insert(code.as_str().into(), name.into()).is_some() {
                return Err(PortError("JPX REIT table: duplicate code".into()));
            }
        }
    }
    if !matched || issuers.is_empty() {
        return Err(PortError(
            "JPX REIT list is empty or its format changed".into(),
        ));
    }
    Ok(issuers)
}

pub(super) async fn listed_reits(day: Date) -> Result<BTreeMap<String, String>, PortError> {
    let body = crate::http::get_bytes(&crate::http::client(), JPX_REITS).await?;
    parse_listed_reits(
        std::str::from_utf8(&body).map_err(|e| PortError(e.to_string()))?,
        day,
    )
}

pub(super) async fn load(store: &D1Store, day: Date) -> Result<Vec<Issuer>, PortError> {
    let master = listed_reits(day).await?;
    let sectors = domain::reit::parse_sectors(SECTORS);
    let limit = format!("{day}T23:59:59");
    let audits=store.query(&stmt(
        "SELECT body FROM reit_rag_document_events WHERE listed_on<=? AND \
         (body->>'docTypeCode'='130' OR coalesce(body->>'withdrawalStatus','0')<>'0' \
          OR coalesce(body->>'disclosureStatus','0')<>'0' OR coalesce(body->>'docInfoEditStatus','0')<>'0')",
        vec![t(day.to_string())])).await?.into_iter().filter_map(|r| serde_json::from_str(r["body"].as_str()?).ok()).collect::<Vec<Value>>();
    let mut issuers = Vec::with_capacity(master.len());
    for (ticker, name) in master {
        let code = ticker.as_str();
        let raw=store.query(&stmt(
            "SELECT (SELECT closes FROM price_history WHERE code=?1) AS closes,\
             (SELECT updated_at FROM price_history WHERE code=?1) AS prices_retrieved_at,\
             (SELECT json_group_array(json_object('date',date,'factor',factor)) FROM splits WHERE code=?1 AND date<=?2) AS splits,\
             (SELECT json_group_array(json_object('period_end',period_end,'dps',dps,'doc_id',doc_id,'disclosed',disclosed,'fetched_at',fetched_at)) \
                 FROM financials_annual WHERE code=?1 AND disclosed<=?2 AND period_end<=?2) AS actuals,\
             (SELECT json_group_array(json_object('fiscal_year_end',fiscal_year_end,'dps',dps,'doc_id',doc_id,'disclosed',disclosed,'fetched_at',fetched_at,\
                 'reit_period_start',reit_period_start,'reit_other_period_start',reit_other_period_start,\
                 'reit_other_period_end',reit_other_period_end,'reit_other_dps',reit_other_dps,'reit_disclosed_time',reit_disclosed_time)) \
                 FROM dividend_forecasts WHERE code=?1 AND disclosed<=?2 AND max(coalesce(fiscal_year_end,''),coalesce(reit_other_period_end,''))>=?3) AS forecasts",
            vec![t(code),t(day.to_string()),t(day.add_days(-rules::PRICE_MAX_AGE).to_string())])).await?;
        let raw = raw
            .first()
            .ok_or_else(|| PortError("REIT inputs not returned".into()))?;
        let price = raw["closes"]
            .as_str()
            .and_then(|s| {
                domain::price_history::decode(s)
                    .into_iter()
                    .filter(|b| b.date <= day && b.close.is_finite() && b.close > 0.0)
                    .max_by_key(|b| b.date)
            })
            .map(|b| {
                make(
                    code,
                    "price",
                    Some(b.close),
                    "JPY/unit",
                    None,
                    b.date,
                    b.date,
                    &b.date.to_string(),
                    &b.date.to_string(),
                    raw["prices_retrieved_at"].as_i64().unwrap_or_default(),
                    JQ_PRICE,
                )
            });
        let parse = |key: &str| {
            serde_json::from_str::<Vec<Value>>(raw[key].as_str().unwrap_or("[]"))
                .map_err(serialization)
        };
        let splits = parse("splits")?
            .iter()
            .filter_map(|r| Split::new(row_date(r, "date")?, r["factor"].as_f64()?))
            .collect();
        let mut observations=store.query(&stmt(
            "SELECT * FROM reit_metric_observations WHERE ticker=? AND disclosure_published_at<=? AND fiscal_period_end>=? \
             ORDER BY metric_key,fiscal_period_end,disclosure_published_at,revision_id",
            vec![t(code),t(&limit),t(year(day,-7).to_string())])).await?.into_iter().map(|mut r| {
                for field in ["normalization_flags","quality_flags"] {
                    r[field]=serde_json::from_str(r[field].as_str().unwrap_or("[]")).map_err(serialization)?;
                }
                serde_json::from_value::<Observation>(r).map_err(serialization)
            }).collect::<Result<Vec<_>,_>>()?;
        for obs in &mut observations {
            if obs.source_url.starts_with(EDINET)
                && obs
                    .source_document_id
                    .as_deref()
                    .is_some_and(|doc| crate::reit_rag::document_issue(doc, &audits).is_some())
            {
                obs.quality_flags.push("disclosure_under_review".into());
            }
        }
        let recorded: BTreeSet<_> = observations
            .iter()
            .filter(|o| o.metric_key == "actual_dpu" && o.validate())
            .map(|o| (o.end(), o.revision_id.clone()))
            .collect();
        for r in parse("actuals")? {
            let (Some(end), Some(disclosed), Some(doc)) = (
                row_date(&r, "period_end"),
                row_date(&r, "disclosed"),
                row_str(&r, "doc_id"),
            ) else {
                continue;
            };
            if !recorded.contains(&(end, doc.to_owned())) {
                observations.extend(jq_actual(
                    code,
                    r["dps"].as_f64(),
                    None,
                    end,
                    disclosed,
                    None,
                    doc,
                    r["fetched_at"].as_i64().unwrap_or_default(),
                ));
            }
        }
        for r in parse("forecasts")? {
            let (Some(day), Some(doc)) = (row_date(&r, "disclosed"), row_str(&r, "doc_id")) else {
                continue;
            };
            // PDF values are sourced from page-verified observations, not the legacy cache.
            if doc.starts_with("0-pdf-") {
                continue;
            }
            let published = row_time(&r, day);
            for (start, end, dpu) in [
                ("reit_period_start", "fiscal_year_end", "dps"),
                (
                    "reit_other_period_start",
                    "reit_other_period_end",
                    "reit_other_dps",
                ),
            ] {
                if let (Some(end), Some(amount)) = (row_date(&r, end), r[dpu].as_f64()) {
                    let o = make(
                        code,
                        "forecast_dpu",
                        Some(amount),
                        "JPY/unit",
                        row_date(&r, start),
                        end,
                        day.min(distribution_unit_basis(end)),
                        &published,
                        doc,
                        r["fetched_at"].as_i64().unwrap_or_default(),
                        JQ_FIN,
                    );
                    observations.push(o);
                }
            }
        }
        issuers.push(build(
            code.into(),
            name,
            sectors
                .get(code)
                .cloned()
                .unwrap_or_else(|| "unknown".into()),
            observations,
            price,
            splits,
            day,
        ));
    }
    Ok(issuers)
}

pub fn edinet_observations(
    record: &crate::edinet::ReitRecord,
    zip: &[u8],
    now: i64,
) -> Result<Vec<Observation>, PortError> {
    use domain::reit::RateType;
    let end = record.period_end;
    let docs = crate::ixbrl::public_docs(zip)?;
    let facts = crate::ixbrl::verified_reit_instant_facts(&docs, end);
    let fact = |names: &[&str]| {
        let mut values = facts
            .iter()
            .filter(|f| names.contains(&f.name.as_str()))
            .map(|f| f.value);
        let v = values.next()?;
        (v >= 0.0 && values.all(|x| x == v)).then_some(v)
    };
    let doc = &record.doc.doc_id;
    let url = format!("{EDINET}{doc}.pdf");
    let published = record.doc.submitted.to_string();
    let mut rows = Vec::new();
    let mut push = |key: &str, value: Option<f64>| {
        // Explicit missing values prevent silently using an older revision of this period.
        let o = make(
            &record.code,
            key,
            value,
            unit(key).unwrap_or_default(),
            None,
            end,
            end,
            &published,
            doc,
            now,
            &url,
        );
        if o.validate() {
            rows.push(o);
        }
    };
    let assets = fact(&["Assets", "TotalAssets"]);
    let net_assets = fact(&["NetAssets"]);
    let liabilities = fact(&["Liabilities", "TotalLiabilities"]);
    let balance_ok = assets
        .zip(net_assets)
        .zip(liabilities)
        .is_some_and(|((a, n), l)| a > 0.0 && (a - n - l).abs() <= a * 0.00001);
    let r = &record.report;
    let debt = r.balance.short_loans + r.balance.long_loans + r.balance.bonds;
    let loans_ok = matches!(record.checks.loans, "一致" | "端数");
    let debt_ok =
        balance_ok && loans_ok && liabilities.is_some_and(|l| debt >= 0 && debt as f64 <= l);
    push("assets", balance_ok.then_some(assets).flatten());
    push("net_assets", balance_ok.then_some(net_assets).flatten());
    push("debt", debt_ok.then_some(debt as f64));
    let property = r.assets.as_ref();
    push(
        "property_fair_value",
        property.map(|v| v.property_fair_value as f64),
    );
    push(
        "property_book_value",
        property.map(|v| v.property_book_value as f64),
    );
    push(
        "nav_per_unit",
        property.and_then(|v| v.nav_per_unit).filter(|&v| v > 0.0),
    );
    let occupancy_ok = matches!(record.checks.properties, "一致" | "端数")
        && r.properties
            .iter()
            .all(|p| p.leased_m2.is_some() || p.occupancy_pct.is_some());
    push(
        "occupancy",
        record
            .portfolio
            .occupancy_pct
            .filter(|_| occupancy_ok)
            .map(|v| v / 100.0),
    );
    push("swap_notional", r.debt.swap_yen.map(|x| x as f64));
    // The legacy mix assumes bonds are fixed; scoring needs explicit evidence.
    // Swap amount alone does not identify which borrowing is hedged.
    let floating_ok = debt_ok
        && r.balance.bonds == 0
        && record.mix.unknown == 0
        && record.mix.swap_amount_only == 0;
    push(
        "floating_ratio",
        floating_ok
            .then(|| rules::ratio(record.mix.floating as f64, debt as f64))
            .flatten(),
    );
    let due_ok = debt_ok && r.debt.loans.iter().all(|l| l.due.is_some_and(|d| d > end));
    push(
        "remaining_years",
        record
            .debt
            .avg_years
            .filter(|_| due_ok && r.balance.bonds == 0),
    );
    let bond_maturity = if r.balance.bonds == 0 {
        Some(0)
    } else {
        let bonds: Vec<_> = r
            .debt
            .ladder
            .iter()
            .filter(|(item, _)| item.contains("投資法人債"))
            .map(|(_, amounts)| amounts)
            .collect();
        let total: i64 = bonds.iter().flat_map(|row| row.iter()).sum();
        (total == r.balance.bonds && bonds.iter().flat_map(|row| row.iter()).all(|&n| n >= 0))
            .then(|| bonds.iter().map(|row| row[0] + row[1]).sum())
    };
    let maturing = |years, fixed_only: bool| {
        let total = r
            .debt
            .loans
            .iter()
            .filter(|l| {
                l.due.is_some_and(|d| d <= year(end, years))
                    && (!fixed_only || l.rate_type == Some(RateType::Fixed))
            })
            .map(|l| l.balance_yen)
            .sum::<i64>();
        total as f64
    };
    push(
        "maturity_24m",
        bond_maturity
            .filter(|_| due_ok)
            .and_then(|bonds| rules::ratio(maturing(2, false) + bonds as f64, debt as f64)),
    );
    // When hedge expiry/rollover is not established, only known fully fixed borrowings qualify.
    let fixed_ok = due_ok && floating_ok && record.mix.swap_fixed == 0;
    push("fixed_maturing_12m", fixed_ok.then(|| maturing(1, true)));
    push("fixed_maturing_24m", fixed_ok.then(|| maturing(2, true)));
    rows.extend(super::edinet_income::observations(record, &docs, now, &url));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        strict_date(s).unwrap()
    }
    fn obs(
        key: &str,
        value: f64,
        start: Option<&str>,
        end: &str,
        published: &str,
        id: &str,
    ) -> Observation {
        let basis = if key == "forecast_dpu" {
            Date::parse(published).unwrap()
        } else {
            d(end)
        };
        make(
            "8951",
            key,
            Some(value),
            unit(key).unwrap(),
            start.map(d),
            d(end),
            basis,
            published,
            id,
            0,
            JQ_FIN,
        )
    }
    fn actual(value: f64, start: &str, end: &str, published: &str, id: &str) -> Observation {
        obs("actual_dpu", value, Some(start), end, published, id)
    }
    fn price(day: Date) -> Observation {
        make(
            "8951",
            "price",
            Some(100_000.0),
            "JPY/unit",
            None,
            day,
            day,
            &day.to_string(),
            "price",
            0,
            JQ_PRICE,
        )
    }
    fn run(rows: Vec<Observation>, splits: Vec<Split>, date: &str) -> Issuer {
        let day = d(date);
        build(
            "8951".into(),
            "テスト投資法人".into(),
            "office".into(),
            rows,
            Some(price(day)),
            splits,
            day,
        )
    }

    #[test]
    fn jpx_universe_skips_future_listings_and_expands_rows() {
        let html = r#"<table>
          <tr><th>上場日</th><th>銘柄名</th><th>コード<br>（ISINコード）</th><th>運用会社</th></tr>
          <tr><td rowspan="2">2001/09/10</td><td rowspan="2">日本ビルファンド投資法人　投資証券</td><td rowspan="2"><a>8951</a>（JP3027670003）</td><td>概要等</td></tr>
          <tr><td>三井不動産</td></tr>
          <tr><td>2026/11/25</td><td>KDX Kolet投資法人　投資証券</td><td>666A（JP3051800005）</td></tr>
        </table>"#;
        assert_eq!(
            parse_listed_reits(html, d("2026-11-08")).unwrap(),
            BTreeMap::from([("8951".into(), "日本ビルファンド投資法人".into())])
        );
        assert_eq!(parse_listed_reits(html, d("2026-11-25")).unwrap().len(), 2);
        assert!(parse_listed_reits("<html>maintenance</html>", d("2026-11-25")).is_err());
    }

    #[test]
    fn annual_dpu_uses_fiscal_periods_and_latest_known_revision() {
        let first = actual(2500.0, "2025-04-01", "2025-09-30", "2025-11-15", "a");
        let second = actual(
            2600.0,
            "2025-10-01",
            "2026-03-31",
            "2026-05-15T15:30:00",
            "b",
        );
        let earlier_revision = actual(
            9999.0,
            "2025-10-01",
            "2026-03-31",
            "2026-05-15T14:30:00",
            "zz",
        );
        let correction = actual(2700.0, "2025-10-01", "2026-03-31", "2026-05-20", "c");
        let rows = vec![
            correction,
            second.clone(),
            earlier_revision,
            first,
            second.clone(),
        ];
        let before = run(rows.clone(), vec![], "2026-05-16");
        assert_eq!(before.values["actual_dpu"].value, 5100.0);
        assert_eq!(before.values["actual_yield"].value, 0.051);
        let split = Split::new(d("2026-05-18"), 0.5).unwrap();
        let after = run(rows, vec![split], "2026-05-21");
        assert_eq!(after.values["actual_dpu"].value, 2600.0);
        assert!(after.values["actual_dpu"]
            .sources
            .iter()
            .any(|s| s.metric_key == "split_factor"));
        let missing = run(vec![second], vec![], "2026-05-16");
        assert!(!missing.values.contains_key("actual_yield"));
    }

    #[test]
    fn forecasts_stay_separate_and_incomplete_forecasts_do_not_annualize() {
        let first = obs(
            "forecast_dpu",
            3100.0,
            Some("2026-04-01"),
            "2026-09-30",
            "2026-05-15",
            "a",
        );
        let second = obs(
            "forecast_dpu",
            2700.0,
            Some("2026-10-01"),
            "2027-03-31",
            "2026-05-16",
            "b",
        );
        let first_only = run(vec![first.clone(), second.clone()], vec![], "2026-05-15");
        assert_eq!(first_only.forecast["status"], "partial");
        assert_eq!(first_only.forecast["dpu"], 3100.0);
        assert!(!first_only.values.contains_key("forecast_yield"));
        let full = run(vec![first, second], vec![], "2026-05-16");
        assert_eq!(full.forecast["status"], "annual");
        assert_eq!(full.values["forecast_yield"].value, 0.058);
        assert!(!full.values.contains_key("actual_yield"));
    }

    #[test]
    fn one_off_gains_do_not_inflate_normalized_growth() {
        let mut rows = vec![];
        for (start, end, disclosed, amount, one_off) in [
            ("2020-04-01", "2020-09-30", "2020-11-15", 2500.0, 0.0),
            ("2020-10-01", "2021-03-31", "2021-05-15", 2500.0, 0.0),
            ("2025-04-01", "2025-09-30", "2025-11-15", 2500.0, 0.0),
            ("2025-10-01", "2026-03-31", "2026-05-15", 5000.0, 2500.0),
        ] {
            rows.push(actual(amount, start, end, disclosed, end));
            rows.push(obs(
                "one_off_dpu",
                one_off,
                Some(start),
                end,
                disclosed,
                end,
            ));
        }
        let result = run(rows.clone(), vec![], "2026-05-16");
        assert_eq!(result.values["normalized_dpu"].value, 5000.0);
        assert_eq!(result.values["normalized_dpu_cagr"].value, 0.0);
        assert!(result.values["dpu_cagr"].value > 0.0);
        rows.pop();
        assert!(!run(rows, vec![], "2026-05-16")
            .values
            .contains_key("normalized_dpu"));
    }

    #[test]
    fn only_same_cohort_noi_counts_and_stress_does_not_reprice_floating_twice() {
        let mut rows = vec![
            obs("annual_noi", 100.0, None, "2023-03-31", "2023-05-15", "old"),
            obs("annual_noi", 200.0, None, "2026-03-31", "2026-05-15", "new"),
            obs("debt", 1_000_000.0, None, "2026-03-31", "2026-05-15", "new"),
            obs(
                "normalized_payout",
                50_000.0,
                None,
                "2026-03-31",
                "2026-05-15",
                "new",
            ),
            obs(
                "floating_ratio",
                0.4,
                None,
                "2026-03-31",
                "2026-05-15",
                "new",
            ),
            obs(
                "fixed_maturing_12m",
                100_000.0,
                None,
                "2026-03-31",
                "2026-05-15",
                "new",
            ),
            obs(
                "fixed_maturing_24m",
                200_000.0,
                None,
                "2026-03-31",
                "2026-05-15",
                "new",
            ),
        ];
        let result = run(rows.clone(), vec![], "2026-05-16");
        assert!(!result.values.contains_key("same_noi_cagr"));
        assert_eq!(result.values["stress_100"].value, 0.08);
        assert_eq!(result.values["stress_100_y1"].value, 0.1);
        assert_eq!(result.values["stress_100_y2"].value, 0.12);
        rows.push(obs(
            "same_noi",
            110.0,
            Some("2023-03-31"),
            "2026-03-31",
            "2026-05-15",
            "cohort",
        ));
        rows.push(obs(
            "same_noi_base",
            100.0,
            Some("2023-03-31"),
            "2026-03-31",
            "2026-05-15",
            "cohort",
        ));
        let result = run(rows, vec![], "2026-05-16");
        assert_eq!(
            result.values["same_noi_cagr"].value,
            1.1_f64.powf(1.0 / 3.0) - 1.0
        );
    }

    #[test]
    fn annualizes_financial_statements_only_when_twelve_months_are_covered() {
        let mut rows = Vec::new();
        for (start, end, disclosed) in [
            ("2025-04-01", "2025-09-30", "2025-11-15"),
            ("2025-10-01", "2026-03-31", "2026-05-15"),
        ] {
            for (key, value) in [
                ("period_interest", 100.0),
                ("period_ebitda", 500.0),
                ("period_noi", 1000.0),
            ] {
                rows.push(obs(key, value, Some(start), end, disclosed, end));
            }
        }
        let issuer = run(rows.clone(), vec![], "2026-05-16");
        assert_eq!(issuer.values["interest_cover"].value, 5.0);
        assert_eq!(issuer.values["interest_noi"].value, 0.1);
        assert_eq!(issuer.values["annual_noi"].value, 2000.0);
        assert_eq!(issuer.values["interest_cover"].sources.len(), 4);
        rows.retain(|o| o.fiscal_period_end != "2025-09-30");
        let issuer = run(rows, vec![], "2026-05-16");
        assert!(!issuer.values.contains_key("interest_cover"));
        assert!(!issuer.values.contains_key("interest_noi"));
    }

    #[test]
    fn invalid_dates_estimates_and_stale_evidence_do_not_earn_points() {
        assert!(strict_date("2026-02-30").is_none());
        let mut occ = obs("occupancy", 0.98, None, "2025-03-31", "2025-05-15", "old");
        let result = run(vec![occ.clone()], vec![], "2026-05-16");
        assert!(!result.values["occupancy"].valid);
        assert!(result.provisional.contains(&"missing_or_stale_disclosure"));
        occ = obs("occupancy", 0.98, None, "2026-03-31", "2026-05-15", "new");
        occ.data_kind = "estimated".into();
        assert!(!run(vec![occ.clone()], vec![], "2026-05-16").values["occupancy"].valid);
        occ.source_url = "javascript:alert(1)".into();
        assert!(!occ.validate());
    }
}
