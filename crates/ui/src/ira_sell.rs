//! Investment account ratio-preserving sell calculator.
//!
//! Port of `KlingBot/scripts/ira_sell.py`. Parses a Raymond James portfolio CSV
//! export and computes whole-share sell orders that preserve fund weights.

use std::io::Cursor;

#[derive(Debug, Clone, PartialEq)]
pub struct Holding {
    pub description: String,
    pub symbol: String,
    pub quantity: f64,
    pub price: f64,
    pub current_value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SellLine {
    pub holding: Holding,
    pub shares_to_sell: u32,
    pub proceeds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SellReport {
    pub holdings: Vec<Holding>,
    pub cash_value: f64,
    pub withdrawal: f64,
    pub plan: Vec<SellLine>,
}

impl SellReport {
    pub fn total_fund_value(&self) -> f64 {
        self.holdings.iter().map(|h| h.current_value).sum()
    }

    pub fn total_portfolio(&self) -> f64 {
        self.total_fund_value() + self.cash_value
    }

    pub fn total_proceeds(&self) -> f64 {
        self.plan.iter().map(|l| l.proceeds).sum()
    }

    pub fn shortfall(&self) -> f64 {
        self.withdrawal - self.total_proceeds()
    }

    /// Fund weight before the sell, as a percentage.
    pub fn weight_before(&self, index: usize) -> f64 {
        let total = self.total_fund_value();
        if total > 0.0 {
            self.holdings[index].current_value / total * 100.0
        } else {
            0.0
        }
    }

    /// Fund weight after the sell, as a percentage.
    pub fn weight_after(&self, index: usize) -> f64 {
        let new_values: Vec<f64> = self
            .plan
            .iter()
            .map(|line| line.holding.current_value - line.proceeds)
            .collect();
        let new_total: f64 = new_values.iter().sum();
        if new_total > 0.0 {
            new_values[index] / new_total * 100.0
        } else {
            0.0
        }
    }

    pub fn weight_delta(&self, index: usize) -> f64 {
        self.weight_after(index) - self.weight_before(index)
    }
}

/// Strip currency formatting and return a float.
pub fn parse_dollar(value: &str) -> f64 {
    let mut negative = false;
    let cleaned: String = value
        .chars()
        .filter_map(|c| match c {
            '(' => {
                negative = true;
                None
            }
            ')' | '$' | ',' | ' ' | '^' | '*' => None,
            c => Some(c),
        })
        .collect();
    let parsed = cleaned.parse::<f64>().unwrap_or(0.0);
    if negative { -parsed } else { parsed }
}

/// Strip quantity formatting and return a float.
pub fn parse_quantity(value: &str) -> f64 {
    let cleaned: String = value
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '^' | '*'))
        .collect();
    cleaned.parse().unwrap_or(0.0)
}

/// Parse a withdrawal amount typed in the UI (`10,000`, `$10000`, etc.).
pub fn parse_withdrawal(input: &str) -> Result<f64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter a withdrawal amount.".into());
    }
    let amount = parse_dollar(trimmed);
    if amount <= 0.0 {
        return Err("Withdrawal must be greater than zero.".into());
    }
    Ok(amount)
}

/// Load fund holdings and cash from a Raymond James portfolio CSV export.
pub fn load_portfolio(csv_text: &str) -> Result<(Vec<Holding>, f64), String> {
    let text = csv_text.strip_prefix('\u{feff}').unwrap_or(csv_text);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(Cursor::new(text.as_bytes()));

    let headers = reader
        .headers()
        .map_err(|e| format!("Invalid CSV header: {e}"))?;

    let col = |name: &str| -> Result<usize, String> {
        headers
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| format!("Missing column: {name}"))
    };

    let description_col = col("Description")?;
    let symbol_col = col("SYMBOL/CUSIP")?;
    let quantity_col = col("Quantity")?;
    let price_col = col("Delayed Price")?;
    let value_col = col("Current Value")?;
    let product_type_col = col("Product Type")?;

    let mut holdings = Vec::new();
    let mut cash_value = 0.0;

    for result in reader.records() {
        let row = result.map_err(|e| format!("Invalid CSV row: {e}"))?;
        let get = |index: usize| row.get(index).unwrap_or("").trim();

        let symbol = get(symbol_col).to_string();
        let product_type = get(product_type_col);
        let current_value = parse_dollar(get(value_col));

        if product_type == "Cash & Cash Alternatives" || symbol.is_empty() {
            cash_value += current_value;
            continue;
        }

        holdings.push(Holding {
            description: get(description_col).to_string(),
            symbol,
            quantity: parse_quantity(get(quantity_col)),
            price: parse_dollar(get(price_col)),
            current_value,
        });
    }

    Ok((holdings, cash_value))
}

