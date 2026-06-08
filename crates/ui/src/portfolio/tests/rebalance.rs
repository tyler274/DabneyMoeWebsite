use crate::portfolio::{analyze_rebalance, TradeSide};

use super::fixtures::{
    assert_rebalance_matches_reference, JUNE2ND_FIXTURE, JUNE2ND_REBALANCE, RJF_FIXTURE,
    RJF_REBALANCE,
};

#[test]
fn rebalance_deploys_cash_to_preserve_portfolio_weights() {
    let june = analyze_rebalance(JUNE2ND_FIXTURE).unwrap();
    assert_rebalance_matches_reference(&june, &JUNE2ND_REBALANCE, "june2nd.csv");

    let rjf = analyze_rebalance(RJF_FIXTURE).unwrap();
    assert_rebalance_matches_reference(&rjf, &RJF_REBALANCE, "RJFPortfolio 5.30.2026.csv");
}

#[test]
fn rebalance_with_no_cash_is_all_holds() {
    const NO_CASH: &str = r#""Description","SYMBOL/CUSIP","Quantity","Delayed Price","Current Value","Product Type"
"FUND","AAA","10","$10.00","$100.00","Funds"
"#;
    let report = analyze_rebalance(NO_CASH).unwrap();
    assert!(report.plan.iter().all(|l| l.side == TradeSide::Hold));
    assert!((report.total_buys() - 0.0).abs() < f64::EPSILON);
}
