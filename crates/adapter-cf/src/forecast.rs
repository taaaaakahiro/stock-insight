use domain::reit_distribution::{ForecastDetails, ForecastPeriod};
use domain::Date;
use serde::Deserialize;

/// 列の読み方を API とバッチでそろえる。
#[derive(Deserialize)]
pub struct ReitForecastColumns {
    reit_period_start: Option<String>,
    reit_other_period_start: Option<String>,
    reit_other_period_end: Option<String>,
    reit_other_dps: Option<f64>,
    reit_disclosed_time: Option<String>,
}

impl From<ReitForecastColumns> for ForecastDetails {
    fn from(r: ReitForecastColumns) -> Self {
        let other = r
            .reit_other_period_end
            .as_deref()
            .and_then(Date::parse)
            .zip(r.reit_other_dps)
            .map(|(end, amount)| ForecastPeriod {
                start: r.reit_other_period_start.as_deref().and_then(Date::parse),
                end,
                amount,
            });
        Self {
            start: r.reit_period_start.as_deref().and_then(Date::parse),
            other,
            disclosed_time: r.reit_disclosed_time,
        }
    }
}
