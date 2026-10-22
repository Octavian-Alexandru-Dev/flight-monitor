use domain::Decimal;
use rust_decimal::prelude::ToPrimitive;

/// Prices are stored as integer minor units so SQLite can aggregate them exactly.
pub fn to_cents(price: Decimal) -> i64 {
    (price * Decimal::ONE_HUNDRED)
        .round()
        .to_i64()
        .unwrap_or(i64::MAX)
}

pub fn from_cents(cents: i64) -> Decimal {
    Decimal::new(cents, 2).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_prices() {
        let price = Decimal::new(49_99, 2);
        assert_eq!(to_cents(price), 4999);
        assert_eq!(from_cents(4999), price);
        assert_eq!(from_cents(49_000), Decimal::new(490, 0));
    }
}
