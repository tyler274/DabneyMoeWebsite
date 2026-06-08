use crate::portfolio::{build_sell_report, compute_sell_plan, load_portfolio, AccountType};

use super::fixtures::{assert_fixture_matches_python, PORTFOLIO_FIXTURES};

#[test]
fn sell_plan_matches_python_reference_for_all_portfolios() {
    for fixture in PORTFOLIO_FIXTURES {
        assert_fixture_matches_python(fixture);
    }
}

#[test]
fn rejects_withdrawal_above_fund_value() {
    for fixture in PORTFOLIO_FIXTURES {
        let (holdings, cash) = load_portfolio(fixture.csv).unwrap();
        let err = build_sell_report(
            holdings,
            cash,
            AccountType::Taxable,
            fixture.fund_value + 1.0,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.contains("exceeds total fund value"),
            "{}: expected over-withdrawal error",
            fixture.name
        );
    }
}

#[test]
fn rejects_empty_fund_holdings() {
    const CASH_ONLY: &str = r#""Description","SYMBOL/CUSIP","Quantity","Delayed Price","Current Value","Product Type"
"Cash","","1000","$1.00","$1000.00","Cash & Cash Alternatives"
"#;
    let (holdings, cash) = load_portfolio(CASH_ONLY).unwrap();
    assert!(holdings.is_empty());
    assert!((cash - 1000.0).abs() < f64::EPSILON);

    let err = build_sell_report(
        holdings,
        cash,
        AccountType::Taxable,
        500.0,
        None,
        None,
    )
    .unwrap_err();
    assert!(err.contains("No fund holdings found"));
}

#[test]
fn compute_sell_plan_rejects_zero_fund_value() {
    let err = compute_sell_plan(&[], 100.0).unwrap_err();
    assert!(err.contains("No fund holdings found"));
}
