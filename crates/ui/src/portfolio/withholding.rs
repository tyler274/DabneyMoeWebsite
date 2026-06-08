/// IRA distribution tax withholding rates (percent of gross distribution).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaxWithholding {
    pub federal_pct: f64,
    pub state_pct: f64,
}

impl TaxWithholding {
    pub const NY_DEFAULT: Self = Self {
        federal_pct: 20.0,
        state_pct: 7.0,
    };

    pub fn total_pct(&self) -> f64 {
        self.federal_pct + self.state_pct
    }

    /// Gross distribution needed so net proceeds equal `net_withdrawal` after withholding.
    pub fn gross_from_net(&self, net_withdrawal: f64) -> Result<f64, String> {
        let rate = self.total_pct() / 100.0;
        if rate <= 0.0 {
            return Ok(net_withdrawal);
        }
        if rate >= 1.0 {
            return Err(format!(
                "Combined withholding ({:.1}%) must be less than 100%.",
                self.total_pct()
            ));
        }
        Ok(net_withdrawal / (1.0 - rate))
    }

    /// Withholding amounts applied to a gross distribution.
    pub fn apply_to_gross(&self, gross: f64) -> WithholdingBreakdown {
        let federal = gross * self.federal_pct / 100.0;
        let state = gross * self.state_pct / 100.0;
        WithholdingBreakdown {
            gross,
            federal,
            state,
            net: gross - federal - state,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WithholdingBreakdown {
    pub gross: f64,
    pub federal: f64,
    pub state: f64,
    pub net: f64,
}

/// Parse a withholding percentage from UI input (e.g. `20`, `7.5`).
pub fn parse_withholding_pct(input: &str, label: &str) -> Result<f64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(format!("Enter a {label} withholding rate."));
    }
    let pct = trimmed
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| format!("'{trimmed}' is not a valid {label} rate."))?;
    if !(0.0..100.0).contains(&pct) {
        return Err(format!("{label} rate must be between 0% and 100%."));
    }
    Ok(pct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ny_default_gross_from_net() {
        let w = TaxWithholding::NY_DEFAULT;
        let gross = w.gross_from_net(10_000.0).unwrap();
        assert!((gross - 13_698.63).abs() < 0.01);

        let breakdown = w.apply_to_gross(gross);
        assert!((breakdown.net - 10_000.0).abs() < 0.02);
        assert!((breakdown.federal - gross * 0.20).abs() < 0.01);
        assert!((breakdown.state - gross * 0.07).abs() < 0.01);
    }

    #[test]
    fn rejects_withholding_at_or_above_100_percent() {
        let w = TaxWithholding {
            federal_pct: 80.0,
            state_pct: 25.0,
        };
        assert!(w.gross_from_net(1000.0).is_err());
    }
}
