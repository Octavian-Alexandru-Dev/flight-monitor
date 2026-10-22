use domain::{AlertRule, Decimal};

/// Minimum samples before an average is trusted for anomaly detection.
const MIN_BASELINE_SAMPLES: i64 = 10;
const ANOMALY_SIGMAS: i64 = 2;

/// Historical price distribution the current price is compared against.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    pub mean: Decimal,
    pub std_dev: Decimal,
    pub samples: i64,
}

/// Facts about a finished run that the rules are evaluated against.
#[derive(Clone, Debug, PartialEq)]
pub struct RunContext {
    pub best_price: Decimal,
    pub previous_best: Option<Decimal>,
    /// Lowest price of all earlier runs.
    pub historic_min: Option<Decimal>,
    pub baseline: Option<Baseline>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Trigger {
    BelowPrice { threshold: Decimal },
    Drop { percent: Decimal, previous: Decimal },
    NewMinimum { previous: Decimal },
    Anomaly { mean: Decimal },
}

impl Trigger {
    pub fn is_priority(&self) -> bool {
        matches!(self, Self::Anomaly { .. })
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::BelowPrice { .. } => "below_price",
            Self::Drop { .. } => "drop",
            Self::NewMinimum { .. } => "new_minimum",
            Self::Anomaly { .. } => "anomaly",
        }
    }
}

pub fn evaluate(rules: &[AlertRule], ctx: &RunContext) -> Vec<Trigger> {
    let best = ctx.best_price;
    rules
        .iter()
        .filter_map(|rule| match rule {
            AlertRule::BelowPrice(threshold) => {
                (best <= *threshold).then_some(Trigger::BelowPrice {
                    threshold: *threshold,
                })
            }
            AlertRule::DropPercent(percent) => ctx.previous_best.and_then(|previous| {
                let floor = previous * (Decimal::ONE_HUNDRED - percent) / Decimal::ONE_HUNDRED;
                (best <= floor && best < previous).then(|| Trigger::Drop {
                    percent: ((previous - best) * Decimal::ONE_HUNDRED / previous).round_dp(1),
                    previous,
                })
            }),
            AlertRule::NewHistoricMinimum => ctx
                .historic_min
                .filter(|min| best < *min)
                .map(|previous| Trigger::NewMinimum { previous }),
            AlertRule::PriceAnomaly => ctx.baseline.as_ref().and_then(|b| {
                let anomalous = b.samples >= MIN_BASELINE_SAMPLES
                    && !b.std_dev.is_zero()
                    && best < b.mean - b.std_dev * Decimal::from(ANOMALY_SIGMAS);
                anomalous.then_some(Trigger::Anomaly { mean: b.mean })
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(best: i64) -> RunContext {
        RunContext {
            best_price: Decimal::from(best),
            previous_best: Some(Decimal::from(600)),
            historic_min: Some(Decimal::from(550)),
            baseline: Some(Baseline {
                mean: Decimal::from(700),
                std_dev: Decimal::from(50),
                samples: 30,
            }),
        }
    }

    fn all_rules() -> Vec<AlertRule> {
        vec![
            AlertRule::BelowPrice(Decimal::from(500)),
            AlertRule::DropPercent(Decimal::from(10)),
            AlertRule::NewHistoricMinimum,
            AlertRule::PriceAnomaly,
        ]
    }

    #[test]
    fn nothing_fires_without_improvement() {
        assert!(evaluate(&all_rules(), &ctx(610)).is_empty());
    }

    #[test]
    fn every_rule_fires_on_a_deep_drop() {
        let fired = evaluate(&all_rules(), &ctx(480));
        let codes: Vec<&str> = fired.iter().map(Trigger::code).collect();
        assert_eq!(codes, ["below_price", "drop", "new_minimum", "anomaly"]);
        assert_eq!(
            fired[1],
            Trigger::Drop {
                percent: Decimal::from(20),
                previous: Decimal::from(600)
            }
        );
    }

    #[test]
    fn small_drop_does_not_reach_percentage() {
        let fired = evaluate(&[AlertRule::DropPercent(Decimal::from(10))], &ctx(560));
        assert!(fired.is_empty());
    }

    #[test]
    fn first_run_has_no_history_to_beat() {
        let first = RunContext {
            best_price: Decimal::from(100),
            previous_best: None,
            historic_min: None,
            baseline: None,
        };
        let rules = [
            AlertRule::DropPercent(Decimal::from(5)),
            AlertRule::NewHistoricMinimum,
            AlertRule::PriceAnomaly,
        ];
        assert!(evaluate(&rules, &first).is_empty());
    }

    #[test]
    fn anomaly_needs_enough_samples() {
        let mut thin = ctx(400);
        if let Some(b) = thin.baseline.as_mut() {
            b.samples = 3;
        }
        assert!(evaluate(&[AlertRule::PriceAnomaly], &thin).is_empty());
    }
}
