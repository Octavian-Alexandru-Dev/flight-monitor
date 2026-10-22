use chrono::{DateTime, Duration, NaiveDate, Utc};
use domain::{Decimal, MonitorPolicy};
use rust_decimal::prelude::ToPrimitive;

const MIN_MINUTES: f64 = 30.0;
const MAX_MINUTES: f64 = 48.0 * 60.0;

/// When to run a search again: the base interval, scaled when adaptive polling
/// is on — faster near departure or with volatile prices, slower when far and stable.
pub fn next_run(
    policy: &MonitorPolicy,
    now: DateTime<Utc>,
    earliest_departure: NaiveDate,
    recent_best: &[Decimal],
) -> DateTime<Utc> {
    let base = f64::from(policy.poll_interval_minutes);
    let minutes = if policy.adaptive {
        let days = (earliest_departure - now.date_naive()).num_days();
        let proximity = match days {
            ..=7 => 0.25,
            8..=21 => 0.5,
            22..=90 => 1.0,
            _ => 1.5,
        };
        (base * proximity * volatility_factor(recent_best)).clamp(MIN_MINUTES, MAX_MINUTES)
    } else {
        base
    };
    now + Duration::minutes(minutes.round() as i64)
}

fn volatility_factor(prices: &[Decimal]) -> f64 {
    let values: Vec<f64> = prices
        .iter()
        .rev()
        .take(6)
        .filter_map(ToPrimitive::to_f64)
        .collect();
    if values.len() < 3 {
        return 1.0;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    if mean <= 0.0 {
        return 1.0;
    }
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let cv = variance.sqrt() / mean;
    if cv > 0.05 {
        0.6
    } else if cv < 0.005 && values.len() >= 4 {
        1.5
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(adaptive: bool) -> MonitorPolicy {
        MonitorPolicy {
            poll_interval_minutes: 240,
            query_budget: 10,
            adaptive,
        }
    }

    fn minutes_until(next: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
        (next - now).num_minutes()
    }

    #[test]
    fn fixed_interval_without_adaptive() {
        let now = Utc::now();
        let soon = now.date_naive() + Duration::days(2);
        assert_eq!(
            minutes_until(next_run(&policy(false), now, soon, &[]), now),
            240
        );
    }

    #[test]
    fn polls_faster_near_departure_and_slower_far_away() {
        let now = Utc::now();
        let near = now.date_naive() + Duration::days(3);
        let far = now.date_naive() + Duration::days(200);
        assert_eq!(
            minutes_until(next_run(&policy(true), now, near, &[]), now),
            60
        );
        assert_eq!(
            minutes_until(next_run(&policy(true), now, far, &[]), now),
            360
        );
    }

    #[test]
    fn volatile_prices_speed_up_stable_prices_slow_down() {
        let now = Utc::now();
        let mid = now.date_naive() + Duration::days(40);
        let volatile = [500, 420, 560, 380].map(Decimal::from);
        let stable = [500, 500, 500, 500].map(Decimal::from);
        assert_eq!(
            minutes_until(next_run(&policy(true), now, mid, &volatile), now),
            144
        );
        assert_eq!(
            minutes_until(next_run(&policy(true), now, mid, &stable), now),
            360
        );
    }
}
