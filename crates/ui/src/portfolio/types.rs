use super::withholding::WithholdingBreakdown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    Taxable,
    TraditionalIra,
    RothIra,
}

impl AccountType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Taxable => "Taxable brokerage",
            Self::TraditionalIra => "Traditional IRA",
            Self::RothIra => "Roth IRA",
        }
    }

    pub fn is_ira(self) -> bool {
        matches!(self, Self::TraditionalIra | Self::RothIra)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Holding {
    pub description: String,
    pub symbol: String,
    pub quantity: f64,
    pub price: f64,
    pub current_value: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeSide {
    Buy,
    Sell,
    Hold,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SellLine {
    pub holding: Holding,
    pub shares_to_sell: u32,
    pub proceeds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SellReport {
    pub account_type: AccountType,
    pub holdings: Vec<Holding>,
    pub cash_value: f64,
    /// Amount used to compute the sell plan (gross distribution when withholding applies).
    pub withdrawal: f64,
    /// User-entered net target when IRA withholding is enabled.
    pub net_withdrawal: Option<f64>,
    pub withholding: Option<WithholdingBreakdown>,
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

    pub fn weight_before(&self, index: usize) -> f64 {
        let total = self.total_fund_value();
        if total > 0.0 {
            self.holdings[index].current_value / total * 100.0
        } else {
            0.0
        }
    }

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

#[derive(Debug, Clone, PartialEq)]
pub struct BuyLine {
    pub holding: Holding,
    pub shares_to_buy: u32,
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuyReport {
    pub holdings: Vec<Holding>,
    pub cash_value: f64,
    pub deposit: f64,
    pub plan: Vec<BuyLine>,
}

impl BuyReport {
    pub fn total_fund_value(&self) -> f64 {
        self.holdings.iter().map(|h| h.current_value).sum()
    }

    pub fn total_portfolio(&self) -> f64 {
        self.total_fund_value() + self.cash_value
    }

    pub fn total_cost(&self) -> f64 {
        self.plan.iter().map(|l| l.cost).sum()
    }

    pub fn leftover(&self) -> f64 {
        self.deposit - self.total_cost()
    }

    pub fn weight_before(&self, index: usize) -> f64 {
        weight_pct(self.holdings[index].current_value, self.total_portfolio())
    }

    pub fn weight_after(&self, index: usize) -> f64 {
        let new_value = self.holdings[index].current_value + self.plan[index].cost;
        weight_pct(new_value, self.total_portfolio())
    }

    pub fn weight_delta(&self, index: usize) -> f64 {
        self.weight_after(index) - self.weight_before(index)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceLine {
    pub holding: Holding,
    pub side: TradeSide,
    pub shares: u32,
    pub amount: f64,
    pub weight_before: f64,
    pub weight_after: f64,
    pub target_value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RebalanceReport {
    pub holdings: Vec<Holding>,
    pub cash_value: f64,
    pub plan: Vec<RebalanceLine>,
}

impl RebalanceReport {
    pub fn total_fund_value(&self) -> f64 {
        self.holdings.iter().map(|h| h.current_value).sum()
    }

    pub fn total_portfolio(&self) -> f64 {
        self.total_fund_value() + self.cash_value
    }

    pub fn total_buys(&self) -> f64 {
        self.plan
            .iter()
            .filter(|l| l.side == TradeSide::Buy)
            .map(|l| l.amount)
            .sum()
    }

    pub fn total_sells(&self) -> f64 {
        self.plan
            .iter()
            .filter(|l| l.side == TradeSide::Sell)
            .map(|l| l.amount)
            .sum()
    }

    pub fn cash_leftover(&self) -> f64 {
        self.cash_value + self.total_sells() - self.total_buys()
    }
}

pub(crate) fn weight_pct(value: f64, total: f64) -> f64 {
    if total > 0.0 {
        value / total * 100.0
    } else {
        0.0
    }
}
