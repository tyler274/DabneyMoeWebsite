//! Investment account portfolio calculators: sell, buy, and rebalance.
//!
//! Parses Raymond James CSV exports and computes whole-share trade plans that
//! preserve fund weight ratios. Sell logic is ported from `KlingBot/scripts/ira_sell.py`.

mod allocate;
mod buy;
mod impact;
mod parse;
mod rebalance;
mod sell;
mod types;
mod withholding;

#[cfg(test)]
mod tests;

pub use buy::{analyze_buy, build_buy_report, compute_buy_plan};
pub use impact::{FundImpact, PortfolioImpact};
pub use parse::{
    load_portfolio, parse_deposit, parse_dollar, parse_positive_amount, parse_quantity,
    parse_withdrawal,
};
pub use rebalance::{analyze_rebalance, build_rebalance_report, compute_rebalance_plan};
pub use sell::{analyze_portfolio, analyze_sell, build_sell_report, compute_sell_plan};
pub use types::{
    AccountType, BuyLine, BuyReport, Holding, RebalanceLine, RebalanceReport, SellLine, SellReport,
    TradeSide,
};
pub use withholding::{parse_withholding_pct, TaxWithholding, WithholdingBreakdown};
