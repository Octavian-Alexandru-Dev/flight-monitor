use alerts::{Baseline, RunContext, alert_message, evaluate};
use domain::{Decimal, SavedSearch, TripOption};
use rust_decimal::prelude::ToPrimitive;
use storage::NewAlert;

use super::Engine;

const BASELINE_RUNS: i64 = 200;

impl Engine {
    /// Evaluates the search's alert rules on its best option; sends at most one message.
    pub(super) async fn alert(
        &self,
        search: &SavedSearch,
        best: &TripOption,
        run_id: i64,
    ) -> anyhow::Result<bool> {
        let def = &search.definition;
        if def.alerts.is_empty() {
            return Ok(false);
        }
        let context = RunContext {
            best_price: best.total_price,
            previous_best: self.db.previous_best(search.id, run_id).await?,
            historic_min: self.db.historic_min(search.id, run_id).await?,
            baseline: self.baseline(search, best, run_id).await?,
        };
        let triggers = evaluate(&def.alerts, &context);
        if triggers.is_empty() {
            return Ok(false);
        }
        let key = best.key();
        if self
            .db
            .alert_already_sent(search.id, &key, best.total_price)
            .await?
        {
            return Ok(false);
        }
        let link = self
            .settings
            .public_url
            .as_ref()
            .map(|base| format!("{}/searches/{}", base.trim_end_matches('/'), search.id.0));
        let message = alert_message(&def.name, best, &triggers, link.as_deref());
        if let Err(error) = self.notifier.send(&message).await {
            tracing::warn!(%error, "alert not delivered, will retry next cycle");
            return Ok(false);
        }
        let reasons: Vec<&str> = triggers.iter().map(|t| t.code()).collect();
        self.db
            .record_alert(NewAlert {
                search_id: search.id,
                option_key: &key,
                price: best.total_price,
                reasons: &reasons.join(","),
                message: &message,
            })
            .await?;
        Ok(true)
    }

    /// Route history for a simple one-way ticket, otherwise the search's own history.
    async fn baseline(
        &self,
        search: &SavedSearch,
        best: &TripOption,
        run_id: i64,
    ) -> anyhow::Result<Option<Baseline>> {
        if let [ticket] = best.tickets.as_slice()
            && ticket.itinerary.return_date.is_none()
        {
            let it = &ticket.itinerary;
            let stats = self
                .db
                .route_stats(it.origin(), it.destination(), &it.currency)
                .await?;
            return Ok(Some(Baseline {
                mean: stats.mean,
                std_dev: stats.std_dev,
                samples: stats.observations,
            }));
        }
        let prices: Vec<f64> = self
            .db
            .run_points(search.id, BASELINE_RUNS)
            .await?
            .into_iter()
            .filter(|p| p.run_id != run_id)
            .filter_map(|p| p.best_price.and_then(|d| d.to_f64()))
            .collect();
        if prices.is_empty() {
            return Ok(None);
        }
        let n = prices.len() as f64;
        let mean = prices.iter().sum::<f64>() / n;
        let std_dev = (prices.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / n).sqrt();
        let decimal = |v: f64| Decimal::from_f64_retain(v).unwrap_or_default().round_dp(2);
        Ok(Some(Baseline {
            mean: decimal(mean),
            std_dev: decimal(std_dev),
            samples: prices.len() as i64,
        }))
    }
}
