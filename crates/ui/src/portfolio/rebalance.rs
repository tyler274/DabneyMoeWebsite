use super::allocate::fund_weights;
use super::buy::compute_buy_plan;
use super::parse::load_portfolio;
use super::types::{weight_pct, Holding, RebalanceLine, RebalanceReport, TradeSide};

/// Quarterly rebalance: deploy idle cash into funds to restore each fund's
/// target share of the total portfolio (cash + invested), preserving the
/// current relative fund weights.
pub fn compute_rebalance_plan(
    holdings: &[Holding],
    cash_value: f64,
) -> Result<Vec<RebalanceLine>, String> {
    if holdings.is_empty() {
        return Err("No fund holdings found in CSV.".into());
    }

    let total_portfolio = holdings.iter().map(|h| h.current_value).sum::<f64>() + cash_value;
    let weights = fund_weights(holdings)?;

    if cash_value <= 0.005 {
        return Ok(holdings
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let weight = weight_pct(h.current_value, total_portfolio);
                RebalanceLine {
                    holding: h.clone(),
                    side: TradeSide::Hold,
                    shares: 0,
                    amount: 0.0,
                    weight_before: weight,
                    weight_after: weight,
                    target_value: weights[i] * total_portfolio,
                }
            })
            .collect());
    }

    let buy_plan = compute_buy_plan(holdings, cash_value)?;

    Ok(holdings
        .iter()
        .enumerate()
        .zip(buy_plan.iter())
        .map(|((i, h), buy)| {
            let weight_before = weight_pct(h.current_value, total_portfolio);
            let new_value = h.current_value + buy.cost;
            let weight_after = weight_pct(new_value, total_portfolio);
            let side = if buy.shares_to_buy > 0 {
                TradeSide::Buy
            } else {
                TradeSide::Hold
            };
            RebalanceLine {
                holding: h.clone(),
                side,
                shares: buy.shares_to_buy,
                amount: buy.cost,
                weight_before,
                weight_after,
                target_value: weights[i] * total_portfolio,
            }
        })
        .collect())
}

/// Build a rebalance report from loaded holdings.
pub fn build_rebalance_report(
    holdings: Vec<Holding>,
    cash_value: f64,
) -> Result<RebalanceReport, String> {
    let plan = compute_rebalance_plan(&holdings, cash_value)?;
    Ok(RebalanceReport {
        holdings,
        cash_value,
        plan,
    })
}

/// Parse CSV text and compute a rebalance report in one step.
pub fn analyze_rebalance(csv_text: &str) -> Result<RebalanceReport, String> {
    let (holdings, cash_value) = load_portfolio(csv_text)?;
    build_rebalance_report(holdings, cash_value)
}
