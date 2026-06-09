use crate::portfolio::{
    analyze_rebalance, build_rebalance_report, deploy_cash_target_weights, load_portfolio,
    target_weights_from_csv, TradeSide,
};

use super::fixtures::{
    assert_rebalance_matches_reference, JUNE2ND_FIXTURE, JUNE2ND_REBALANCE, RJF_FIXTURE,
    RJF_REBALANCE,
};

/// Same fund values as `june2nd.csv` but without cash — a fully-invested target allocation.
const JUNE2ND_TARGET_FIXTURE: &str = r#""Description","SYMBOL/CUSIP","Quantity","Delayed Price","Current Value","Product Type"
"AMERICAN FUNDS EUPAC FUND CL F2 N/L","AEPFX","378.010","$67.40*","$25,477.87","Funds"
"COHEN & STEERS GLOBAL REALTY FUND CL I N/L","CSSPX","103.918","$58.06*","$6,033.48","Funds"
"ISHARES TR CORE MSCI EAFE","IEFA","62.000","$98.09","$6,081.27","Funds"
"VANGUARD FTSE DEVELOPED MARKETS ETF","VEA","120.000","$72.19","$8,662.20","Funds"
"#;

fn analyze_deploy_cash_rebalance(portfolio_csv: &str) -> crate::portfolio::RebalanceReport {
    let (holdings, cash) = load_portfolio(portfolio_csv).unwrap();
    let targets = deploy_cash_target_weights(&holdings);
    build_rebalance_report(holdings, cash, targets).unwrap()
}

#[test]
fn rebalance_deploys_cash_to_preserve_portfolio_weights() {
    let june = analyze_deploy_cash_rebalance(JUNE2ND_FIXTURE);
    assert_rebalance_matches_reference(&june, &JUNE2ND_REBALANCE, "june2nd.csv");

    let rjf = analyze_deploy_cash_rebalance(RJF_FIXTURE);
    assert_rebalance_matches_reference(&rjf, &RJF_REBALANCE, "RJFPortfolio 5.30.2026.csv");
}

#[test]
fn rebalance_with_no_cash_is_all_holds() {
    const NO_CASH: &str = r#""Description","SYMBOL/CUSIP","Quantity","Delayed Price","Current Value","Product Type"
"FUND","AAA","10","$10.00","$100.00","Funds"
"#;
    let report = analyze_rebalance(NO_CASH, NO_CASH).unwrap();
    assert!(report.plan.iter().all(|l| l.side == TradeSide::Hold));
    assert!((report.total_buys() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn target_weights_from_portfolio_csv() {
    let (holdings, cash) = load_portfolio(JUNE2ND_FIXTURE).unwrap();
    let weights = target_weights_from_csv(JUNE2ND_FIXTURE).unwrap();
    let total = holdings.iter().map(|h| h.current_value).sum::<f64>() + cash;
    for h in &holdings {
        let expected = h.current_value / total;
        assert!((weights[&h.symbol] - expected).abs() < 1e-10);
    }
    let weight_sum: f64 = weights.values().sum();
    assert!((weight_sum + cash / total - 1.0).abs() < 1e-10);
}

#[test]
fn analyze_rebalance_reads_target_portfolio_csv() {
    let report = analyze_rebalance(JUNE2ND_FIXTURE, JUNE2ND_TARGET_FIXTURE).unwrap();
    assert_rebalance_matches_reference(&report, &JUNE2ND_REBALANCE, "june2nd.csv via target CSV");
}
