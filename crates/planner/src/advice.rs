use domain::Decimal;

const MIN_HISTORY: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    BuyNow,
    Wait,
    Watch,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::BuyNow => "Compra ora",
            Self::Wait => "Aspetta",
            Self::Watch => "Monitora",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Advice {
    pub verdict: Verdict,
    pub reason: String,
}

/// A heuristic buy-or-wait suggestion from a search's price history (oldest first).
pub fn advise(history: &[Decimal], current: Decimal, days_to_departure: i64) -> Advice {
    let advice = |verdict, reason: String| Advice { verdict, reason };
    if days_to_departure < 0 {
        return advice(Verdict::Watch, "La data di partenza è passata".into());
    }
    if days_to_departure <= 14 {
        return advice(
            Verdict::BuyNow,
            "Mancano meno di due settimane: i prezzi tendono a salire".into(),
        );
    }
    if history.len() < MIN_HISTORY {
        return advice(
            Verdict::Watch,
            format!("Storico ancora breve ({} rilevamenti)", history.len()),
        );
    }
    let cheaper = history.iter().filter(|p| **p < current).count();
    let percentile = cheaper * 100 / history.len();
    let trend = trend_percent(history);
    if percentile <= 20 {
        advice(
            Verdict::BuyNow,
            format!("Prezzo nel {}% più basso mai osservato", percentile.max(1)),
        )
    } else if trend <= -3 && days_to_departure > 45 {
        advice(
            Verdict::Wait,
            format!("Prezzi in calo del {}% negli ultimi rilevamenti", -trend),
        )
    } else if trend >= 3 {
        advice(
            Verdict::BuyNow,
            format!("Prezzi in salita del {trend}% negli ultimi rilevamenti"),
        )
    } else {
        advice(
            Verdict::Watch,
            format!("Prezzo nella media: più alto del {percentile}% dei rilevamenti"),
        )
    }
}

/// Change between the first and last third of the recent history, in percent.
fn trend_percent(history: &[Decimal]) -> i64 {
    let recent: Vec<Decimal> = history.iter().rev().take(12).rev().copied().collect();
    let third = (recent.len() / 3).max(1);
    let avg =
        |slice: &[Decimal]| slice.iter().copied().sum::<Decimal>() / Decimal::from(slice.len());
    let (early, late) = (avg(&recent[..third]), avg(&recent[recent.len() - third..]));
    if early.is_zero() {
        return 0;
    }
    ((late - early) * Decimal::ONE_HUNDRED / early)
        .round()
        .to_string()
        .parse()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prices(values: &[i64]) -> Vec<Decimal> {
        values.iter().copied().map(Decimal::from).collect()
    }

    #[test]
    fn buys_close_to_departure() {
        assert_eq!(advise(&[], Decimal::from(100), 5).verdict, Verdict::BuyNow);
    }

    #[test]
    fn needs_history() {
        assert_eq!(
            advise(&prices(&[1, 2]), Decimal::from(100), 60).verdict,
            Verdict::Watch
        );
    }

    #[test]
    fn buys_at_historic_lows() {
        let history = prices(&[500, 520, 480, 510, 530, 505]);
        assert_eq!(
            advise(&history, Decimal::from(470), 90).verdict,
            Verdict::BuyNow
        );
    }

    #[test]
    fn waits_when_prices_fall_far_from_departure() {
        let history = prices(&[600, 590, 580, 560, 540, 520, 500, 480, 470]);
        assert_eq!(
            advise(&history, Decimal::from(500), 120).verdict,
            Verdict::Wait
        );
    }

    #[test]
    fn buys_when_prices_rise() {
        let history = prices(&[400, 410, 420, 450, 480, 500]);
        assert_eq!(
            advise(&history, Decimal::from(480), 90).verdict,
            Verdict::BuyNow
        );
    }
}
