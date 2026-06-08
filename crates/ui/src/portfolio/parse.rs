use std::io::Cursor;

use super::types::Holding;

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
    if negative {
        -parsed
    } else {
        parsed
    }
}

/// Strip quantity formatting and return a float.
pub fn parse_quantity(value: &str) -> f64 {
    let cleaned: String = value
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '^' | '*'))
        .collect();
    cleaned.parse().unwrap_or(0.0)
}

/// Parse a positive dollar amount typed in the UI (`10,000`, `$10000`, etc.).
pub fn parse_positive_amount(input: &str, label: &str) -> Result<f64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(format!("Enter a {label} amount."));
    }
    let amount = parse_dollar(trimmed);
    if amount <= 0.0 {
        return Err(format!("{label} must be greater than zero."));
    }
    Ok(amount)
}

/// Parse a withdrawal amount typed in the UI.
pub fn parse_withdrawal(input: &str) -> Result<f64, String> {
    parse_positive_amount(input, "withdrawal")
}

/// Parse a deposit amount typed in the UI.
pub fn parse_deposit(input: &str) -> Result<f64, String> {
    parse_positive_amount(input, "deposit")
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
