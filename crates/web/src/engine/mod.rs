mod health;
mod notify;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alerts::Notifier;
use chrono::{DateTime, Utc};
use domain::{Itinerary, ProviderQuery, SavedSearch, SearchId};
use geo::GeoIndex;
use planner::{Planner, dedup};
use providers::FlightProvider;
use storage::{Db, NewQueryRun, NewSearchRun};
use tokio::sync::Mutex;
use tokio::time::Instant;

pub use health::HealthEvent;

#[derive(Clone, Debug)]
pub struct EngineSettings {
    /// Identical queries newer than this are answered from storage.
    pub cache_ttl: chrono::Duration,
    pub verify_top_n: usize,
    /// Base URL of the web UI, used for links in notifications.
    pub public_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CycleReport {
    pub searches: usize,
    pub executed: usize,
    pub cached: usize,
    pub failed: usize,
    pub alerts: usize,
}

#[derive(Clone, Default)]
struct Fetched {
    itineraries: Vec<Itinerary>,
    failed: bool,
}

/// Runs saved searches end to end: plan, scan, verify, store, alert.
pub struct Engine {
    pub db: Db,
    pub geo: Arc<GeoIndex>,
    live: Arc<dyn FlightProvider>,
    scan: Option<Arc<dyn FlightProvider>>,
    notifier: Arc<dyn Notifier>,
    settings: EngineSettings,
    last_call: Mutex<HashMap<String, Instant>>,
    cycle: Mutex<()>,
}

impl Engine {
    pub fn new(
        db: Db,
        geo: Arc<GeoIndex>,
        live: Arc<dyn FlightProvider>,
        scan: Option<Arc<dyn FlightProvider>>,
        notifier: Arc<dyn Notifier>,
        settings: EngineSettings,
    ) -> Self {
        Self {
            db,
            geo,
            live,
            scan,
            notifier,
            settings,
            last_call: Mutex::new(HashMap::new()),
            cycle: Mutex::new(()),
        }
    }

    pub fn planner(&self) -> Planner<'_> {
        Planner {
            geo: &self.geo,
            live: self.live.capabilities(),
            scan: self.scan.as_ref().map(|s| s.capabilities()),
            verify_top_n: self.settings.verify_top_n,
        }
    }

    pub fn providers(&self) -> Vec<Arc<dyn FlightProvider>> {
        std::iter::once(self.live.clone())
            .chain(self.scan.clone())
            .collect()
    }

    pub fn notifier(&self) -> &Arc<dyn Notifier> {
        &self.notifier
    }

    pub fn is_running(&self) -> bool {
        self.cycle.try_lock().is_err()
    }

    pub async fn run_due(&self) -> anyhow::Result<CycleReport> {
        let due: Vec<SavedSearch> = self
            .db
            .due_searches(Utc::now())
            .await?
            .into_iter()
            .map(|r| r.search)
            .collect();
        if due.is_empty() {
            return Ok(CycleReport::default());
        }
        self.run_searches(due).await
    }

    /// One cycle over `searches`, sharing identical queries between them.
    pub async fn run_searches(&self, searches: Vec<SavedSearch>) -> anyhow::Result<CycleReport> {
        let _cycle = self.cycle.lock().await;
        let started = Utc::now();
        let mut report = CycleReport {
            searches: searches.len(),
            ..CycleReport::default()
        };
        let planner = self.planner();
        let history = self.db.query_history().await?;

        let mut runnable = Vec::new();
        let mut scan_plans = Vec::new();
        for search in searches {
            match planner.scan_queries(&search.definition) {
                Ok(queries) => {
                    scan_plans.push((search.id, queries));
                    runnable.push(search);
                }
                Err(error) => {
                    tracing::warn!(search = search.definition.name, %error, "search cannot be planned");
                    self.reschedule(&search, started).await?;
                }
            }
        }
        let scanned = match &self.scan {
            Some(scan) => self.execute(scan, &dedup(&scan_plans), &mut report).await?,
            None => HashMap::new(),
        };

        let mut live_plans = Vec::new();
        for (search, (_, scans)) in runnable.iter().zip(&scan_plans) {
            let quotes = gather(scans, &scanned).0;
            let plan = planner.live_queries(&search.definition, &quotes, &history)?;
            live_plans.push((search.id, plan.queries));
        }
        let fetched = self
            .execute(&self.live, &dedup(&live_plans), &mut report)
            .await?;

        for (search, (_, queries)) in runnable.iter().zip(&live_plans) {
            let (itineraries, failed) = gather(queries, &fetched);
            if self
                .finish(search, started, queries.len(), failed, itineraries)
                .await?
            {
                report.alerts += 1;
            }
        }
        tracing::info!(?report, "cycle finished");
        Ok(report)
    }

