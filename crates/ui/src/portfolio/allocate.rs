use super::types::Holding;

pub(crate) fn fund_weights(holdings: &[Holding]) -> Result<Vec<f64>, String> {
    let total_fund_value: f64 = holdings.iter().map(|h| h.current_value).sum();
    if total_fund_value <= 0.0 {
        return Err("No fund holdings found.".into());
    }
    Ok(holdings
        .iter()
        .map(|h| h.current_value / total_fund_value)
        .collect())
}

/// Allocate a dollar amount across funds using explicit weights (must sum to ~1).
pub(crate) fn allocate_whole_shares_weighted(
    holdings: &[Holding],
    amount: f64,
    weights: &[f64],
) -> Result<Vec<u32>, String> {
    if holdings.len() != weights.len() {
        return Err("Weight count does not match holdings.".into());
    }
    let weight_sum: f64 = weights.iter().sum();
    if weight_sum <= 0.0 {
        return Err("Allocation weights must sum to a positive value.".into());
    }

    let normalized: Vec<f64> = weights.iter().map(|w| w / weight_sum).collect();
    let raw_shares: Vec<f64> = holdings
        .iter()
        .zip(normalized.iter())
        .map(|(h, weight)| {
            if h.price > 0.0 {
                amount * weight / h.price
            } else {
                0.0
            }
        })
        .collect();

    allocate_from_raw_shares(holdings, amount, raw_shares)
}

/// Allocate a dollar amount across funds proportionally, returning whole-share
/// counts per fund (largest-remainder method, matching the Python sell script).
pub(crate) fn allocate_whole_shares(holdings: &[Holding], amount: f64) -> Result<Vec<u32>, String> {
    let weights = fund_weights(holdings)?;
    allocate_whole_shares_weighted(holdings, amount, &weights)
}

/// Whole-share sells capped at each fund's held quantity.
pub(crate) fn allocate_sell_shares(
    holdings: &[Holding],
    sell_amounts: &[f64],
) -> Result<Vec<u32>, String> {
    if holdings.len() != sell_amounts.len() {
        return Err("Sell amount count does not match holdings.".into());
    }

    Ok(holdings
        .iter()
        .zip(sell_amounts.iter())
        .map(|(h, amount)| {
            if h.price <= 0.0 || *amount <= 0.0 {
                0
            } else {
                let max_shares = h.quantity.floor() as u32;
                ((amount / h.price).floor() as u32).min(max_shares)
            }
        })
        .collect())
}

fn allocate_from_raw_shares(
    holdings: &[Holding],
    amount: f64,
    raw_shares: Vec<f64>,
) -> Result<Vec<u32>, String> {
    let mut floor_shares: Vec<u32> = raw_shares.iter().map(|r| r.floor() as u32).collect();
    let floor_total: f64 = floor_shares
        .iter()
        .enumerate()
        .map(|(i, shares)| *shares as f64 * holdings[i].price)
        .sum();

    let mut gap = amount - floor_total;
    let mut remainders: Vec<(f64, usize)> = raw_shares
        .iter()
        .enumerate()
        .map(|(i, raw)| (raw - floor_shares[i] as f64, i))
        .collect();
    remainders.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, idx) in remainders {
        let extra = holdings[idx].price;
        if gap >= extra * 0.5 {
            floor_shares[idx] += 1;
            gap -= extra;
        }
        if gap.abs() < 0.01 {
            break;
        }
    }

    Ok(floor_shares)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holding(symbol: &str, value: f64, price: f64, qty: f64) -> Holding {
        Holding {
            description: symbol.into(),
            symbol: symbol.into(),
            quantity: qty,
            price,
            current_value: value,
        }
    }

    #[test]
    fn weighted_allocation_respects_custom_weights() {
        let holdings = vec![
            holding("A", 100.0, 10.0, 20.0),
            holding("B", 100.0, 10.0, 20.0),
        ];
        let shares = allocate_whole_shares_weighted(&holdings, 100.0, &[0.75, 0.25]).unwrap();
        assert_eq!(shares, vec![8, 2]);
    }
}
