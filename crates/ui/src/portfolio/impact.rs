use super::types::{BuyReport, RebalanceReport, SellReport};

#[derive(Debug, Clone, PartialEq)]
pub struct FundImpact {
    pub symbol: String,
    pub label: String,
    pub value_before: f64,
    pub value_after: f64,
    pub weight_before: f64,
    pub weight_after: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PortfolioImpact {
    pub total_before: f64,
    pub total_after: f64,
    pub fund_value_before: f64,
    pub fund_value_after: f64,
    pub cash_before: f64,
    pub cash_after: f64,
    pub funds: Vec<FundImpact>,
}

impl PortfolioImpact {
    pub fn from_sell(report: &SellReport) -> Self {
        let fund_value_before = report.total_fund_value();
        let new_fund_values: Vec<f64> = report
            .plan
            .iter()
            .map(|line| line.holding.current_value - line.proceeds)
            .collect();
        let fund_value_after: f64 = new_fund_values.iter().sum();
        let proceeds = report.total_proceeds();
        let shortfall = report.shortfall();

        let cash_after = report.cash_value + proceeds - shortfall;
        let total_before = report.total_portfolio();
        let total_after = fund_value_after + cash_after;

        let funds = report
            .plan
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let value_before = line.holding.current_value;
                let value_after = new_fund_values[index];
                FundImpact {
                    symbol: line.holding.symbol.clone(),
                    label: line.holding.description.clone(),
                    value_before,
                    value_after,
                    weight_before: pct_of(value_before, total_before),
                    weight_after: pct_of(value_after, total_after),
                }
            })
            .collect();

        Self {
            total_before,
            total_after,
            fund_value_before,
            fund_value_after,
            cash_before: report.cash_value,
            cash_after,
            funds,
        }
    }

    pub fn from_buy(report: &BuyReport) -> Self {
        let fund_value_before = report.total_fund_value();
        let total_before = report.total_portfolio();
        let leftover = report.leftover();
        let cash_after = report.cash_value + report.deposit - leftover;
        let fund_value_after: f64 = report
            .plan
            .iter()
            .map(|line| line.holding.current_value + line.cost)
            .sum();
        let total_after = fund_value_after + cash_after;
        let funds = report
            .plan
            .iter()
            .map(|line| {
                let value_before = line.holding.current_value;
                let value_after = value_before + line.cost;
                FundImpact {
                    symbol: line.holding.symbol.clone(),
                    label: line.holding.description.clone(),
                    value_before,
                    value_after,
                    weight_before: pct_of(value_before, total_before),
                    weight_after: pct_of(value_after, total_after),
                }
            })
            .collect();

        Self {
            total_before,
            total_after,
            fund_value_before,
            fund_value_after,
            cash_before: report.cash_value,
            cash_after,
            funds,
        }
    }

    pub fn from_rebalance(report: &RebalanceReport) -> Self {
        let fund_value_before = report.total_fund_value();
        let total_before = report.total_portfolio();
        let cash_after = report.cash_leftover();
        let fund_value_after: f64 = report
            .plan
            .iter()
            .map(|line| match line.side {
                super::types::TradeSide::Buy => line.holding.current_value + line.amount,
                super::types::TradeSide::Sell => line.holding.current_value - line.amount,
                super::types::TradeSide::Hold => line.holding.current_value,
            })
            .sum();
        let total_after = fund_value_after + cash_after;

        let funds = report
            .plan
            .iter()
            .map(|line| {
                let value_before = line.holding.current_value;
                let value_after = match line.side {
                    super::types::TradeSide::Buy => value_before + line.amount,
                    super::types::TradeSide::Sell => value_before - line.amount,
                    super::types::TradeSide::Hold => value_before,
                };
                FundImpact {
                    symbol: line.holding.symbol.clone(),
                    label: line.holding.description.clone(),
                    value_before,
                    value_after,
                    weight_before: line.weight_before,
                    weight_after: line.weight_after,
                }
            })
            .collect();

        Self {
            total_before,
            total_after,
            fund_value_before,
            fund_value_after,
            cash_before: report.cash_value,
            cash_after,
            funds,
        }
    }
}

fn pct_of(value: f64, total: f64) -> f64 {
    if total > 0.0 {
        value / total * 100.0
    } else {
        0.0
    }
}