    async fn finish(
        &self,
        search: &SavedSearch,
        started: DateTime<Utc>,
        planned: usize,
        failed: usize,
        itineraries: Vec<Itinerary>,
    ) -> anyhow::Result<bool> {
        let def = &search.definition;
        let resolved = planner::resolve(def, &self.geo)?;
        let outcome = planner::build_options(def, &resolved, &itineraries, &self.geo);
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let run_id = self
            .db
            .record_search_run(NewSearchRun {
                search_id: search.id,
                started_at: started,
                finished_at: Utc::now(),
                queries_planned: count(planned),
                queries_executed: count(planned - failed),
                queries_failed: count(failed),
                currency: &def.currency,
                options: &outcome.options,
                calendar: &outcome.calendar,
            })
            .await?;
        let alerted = match outcome.options.iter().min_by_key(|o| o.total_price) {
            Some(best) => self.alert(search, best, run_id).await?,
            None => false,
        };
        self.reschedule(search, started).await?;
        Ok(alerted)
    }

    async fn reschedule(&self, search: &SavedSearch, ran_at: DateTime<Utc>) -> anyhow::Result<()> {
        let def = &search.definition;
        let recent: Vec<_> = self
            .db
            .run_points(search.id, 6)
            .await?
            .into_iter()
            .filter_map(|p| p.best_price)
            .collect();
        let next = planner::next_run(&def.policy, Utc::now(), def.outbound.first(), &recent);
        self.db.mark_search_ran(search.id, ran_at, next).await?;
        Ok(())
    }

    async fn execute(
        &self,
        provider: &Arc<dyn FlightProvider>,
        tasks: &[(ProviderQuery, Vec<SearchId>)],
        report: &mut CycleReport,
    ) -> anyhow::Result<HashMap<ProviderQuery, Fetched>> {
        let mut results = HashMap::new();
        for (query, _) in tasks {
            let key = query.key();
            let fresh_since = Utc::now() - self.settings.cache_ttl;
            if let Some(itineraries) = self.db.cached_itineraries(&key, fresh_since).await? {
                report.cached += 1;
                results.insert(
                    query.clone(),
                    Fetched {
                        itineraries,
                        failed: false,
                    },
                );
                continue;
            }
            self.pace(provider.as_ref()).await;
            let started_at = Utc::now();
            let clock = Instant::now();
            let outcome = provider.search(query).await;
            let duration_ms = i64::try_from(clock.elapsed().as_millis()).unwrap_or(i64::MAX);
            let id = provider.id();
            let (itineraries, error) = match outcome {
                Ok(itineraries) => (itineraries, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            let message = error.as_ref().map(ToString::to_string);
            let event = match &error {
                Some(error) => HealthEvent::Failure(error.clone()),
                None if itineraries.is_empty()
                    && self.db.query_usually_has_results(&key).await? =>
                {
                    HealthEvent::SuspiciousEmpty
                }
                None => HealthEvent::Answered,
            };
            self.db
                .record_query_run(NewQueryRun {
                    provider: &id,
                    query_key: &key,
                    description: &query.describe(),
                    started_at,
                    duration_ms,
                    outcome: error.as_ref().map_or("ok", |e| e.kind()),
                    error: message.as_deref(),
                    itineraries: &itineraries,
                })
                .await?;
            health::record(&self.db, self.notifier.as_ref(), id.as_str(), event).await?;
            if error.is_some() {
                report.failed += 1;
            } else {
                report.executed += 1;
            }
            results.insert(
                query.clone(),
                Fetched {
                    itineraries,
                    failed: error.is_some(),
                },
            );
        }
        Ok(results)
    }

    /// Waits until the provider's minimum interval since its previous call has passed.
    async fn pace(&self, provider: &dyn FlightProvider) {
        let interval = provider.capabilities().min_interval;
        let mut last = self.last_call.lock().await;
        let id = provider.id().to_string();
        if let Some(previous) = last.get(&id) {
            let ready = *previous + interval;
            if ready > Instant::now() {
                tokio::time::sleep_until(ready).await;
            }
        }
        last.insert(id, Instant::now());
    }
}

/// Itineraries answering `queries` (deduplicated) and how many of them failed.
fn gather(
    queries: &[ProviderQuery],
    results: &HashMap<ProviderQuery, Fetched>,
) -> (Vec<Itinerary>, usize) {
    let mut seen = HashSet::new();
    let mut itineraries = Vec::new();
    let mut failed = 0;
    for query in queries {
        let Some(fetched) = results.get(query) else {
            continue;
        };
        if fetched.failed {
            failed += 1;
        }
        for it in &fetched.itineraries {
            if seen.insert(it.key()) {
                itineraries.push(it.clone());
            }
        }
    }
    (itineraries, failed)
}
