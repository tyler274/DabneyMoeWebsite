use crate::portfolio::{
    analyze_portfolio, build_sell_report, load_portfolio, AccountType, RebalanceReport, SellReport,
    TradeSide,
};

pub const RJF_FIXTURE: &str = include_str!("../../../tests/fixtures/rjf_portfolio.csv");
pub const JUNE2ND_FIXTURE: &str = include_str!("../../../tests/fixtures/june2nd.csv");

/// Expected output from `KlingBot/scripts/ira_sell.py`.
pub struct PythonReference {
    /// `None` withdraws the full loaded fund value (liquidation test).
    pub withdrawal: Option<f64>,
    pub total_proceeds: f64,
    pub shortfall: f64,
    /// `(symbol, shares_to_sell, proceeds)` per fund, in CSV order.
    pub lines: &'static [(&'static str, u32, f64)],
}

pub struct PortfolioFixture {
    pub name: &'static str,
    pub csv: &'static str,
    pub cash: f64,
    pub fund_value: f64,
    pub total_portfolio: f64,
    pub holdings_count: usize,
    pub references: &'static [PythonReference],
    pub weight_deltas_10k: Option<&'static [(&'static str, f64)]>,
}

pub struct RebalanceReference {
    pub lines: &'static [(&'static str, u32, f64)],
    pub total_buys: f64,
    pub cash_leftover: f64,
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
        withdrawal: None,
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

pub const PORTFOLIO_FIXTURES: &[PortfolioFixture] = &[
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

pub const JUNE2ND_REBALANCE: RebalanceReference = RebalanceReference {
    lines: &[
        ("AEPFX", 5, 337.00),
        ("CSSPX", 1, 58.06),
        ("IEFA", 1, 98.09),
        ("VEA", 2, 144.38),
    ],
    total_buys: 637.53,
    cash_leftover: 22.13,
};

pub const RJF_REBALANCE: RebalanceReference = RebalanceReference {
    lines: &[
        ("AEPFX", 159, 10_691.16),
        ("CSSPX", 44, 2_605.24),
        ("IEFA", 26, 2_548.52),
        ("IJR", 15, 2_079.90),
        ("MDYG", 28, 3_050.60),
        ("MDYV", 26, 2_378.48),
        ("SLYG", 21, 2_288.37),
        ("SLYV", 16, 1_669.92),
        ("VEA", 50, 3_588.50),
    ],
    total_buys: 30_900.69,
    cash_leftover: -53.15,
};

pub fn assert_report_matches_python(
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

pub fn assert_fixture_matches_python(fixture: &PortfolioFixture) {
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
        let report = build_sell_report(
            holdings.clone(),
            cash,
            AccountType::Taxable,
            withdrawal,
            None,
            None,
        )
        .unwrap();
        assert_report_matches_python(&report, reference, fixture.name, withdrawal);
    }

    for reference in fixture.references {
        let withdrawal = reference.withdrawal.unwrap_or(fund_value);
        let report =
            analyze_portfolio(fixture.csv, AccountType::Taxable, withdrawal, None, None).unwrap();
        assert_report_matches_python(&report, reference, fixture.name, withdrawal);
    }

    if let Some(deltas) = fixture.weight_deltas_10k {
        let report =
            analyze_portfolio(fixture.csv, AccountType::Taxable, 10_000.0, None, None).unwrap();
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

pub fn assert_rebalance_matches_reference(
    report: &RebalanceReport,
    reference: &RebalanceReference,
    name: &str,
) {
    assert!(
        (report.total_buys() - reference.total_buys).abs() < 0.01,
        "{name}: total buys mismatch"
    );
    assert!(
        (report.cash_leftover() - reference.cash_leftover).abs() < 0.01,
        "{name}: cash leftover mismatch"
    );
    for (line, (sym, shares, cost)) in report.plan.iter().zip(reference.lines) {
        assert_eq!(line.holding.symbol, *sym, "{name}: symbol mismatch");
        assert_eq!(line.shares, *shares, "{name}: {sym} share mismatch");
        assert!(
            (line.amount - cost).abs() < 0.01,
            "{name}: {sym} cost mismatch"
        );
        assert_eq!(line.side, TradeSide::Buy, "{name}: {sym} should be a buy");
    }
}
