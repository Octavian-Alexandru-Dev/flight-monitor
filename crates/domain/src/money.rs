use rust_decimal::Decimal;

/// Whole-unit price with thousands grouping, as shown everywhere in the app: `€1.364`.
pub fn format_money(amount: Decimal, currency: &str) -> String {
    let rounded = amount.round_dp(0).normalize();
    let digits = rounded.abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(c);
    }
    let sign = if rounded.is_sign_negative() { "-" } else { "" };
    match currency {
        "EUR" => format!("{sign}€{grouped}"),
        "USD" => format!("{sign}${grouped}"),
        "GBP" => format!("{sign}£{grouped}"),
        _ => format!("{sign}{grouped} {currency}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands_and_rounds() {
        assert_eq!(format_money(Decimal::from(1364), "EUR"), "€1.364");
        assert_eq!(format_money(Decimal::new(4999, 2), "EUR"), "€50");
        assert_eq!(format_money(Decimal::from(123_456), "JPY"), "123.456 JPY");
        assert_eq!(format_money(Decimal::from(-24), "EUR"), "-€24");
    }
}
