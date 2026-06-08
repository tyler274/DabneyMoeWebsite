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

/// Allocate a dollar amount across funds proportionally, returning whole-share
/// counts per fund (largest-remainder method, matching the Python sell script).
pub(crate) fn allocate_whole_shares(holdings: &[Holding], amount: f64) -> Result<Vec<u32>, String> {
    let weights = fund_weights(holdings)?;

    let raw_shares: Vec<f64> = holdings
        .iter()
        .zip(weights.iter())
        .map(|(h, weight)| {
            if h.price > 0.0 {
                amount * weight / h.price
            } else {
                0.0
            }
        })
        .collect();

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