/// Compute whole-share sells that preserve portfolio weights as closely as possible.
pub fn compute_sell_plan(holdings: &[Holding], withdrawal: f64) -> Result<Vec<SellLine>, String> {
    let total_fund_value: f64 = holdings.iter().map(|h| h.current_value).sum();

    if total_fund_value <= 0.0 {
        return Err("No fund holdings found — nothing to sell.".into());
    }
    if withdrawal > total_fund_value {
        return Err(format!(
            "Withdrawal ${withdrawal:.2} exceeds total fund value ${total_fund_value:.2}."
        ));
    }

    let raw_shares: Vec<f64> = holdings
        .iter()
        .map(|h| {
            let weight = h.current_value / total_fund_value;
            let allocated = withdrawal * weight;
            if h.price > 0.0 {
                allocated / h.price
            } else {
                0.0
            }
        })
        .collect();

    let mut floor_shares: Vec<u32> = raw_shares.iter().map(|r| r.floor() as u32).collect();
    let floor_proceeds: f64 = floor_shares
        .iter()
        .enumerate()
        .map(|(i, shares)| *shares as f64 * holdings[i].price)
        .sum();

    let mut gap = withdrawal - floor_proceeds;
    let mut remainders: Vec<(f64, usize)> = raw_shares
        .iter()
        .enumerate()
        .map(|(i, raw)| (raw - floor_shares[i] as f64, i))
        .collect();
    remainders.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, idx) in remainders {
        let extra_proceeds = holdings[idx].price;
        if gap >= extra_proceeds * 0.5 {
            floor_shares[idx] += 1;
            gap -= extra_proceeds;
        }
        if gap.abs() < 0.01 {
            break;
        }
    }

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

/// Parse the CSV and compute a full sell report.
pub fn build_report(
    holdings: Vec<Holding>,
    cash_value: f64,
    withdrawal: f64,
) -> Result<SellReport, String> {
    if holdings.is_empty() {
        return Err("No fund holdings found in CSV.".into());
    }
    let plan = compute_sell_plan(&holdings, withdrawal)?;
    Ok(SellReport {
        holdings,
        cash_value,
        withdrawal,
        plan,
    })
}

