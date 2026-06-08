use crate::portfolio::{build_buy_report, build_sell_report, load_portfolio, AccountType};

use super::fixtures::PORTFOLIO_FIXTURES;

#[test]
fn buy_plan_matches_sell_plan_for_same_amounts() {
    for fixture in PORTFOLIO_FIXTURES {
        for reference in fixture.references {
            let Some(withdrawal) = reference.withdrawal else {
                continue;
            };
            let (holdings, cash) = load_portfolio(fixture.csv).unwrap();
            let sell = build_sell_report(
                holdings.clone(),
                cash,
                AccountType::Taxable,
                withdrawal,
                None,
                None,
            )
            .unwrap();
            let buy = build_buy_report(holdings, cash, withdrawal).unwrap();
            assert!(
                (buy.total_cost() - reference.total_proceeds).abs() < 0.01,
                "{}: buy cost mismatch at ${withdrawal}",
                fixture.name
            );
            assert!(
                (buy.leftover() - reference.shortfall).abs() < 0.01,
                "{}: buy leftover mismatch at ${withdrawal}",
                fixture.name
            );
            for (sell_line, buy_line) in sell.plan.iter().zip(buy.plan.iter()) {
                assert_eq!(sell_line.holding.symbol, buy_line.holding.symbol);
                assert_eq!(sell_line.shares_to_sell, buy_line.shares_to_buy);
                assert!((sell_line.proceeds - buy_line.cost).abs() < 0.01);
            }
        }
    }
}
