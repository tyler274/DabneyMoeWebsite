use super::allocate::allocate_whole_shares;
use super::parse::load_portfolio;
use super::types::{AccountType, Holding, SellLine, SellReport};
use super::withholding::TaxWithholding;

/// Compute whole-share sells that preserve portfolio weights as closely as possible.
pub fn compute_sell_plan(holdings: &[Holding], withdrawal: f64) -> Result<Vec<SellLine>, String> {
    let total_fund_value: f64 = holdings.iter().map(|h| h.current_value).sum();

    if total_fund_value <= 0.0 {
        return Err("No fund holdings found - nothing to sell.".into());
    }
    if withdrawal > total_fund_value {
        return Err(format!(
            "Withdrawal ${withdrawal:.2} exceeds total fund value ${total_fund_value:.2}."
        ));
    }

    let floor_shares = allocate_whole_shares(holdings, withdrawal)?;

    Ok(holdings
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let shares = floor_shares[i];
            SellLine {
                holding: h.clone(),
                shares_to_sell: shares,
                proceeds: shares as f64 * h.price,
            }
        })
        .collect())
}

/// Build a full sell report from loaded holdings.
pub fn build_sell_report(
    holdings: Vec<Holding>,
    cash_value: f64,
    account_type: AccountType,
    withdrawal: f64,
    net_withdrawal: Option<f64>,
    withholding: Option<TaxWithholding>,
) -> Result<SellReport, String> {
    if holdings.is_empty() {
        return Err("No fund holdings found in CSV.".into());
    }

    let (gross_withdrawal, breakdown) = match (net_withdrawal, withholding) {
        (Some(net), Some(rates)) => {
            let gross = rates.gross_from_net(net)?;
            let breakdown = rates.apply_to_gross(gross);
            (gross, Some(breakdown))
        }
        _ => (withdrawal, None),
    };

    let plan = compute_sell_plan(&holdings, gross_withdrawal)?;
    Ok(SellReport {
        account_type,
        holdings,
        cash_value,
        withdrawal: gross_withdrawal,
        net_withdrawal,
        withholding: breakdown,
        plan,
    })
}

/// Parse CSV text and compute a sell report in one step.
pub fn analyze_sell(
    csv_text: &str,
    account_type: AccountType,
    withdrawal: f64,
    net_withdrawal: Option<f64>,
    withholding: Option<TaxWithholding>,
) -> Result<SellReport, String> {
    let (holdings, cash_value) = load_portfolio(csv_text)?;
    build_sell_report(
        holdings,
        cash_value,
        account_type,
        withdrawal,
        net_withdrawal,
        withholding,
    )
}

/// Alias kept for callers that name the operation after the portfolio input.
pub fn analyze_portfolio(
    csv_text: &str,
    account_type: AccountType,
    withdrawal: f64,
    net_withdrawal: Option<f64>,
    withholding: Option<TaxWithholding>,
) -> Result<SellReport, String> {
    analyze_sell(
        csv_text,
        account_type,
        withdrawal,
        net_withdrawal,
        withholding,
    )
}
