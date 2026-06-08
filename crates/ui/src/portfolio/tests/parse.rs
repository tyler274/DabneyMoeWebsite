use crate::portfolio::{load_portfolio, parse_dollar, parse_quantity, parse_withdrawal};

use super::fixtures::PORTFOLIO_FIXTURES;

#[test]
fn parse_dollar_matches_python() {
    assert!((parse_dollar("$1,234.56*") - 1234.56).abs() < f64::EPSILON);
    assert!((parse_dollar("($12.34)") - (-12.34)).abs() < f64::EPSILON);
    assert!((parse_dollar("$30,847.54") - 30_847.54).abs() < f64::EPSILON);
    assert!((parse_dollar("") - 0.0).abs() < f64::EPSILON);
    assert!((parse_dollar("$67.24*") - 67.24).abs() < f64::EPSILON);
}

#[test]
fn parse_quantity_matches_python() {
    assert!((parse_quantity("378.010") - 378.010).abs() < f64::EPSILON);
    assert!((parse_quantity("30,847.540 ^") - 30_847.540).abs() < 0.001);
    assert!((parse_quantity("") - 0.0).abs() < f64::EPSILON);
}

#[test]
fn parse_withdrawal_matches_python_main() {
    assert!((parse_withdrawal("10000").unwrap() - 10_000.0).abs() < f64::EPSILON);
    assert!((parse_withdrawal("$10,000").unwrap() - 10_000.0).abs() < f64::EPSILON);
    assert!(parse_withdrawal("").is_err());
    assert!(parse_withdrawal("abc").is_err());
    assert!(parse_withdrawal("-500").is_err());
}

#[test]
fn load_portfolio_reads_raymond_james_exports() {
    let rjf = &PORTFOLIO_FIXTURES[0];
    let (holdings, cash) = load_portfolio(rjf.csv).unwrap();
    assert_eq!(holdings.len(), 9);
    assert!((cash - 30_847.54).abs() < 0.01);
    assert_eq!(holdings[0].symbol, "AEPFX");
    assert!((holdings[0].quantity - 378.010).abs() < 0.001);
    assert!((holdings[0].price - 67.24).abs() < 0.01);
    assert!((holdings[0].current_value - 25_417.39).abs() < 0.01);

    let june = &PORTFOLIO_FIXTURES[1];
    let (holdings, cash) = load_portfolio(june.csv).unwrap();
    assert_eq!(holdings.len(), 4);
    assert!((cash - 659.66).abs() < 0.01);
    assert_eq!(holdings[0].symbol, "AEPFX");
    assert!((holdings[0].price - 67.40).abs() < 0.01);
    assert!((holdings[0].current_value - 25_477.87).abs() < 0.01);
    assert_eq!(holdings[3].symbol, "VEA");
}

#[test]
fn load_portfolio_strips_utf8_bom() {
    for fixture in PORTFOLIO_FIXTURES {
        let bom_fixture = format!("\u{feff}{}", fixture.csv);
        let (holdings, cash) = load_portfolio(&bom_fixture).unwrap();
        assert_eq!(holdings.len(), fixture.holdings_count, "{}", fixture.name);
        assert!((cash - fixture.cash).abs() < 0.01, "{}", fixture.name);
    }
}
