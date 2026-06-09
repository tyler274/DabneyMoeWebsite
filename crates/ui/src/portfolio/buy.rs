use super::allocate::allocate_whole_shares;
use super::parse::load_portfolio;
use super::types::{BuyLine, BuyReport, Holding};

/// Compute whole-share buys that preserve portfolio weights as closely as possible.
pub fn compute_buy_plan(holdings: &[Holding], deposit: f64) -> Result<Vec<BuyLine>, String> {
    if holdings.is_empty() {
        return Err("No fund holdings found - nothing to buy.".into());
    }

    let floor_shares = allocate_whole_shares(holdings, deposit)?;

    Ok(holdings
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let shares = floor_shares[i];
            BuyLine {
                holding: h.clone(),
                shares_to_buy: shares,
                cost: shares as f64 * h.price,
            }
        })
        .collect())
}

/// Build a full buy report from loaded holdings.
pub fn build_buy_report(
    holdings: Vec<Holding>,
    cash_value: f64,
    deposit: f64,
) -> Result<BuyReport, String> {
    if holdings.is_empty() {
        return Err("No fund holdings found in CSV.".into());
    }
    let plan = compute_buy_plan(&holdings, deposit)?;
    Ok(BuyReport {
        holdings,
        cash_value,
        deposit,
        plan,
    })
}

/// Parse CSV text and compute a buy report in one step.
pub fn analyze_buy(csv_text: &str, deposit: f64) -> Result<BuyReport, String> {
    let (holdings, cash_value) = load_portfolio(csv_text)?;
    build_buy_report(holdings, cash_value, deposit)
}