/// Convenience: parse CSV text and compute the sell report in one step.
pub fn analyze_portfolio(csv_text: &str, withdrawal: f64) -> Result<SellReport, String> {
    let (holdings, cash_value) = load_portfolio(csv_text)?;
    build_report(holdings, cash_value, withdrawal)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RJF_FIXTURE: &str = include_str!("../tests/fixtures/rjf_portfolio.csv");
    const JUNE2ND_FIXTURE: &str = include_str!("../tests/fixtures/june2nd.csv");

    /// Expected output from `KlingBot/scripts/ira_sell.py`.
    struct PythonReference {
        /// `None` withdraws the full loaded fund value (liquidation test).
        withdrawal: Option<f64>,
        total_proceeds: f64,
        shortfall: f64,
        /// `(symbol, shares_to_sell, proceeds)` per fund, in CSV order.
        lines: &'static [(&'static str, u32, f64)],
    }

    /// A portfolio CSV plus Python-generated sell-plan references.
    struct PortfolioFixture {
        name: &'static str,
        csv: &'static str,
        cash: f64,
        fund_value: f64,
        total_portfolio: f64,
        holdings_count: usize,
        references: &'static [PythonReference],
        weight_deltas_10k: Option<&'static [(&'static str, f64)]>,
    }

    const RJF_REFERENCES: &[PythonReference] = &[
        PythonReference {
            withdrawal: Some(1_000.0),
            total_proceeds: 1_050.74,
            shortfall: -50.74,
            lines: &[
                ("AEPFX", 5, 336.20),
                ("CSSPX", 1, 59.21),
                ("IEFA", 1, 98.02),
                ("IJR", 0, 0.00),
                ("MDYG", 1, 108.95),
                ("MDYV", 1, 91.48),
                ("SLYG", 1, 108.97),
                ("SLYV", 1, 104.37),
                ("VEA", 2, 143.54),
            ],
        },
        PythonReference {
            withdrawal: Some(5_000.0),
            total_proceeds: 5_028.73,
            shortfall: -28.73,
            lines: &[
                ("AEPFX", 26, 1_748.24),
                ("CSSPX", 7, 414.47),
                ("IEFA", 4, 392.08),
                ("IJR", 2, 277.32),
                ("MDYG", 5, 544.75),
                ("MDYV", 4, 365.92),
                ("SLYG", 3, 326.91),
                ("SLYV", 3, 313.11),
                ("VEA", 9, 645.93),
            ],
        },
        PythonReference {
            withdrawal: Some(10_000.0),
            total_proceeds: 9_979.01,
            shortfall: 20.99,
            lines: &[
                ("AEPFX", 51, 3_429.24),
                ("CSSPX", 14, 828.94),
                ("IEFA", 9, 882.18),
                ("IJR", 5, 693.30),
                ("MDYG", 9, 980.55),
                ("MDYV", 8, 731.84),
                ("SLYG", 7, 762.79),
                ("SLYV", 5, 521.85),
                ("VEA", 16, 1_148.32),
            ],
        },
        PythonReference {
            withdrawal: Some(25_000.0),
            total_proceeds: 24_980.21,
            shortfall: 19.79,
            lines: &[
                ("AEPFX", 128, 8_606.72),
                ("CSSPX", 35, 2_072.35),
                ("IEFA", 21, 2_058.42),
                ("IJR", 12, 1_663.92),
                ("MDYG", 23, 2_505.85),
                ("MDYV", 21, 1_921.08),
                ("SLYG", 17, 1_852.49),
                ("SLYV", 13, 1_356.81),
                ("VEA", 41, 2_942.57),
            ],
        },
        PythonReference {
            withdrawal: Some(73_511.95),
            total_proceeds: 73_516.14,
            shortfall: -4.19,
            lines: &[
                ("AEPFX", 378, 25_416.72),
                ("CSSPX", 104, 6_157.84),
                ("IEFA", 62, 6_077.24),
                ("IJR", 35, 4_853.10),
                ("MDYG", 68, 7_408.60),
                ("MDYV", 61, 5_580.28),
                ("SLYG", 49, 5_339.53),
                ("SLYV", 39, 4_070.43),
                ("VEA", 120, 8_612.40),
            ],
        },
    ];

    const RJF_WEIGHT_DELTAS_10K: &[(&str, f64)] = &[
        ("AEPFX", 0.03),
        ("CSSPX", 0.01),
        ("IEFA", -0.09),
        ("IJR", -0.05),
        ("MDYG", 0.04),
        ("MDYV", 0.04),
        ("SLYG", -0.06),
        ("SLYV", 0.05),
        ("VEA", 0.03),
    ];

    const JUNE2ND_REFERENCES: &[PythonReference] = &[
        PythonReference {
            withdrawal: Some(1_000.0),
            total_proceeds: 1_028.04,
            shortfall: -28.04,
            lines: &[
                ("AEPFX", 8, 539.20),
                ("CSSPX", 3, 174.18),
                ("IEFA", 1, 98.09),
                ("VEA", 3, 216.57),
            ],
        },
        PythonReference {
            withdrawal: Some(5_000.0),
            total_proceeds: 5_027.16,
            shortfall: -27.16,
            lines: &[
                ("AEPFX", 41, 2_763.40),
                ("CSSPX", 11, 638.66),
                ("IEFA", 7, 686.63),
                ("VEA", 13, 938.47),
            ],
        },
        PythonReference {
            withdrawal: Some(10_000.0),
            total_proceeds: 10_014.29,
            shortfall: -14.29,
            lines: &[
                ("AEPFX", 82, 5_526.80),
                ("CSSPX", 23, 1_335.38),
                ("IEFA", 13, 1_275.17),
                ("VEA", 26, 1_876.94),
            ],
        },
        PythonReference {
            withdrawal: Some(25_000.0),
            total_proceeds: 25_028.37,
            shortfall: -28.37,
            lines: &[
                ("AEPFX", 204, 13_749.60),
                ("CSSPX", 56, 3_251.36),
                ("IEFA", 34, 3_335.06),
                ("VEA", 65, 4_692.35),
            ],
        },
        PythonReference {
            withdrawal: None,
            total_proceeds: 46_259.82,
            shortfall: -5.00,
            lines: &[
                ("AEPFX", 378, 25_477.20),
                ("CSSPX", 104, 6_038.24),
                ("IEFA", 62, 6_081.58),
                ("VEA", 120, 8_662.80),
            ],
        },
    ];

    const JUNE2ND_WEIGHT_DELTAS_10K: &[(&str, f64)] = &[
        ("AEPFX", -0.03),
        ("CSSPX", -0.08),
        ("IEFA", 0.11),
        ("VEA", -0.00),
    ];

    const PORTFOLIO_FIXTURES: &[PortfolioFixture] = &[
        PortfolioFixture {
            name: "RJFPortfolio 5.30.2026.csv",
            csv: RJF_FIXTURE,
            cash: 30_847.54,
            fund_value: 73_511.95,
            total_portfolio: 104_359.49,
            holdings_count: 9,
            references: RJF_REFERENCES,
            weight_deltas_10k: Some(RJF_WEIGHT_DELTAS_10K),
        },
        PortfolioFixture {
            name: "june2nd.csv",
            csv: JUNE2ND_FIXTURE,
            cash: 659.66,
            fund_value: 46_254.82,
            total_portfolio: 46_914.48,
            holdings_count: 4,
            references: JUNE2ND_REFERENCES,
            weight_deltas_10k: Some(JUNE2ND_WEIGHT_DELTAS_10K),
        },
    ];

    fn assert_report_matches_python(
        report: &SellReport,
        reference: &PythonReference,
        fixture_name: &str,
        withdrawal: f64,
    ) {
        assert!(
            (report.withdrawal - withdrawal).abs() < f64::EPSILON,
            "{fixture_name}: withdrawal mismatch"
        );
        assert!(
            (report.total_proceeds() - reference.total_proceeds).abs() < 0.01,
            "{fixture_name}: total proceeds mismatch for ${withdrawal}"
        );
        assert!(
            (report.shortfall() - reference.shortfall).abs() < 0.01,
            "{fixture_name}: shortfall mismatch for ${withdrawal}"
        );
        assert_eq!(
            report.plan.len(),
            reference.lines.len(),
            "{fixture_name}: plan length mismatch for ${withdrawal}"
        );

        for (line, (sym, shares, proceeds)) in report.plan.iter().zip(reference.lines) {
            assert_eq!(
                line.holding.symbol, *sym,
                "{fixture_name}: symbol mismatch at ${withdrawal} withdrawal"
            );
            assert_eq!(
                line.shares_to_sell, *shares,
                "{fixture_name}: {sym} share count mismatch at ${withdrawal} withdrawal"
            );
            assert!(
                (line.proceeds - proceeds).abs() < 0.01,
                "{fixture_name}: {sym} proceeds mismatch at ${withdrawal} withdrawal: got {}, expected {}",
                line.proceeds,
                proceeds
            );
        }
    }

    fn assert_fixture_matches_python(fixture: &PortfolioFixture) {
        let (holdings, cash) = load_portfolio(fixture.csv).unwrap();
        assert_eq!(
            holdings.len(),
            fixture.holdings_count,
            "{}: unexpected holdings count",
            fixture.name
        );
        assert!(
            (cash - fixture.cash).abs() < 0.01,
            "{}: cash mismatch",
            fixture.name
        );

        let fund_value: f64 = holdings.iter().map(|h| h.current_value).sum();
        assert!(
            (fund_value - fixture.fund_value).abs() < 0.01,
            "{}: fund value mismatch",
            fixture.name
        );
        assert!(
            (cash + fund_value - fixture.total_portfolio).abs() < 0.01,
            "{}: total portfolio mismatch",
            fixture.name
        );

        for reference in fixture.references {
            let withdrawal = reference.withdrawal.unwrap_or(fund_value);
            let report = build_report(holdings.clone(), cash, withdrawal).unwrap();
            assert_report_matches_python(&report, reference, fixture.name, withdrawal);
        }

        for reference in fixture.references {
            let withdrawal = reference.withdrawal.unwrap_or(fund_value);
            let report = analyze_portfolio(fixture.csv, withdrawal).unwrap();
            assert_report_matches_python(&report, reference, fixture.name, withdrawal);
        }

        if let Some(deltas) = fixture.weight_deltas_10k {
            let report = analyze_portfolio(fixture.csv, 10_000.0).unwrap();
            for (index, (sym, expected_delta)) in deltas.iter().enumerate() {
                assert_eq!(report.plan[index].holding.symbol, *sym);
                assert!(
                    (report.weight_delta(index) - expected_delta).abs() < 0.01,
                    "{}: {sym} weight delta: got {}, expected {}",
                    fixture.name,
                    report.weight_delta(index),
                    expected_delta
                );
            }
        }
    }

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
            let err = build_report(holdings, cash, fixture.fund_value + 1.0).unwrap_err();
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

        let err = build_report(holdings, cash, 500.0).unwrap_err();
        assert!(err.contains("No fund holdings found"));
    }

    #[test]
    fn compute_sell_plan_rejects_zero_fund_value() {
        let err = compute_sell_plan(&[], 100.0).unwrap_err();
        assert!(err.contains("No fund holdings found"));
    }
}
