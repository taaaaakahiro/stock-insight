use serde_json::{json, Value};
use usecase::screening::StockRow;

pub fn row_json(r: &StockRow) -> Value {
    let mut v = json!({
        "code": r.code,
        "price": r.price,
        "price_date": r.price_date,
        "dividend": r.dividend,
        "dividend_total": r.dividend_total,
        "dividend_special": r.dividend_special,
        "dividend_commemorative": r.dividend_commemorative,
        "dividend_source": r.dividend_source,
        "dividend_basis": r.dividend_basis,
        "dividend_yield": r.dividend_yield,
        "yield_percentile": r.yield_percentile,
        "pbr": r.pbr,
        "per": r.per,
        "payout_ratio": r.payout_ratio,
        "doe": r.doe,
        "consecutive_increases": r.consecutive_increases,
        "operating_margin": r.operating_margin,
        "no_loss_years": r.no_loss_years,
        "profit_uptrend": r.profit_uptrend,
        "valuation": r.valuation,
        "fetched_at": r.fetched_at,
        "name": r.name,
        "kind": r.kind,
        "market": r.market,
        "sector33": r.sector33,
        "size_class": r.size_class,
        "dividend_every_year": r.dividend_every_year,
        "no_quarterly_loss": r.no_quarterly_loss,
        "is_value": r.is_value,
        "data_years": r.data_years,
        "sales_growth": r.sales_growth,
        "op_growth": r.op_growth,
        "dividend_cut": r.dividend_cut,
        "progressive_dividend": r.progressive_dividend,
        "no_dividend_cut": r.no_dividend_cut,
        "reit": r.reit.as_ref().map(|x| json!({
            "sector": x.sector,
            "period_end": x.period_end,
            "floating_ratio": x.floating_ratio,
            "unknown_ratio": x.unknown_ratio,
            "reprice_ratio": x.reprice_ratio,
            "reprice_max_ratio": x.reprice_max_ratio,
            "avg_rate": x.avg_rate,
            "occupancy": x.occupancy,
        })),
    });
    // json! の再帰の上限に当たるので、配当方針は後から足す
    v["doe_floor"] = json!(r.doe_floor);
    v["doe_goal"] = json!(r.doe_goal);
    v["doe_forecast"] = json!(r.doe_forecast);
    v["payout_floor"] = json!(r.payout_floor);
    v["payout_goal"] = json!(r.payout_goal);
    v["payout_forecast"] = json!(r.payout_forecast);
    v["op_growth_q"] = json!(r.op_growth_q);
    v["net_growth_q"] = json!(r.net_growth_q);
    v["op_growth_ttm"] = json!(r.op_growth_ttm);
    v["profit_one_off"] = json!(r.profit_one_off);
    // 方針と今の配当の乖離（方針どおりなら配当が何 % 増えるか）。方針がないか満たしていれば null
    v["policy_gap"] = json!(r.policy_gap().map(|g| json!({
        "basis": g.key(),
        "policy": g.policy,
        "now": g.now,
        "upside": g.upside,
    })));
    v
}
