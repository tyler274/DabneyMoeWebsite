use std::collections::HashMap;

use super::allocate::{allocate_sell_shares, allocate_whole_shares_weighted};
use super::parse::{load_portfolio, target_weights_from_csv};
use super::types::{weight_pct, Holding, RebalanceLine, RebalanceReport, TradeSide};

/// Rebalance toward explicit target weights (fraction of total portfolio per fund).
pub fn compute_rebalance_plan(
    holdings: &[Holding],
    cash_value: f64,
    target_weights: &HashMap<String, f64>,
) -> Result<Vec<RebalanceLine>, String> {
    if holdings.is_empty() {
        return Err("No fund holdings found in CSV.".into());
    }

    let total_portfolio = holdings.iter().map(|h| h.current_value).sum::<f64>() + cash_value;
    let target_fractions: Vec<f64> = holdings
        .iter()
        .map(|h| {
            target_weights
                .get(&h.symbol)
                .copied()
                .ok_or_else(|| format!("No target weight for {}.", h.symbol))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let target_values: Vec<f64> = target_fractions
        .iter()
        .map(|weight| weight * total_portfolio)
        .collect();

    let overweight: Vec<f64> = holdings
        .iter()
        .zip(target_values.iter())
        .map(|(h, target)| (h.current_value - target).max(0.0))
        .collect();

    let sell_shares = allocate_sell_shares(holdings, &overweight)?;
    let sell_amounts: Vec<f64> = holdings
        .iter()
        .zip(sell_shares.iter())
        .map(|(h, shares)| *shares as f64 * h.price)
        .collect();

    let after_sell_values: Vec<f64> = holdings
        .iter()
        .zip(sell_amounts.iter())
        .map(|(h, sold)| h.current_value - sold)
        .collect();
    let sell_proceeds: f64 = sell_amounts.iter().sum();
    let buy_pool = cash_value + sell_proceeds;

    let underweight: Vec<f64> = target_values
        .iter()
        .zip(after_sell_values.iter())
        .map(|(target, current)| (target - current).max(0.0))
        .collect();
    let total_underweight: f64 = underweight.iter().sum();

    let buy_shares = if total_underweight > 0.005 && buy_pool > 0.005 {
        let buy_weights: Vec<f64> = underweight
            .iter()
            .map(|gap| gap / total_underweight)
            .collect();
        allocate_whole_shares_weighted(holdings, buy_pool, &buy_weights)?
    } else {
        vec![0; holdings.len()]
    };

    let buy_amounts: Vec<f64> = holdings
        .iter()
        .zip(buy_shares.iter())
        .map(|(h, shares)| *shares as f64 * h.price)
        .collect();

    Ok(holdings
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let weight_before = weight_pct(h.current_value, total_portfolio);
            let new_value = after_sell_values[i] + buy_amounts[i];
            let weight_after = weight_pct(new_value, total_portfolio);
            let (side, shares, amount) = if sell_shares[i] > 0 {
                (TradeSide::Sell, sell_shares[i], sell_amounts[i])
            } else if buy_shares[i] > 0 {
                (TradeSide::Buy, buy_shares[i], buy_amounts[i])
            } else {
                (TradeSide::Hold, 0, 0.0)
            };
            RebalanceLine {
                holding: h.clone(),
                side,
                shares,
                amount,
                weight_before,
                weight_after,
                target_value: target_values[i],
            }
        })
        .collect())
}

/// Build a rebalance report from loaded holdings and target weights.
pub fn build_rebalance_report(
    holdings: Vec<Holding>,
    cash_value: f64,
    target_weights: HashMap<String, f64>,
) -> Result<RebalanceReport, String> {
    let plan = compute_rebalance_plan(&holdings, cash_value, &target_weights)?;
    Ok(RebalanceReport {
        holdings,
        cash_value,
        plan,
    })
}

/// Parse portfolio + target-weight CSVs and compute a rebalance report.
pub fn analyze_rebalance(
    portfolio_csv: &str,
    target_weights_csv: &str,
) -> Result<RebalanceReport, String> {
    let (holdings, cash_value) = load_portfolio(portfolio_csv)?;
    let target_weights = target_weights_from_csv(target_weights_csv)?;
    build_rebalance_report(holdings, cash_value, target_weights)
}

/// Target weights that reproduce the cash-deployment rebalance (each fund at
/// `current_value / total_fund_value` of the total portfolio).
pub fn deploy_cash_target_weights(holdings: &[Holding]) -> HashMap<String, f64> {
    let fund_total: f64 = holdings.iter().map(|h| h.current_value).sum();
    holdings
        .iter()
        .map(|h| (h.symbol.clone(), h.current_value / fund_total))
        .collect()
}
