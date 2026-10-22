use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use domain::{
    Capabilities, Decimal, Iata, Itinerary, ProviderQuery, QueryLeg, QueryStats, SearchDefinition,
    SearchId,
};
use geo::GeoIndex;

use crate::needs::{LegGroup, Need, needs};
use crate::{PlanError, resolve};

/// Share of a budget spent on historically competitive queries; the rest explores.
const EXPLOIT_PERCENT: usize = 70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Every candidate fits in the budget.
    All,
    /// Over budget: competitive and stale queries first.
    Budget,
    /// Cached prices picked the most promising candidates.
    ScanVerify,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LivePlan {
    pub queries: Vec<ProviderQuery>,
    pub candidates: usize,
    pub strategy: Strategy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    pub origins: Vec<Iata>,
    pub destinations: Option<Vec<Iata>>,
    pub candidates: usize,
    pub live_per_cycle: usize,
    pub scan_per_cycle: usize,
    pub strategy: Strategy,
}

struct Candidate {
    group: LegGroup,
    query: ProviderQuery,
}

pub struct Planner<'a> {
    pub geo: &'a GeoIndex,
    pub live: &'a Capabilities,
    pub scan: Option<&'a Capabilities>,
    pub verify_top_n: usize,
}

impl Planner<'_> {
    /// Cheap cached-data queries to run before the live ones; empty when not worthwhile.
    pub fn scan_queries(&self, def: &SearchDefinition) -> Result<Vec<ProviderQuery>, PlanError> {
        let resolved = resolve(def, self.geo)?;
        let Some(scan) = self.scan else {
            return match resolved.destinations {
                None => Err(PlanError::NeedsScanProvider),
                Some(_) => Ok(Vec::new()),
            };
        };
        let destinations = match &resolved.destinations {
            Some(destinations) => {
                let needs = needs(def, &resolved, destinations);
                if !self.worth_scanning(def, &needs) {
                    return Ok(Vec::new());
                }
                destinations.clone()
            }
            None => Vec::new(),
        };
        let mut queries: Vec<ProviderQuery> = needs(def, &resolved, &destinations)
            .iter()
            .flat_map(as_one_way)
            .filter(|(_, from, ..)| !from.is_empty())
            .flat_map(|(_, from, to, dates, passengers)| {
                expand(def, scan, &from, &to, &dates, passengers)
            })
            .collect();
        queries.sort();
        queries.dedup();
        Ok(queries)
    }

    /// Live queries for this cycle, given the scan quotes (if a scan ran) and run history.
    pub fn live_queries(
        &self,
        def: &SearchDefinition,
        quotes: &[Itinerary],
        history: &HashMap<String, QueryStats>,
    ) -> Result<LivePlan, PlanError> {
        let resolved = resolve(def, self.geo)?;
        let destinations = match &resolved.destinations {
            Some(destinations) => destinations.clone(),
            None => cheapest_destinations(quotes, self.verify_top_n),
        };
        if destinations.is_empty() {
            return Ok(LivePlan {
                queries: Vec::new(),
                candidates: 0,
                strategy: Strategy::ScanVerify,
            });
        }
        let candidates = self.candidates(def, &needs(def, &resolved, &destinations));
        let total = candidates.len();
        let budget = def.policy.query_budget as usize;
        let (selected, strategy) = if !quotes.is_empty() {
            (
                self.verify(candidates, quotes, budget),
                Strategy::ScanVerify,
            )
        } else if total <= budget {
            (candidates, Strategy::All)
        } else {
            (by_history(candidates, budget, history), Strategy::Budget)
        };
        Ok(LivePlan {
            queries: selected.into_iter().map(|c| c.query).collect(),
            candidates: total,
            strategy,
        })
    }

    /// Query counts per cycle, shown when a search is saved.
    pub fn estimate(&self, def: &SearchDefinition) -> Result<Estimate, PlanError> {
        let resolved = resolve(def, self.geo)?;
        let scan_per_cycle = self.scan_queries(def)?.len();
        let budget = def.policy.query_budget as usize;
        let (candidates, live_per_cycle, strategy) = match &resolved.destinations {
            Some(destinations) => {
                let needs = needs(def, &resolved, destinations);
                let candidates = self.candidates(def, &needs);
                let groups = candidates
                    .iter()
                    .map(|c| c.group)
                    .collect::<HashSet<_>>()
                    .len();
                let total = candidates.len();
                if scan_per_cycle > 0 {
                    (
                        total,
                        (self.verify_top_n * groups).min(budget),
                        Strategy::ScanVerify,
                    )
                } else if total <= budget {
                    (total, total, Strategy::All)
                } else {
                    (total, budget, Strategy::Budget)
                }
            }
            None => (0, self.verify_top_n.min(budget), Strategy::ScanVerify),
        };
        Ok(Estimate {
            origins: resolved.origins,
            destinations: resolved.destinations,
            candidates,
            live_per_cycle,
            scan_per_cycle,
            strategy,
        })
    }

    fn worth_scanning(&self, def: &SearchDefinition, needs: &[Need]) -> bool {
        let candidates = self.candidates(def, needs);
        let groups = candidates
            .iter()
            .map(|c| c.group)
            .collect::<HashSet<_>>()
            .len();
        candidates.len() > self.verify_top_n * groups
    }

    fn candidates(&self, def: &SearchDefinition, needs: &[Need]) -> Vec<Candidate> {
        let mut out = Vec::new();
        for need in needs {
            match need {
                Need::OneWay {
                    group,
                    from,
                    to,
                    dates,
                    passengers,
                } => out.extend(
                    expand(def, self.live, from, to, dates, *passengers)
                        .into_iter()
                        .map(|query| Candidate {
                            group: *group,
                            query,
                        }),
                ),
                Need::RoundTrip {
                    from,
                    to,
                    pairs,
                    passengers,
                } => {
                    let size = self.live.max_airports_per_side;
                    for (out_date, back_date) in pairs {
                        for f in chunks(from, size) {
                            for t in chunks(to, size) {
                                let legs = vec![
                                    QueryLeg::on(f.clone(), t.clone(), *out_date),
                                    QueryLeg::on(t.clone(), f.clone(), *back_date),
                                ];
                                out.push(Candidate {
                                    group: LegGroup::RoundTrip,
                                    query: query(def, legs, *passengers),
                                });
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Keeps the best-quoted candidates of each group, never exceeding the budget.
    fn verify(
        &self,
        candidates: Vec<Candidate>,
        quotes: &[Itinerary],
        budget: usize,
    ) -> Vec<Candidate> {
        let mut groups: BTreeMap<LegGroup, Vec<(Decimal, Candidate)>> = BTreeMap::new();
        for candidate in candidates {
            if let Some(price) = score(&candidate.query, quotes) {
                groups
                    .entry(candidate.group)
                    .or_default()
                    .push((price, candidate));
            }
        }
        let mut ranked: Vec<Vec<Candidate>> = groups
            .into_values()
            .map(|mut list| {
                list.sort_by_key(|(price, _)| *price);
                list.into_iter()
                    .take(self.verify_top_n)
                    .map(|(_, c)| c)
                    .collect()
            })
            .collect();
        round_robin(&mut ranked, budget)
    }
}

fn query(def: &SearchDefinition, legs: Vec<QueryLeg>, passengers: u8) -> ProviderQuery {
    ProviderQuery {
        legs,
        max_stops: def.constraints.max_stops,
        passengers,
        currency: def.currency.clone(),
        checked_bags: def.constraints.checked_bags,
    }
}

type OneWayParts = (LegGroup, Vec<Iata>, Vec<Iata>, Vec<NaiveDate>, u8);

/// Scanning treats a round trip as its two one-way directions.
fn as_one_way(need: &Need) -> Vec<OneWayParts> {
    match need {
        Need::OneWay {
            group,
            from,
            to,
            dates,
            passengers,
        } => vec![(*group, from.clone(), to.clone(), dates.clone(), *passengers)],
        Need::RoundTrip {
            from,
            to,
            pairs,
            passengers,
        } => {
            let (mut out, mut back): (Vec<_>, Vec<_>) = pairs.iter().copied().unzip();
            for dates in [&mut out, &mut back] {
                dates.sort();
                dates.dedup();
            }
            vec![
                (
                    LegGroup::Outbound,
                    from.clone(),
                    to.clone(),
                    out,
                    *passengers,
                ),
                (
                    LegGroup::Return,
                    to.clone(),
                    from.clone(),
                    back,
                    *passengers,
                ),
            ]
        }
    }
}

/// One query per airport chunk pair and date (or per month, for range-capable sources).
fn expand(
    def: &SearchDefinition,
    caps: &Capabilities,
    from: &[Iata],
    to: &[Iata],
    dates: &[NaiveDate],
    passengers: u8,
) -> Vec<ProviderQuery> {
    let ranges: Vec<(NaiveDate, NaiveDate)> = if caps.date_range {
        month_ranges(dates)
    } else {
        dates.iter().map(|d| (*d, *d)).collect()
    };
    let size = if caps.multi_airport_query {
        caps.max_airports_per_side
    } else {
        1
    };
    let mut out = Vec::new();
    for (first, last) in ranges {
        for f in chunks(from, size) {
            for t in chunks(to, size) {
                let leg = QueryLeg::between(f.clone(), t, first, last);
                out.push(query(def, vec![leg], passengers));
            }
        }
    }
    out
}

fn chunks(codes: &[Iata], size: usize) -> Vec<Vec<Iata>> {
    if codes.is_empty() {
        return vec![Vec::new()];
    }
    codes.chunks(size.max(1)).map(<[Iata]>::to_vec).collect()
}

fn month_ranges(dates: &[NaiveDate]) -> Vec<(NaiveDate, NaiveDate)> {
    let mut ranges: Vec<(NaiveDate, NaiveDate)> = Vec::new();
    let mut sorted = dates.to_vec();
    sorted.sort();
    for date in sorted {
        match ranges.last_mut() {
            Some((first, last)) if (first.year(), first.month()) == (date.year(), date.month()) => {
                *last = date;
            }
            _ => ranges.push((date, date)),
        }
    }
    ranges
}

fn leg_score(leg: &QueryLeg, quotes: &[Itinerary]) -> Option<Decimal> {
    quotes
        .iter()
        .filter(|q| leg.covers(q.origin(), q.destination(), q.departure_date()))
        .map(|q| q.price)
        .min()
}

/// Estimated price of a candidate. A return leg without quotes (e.g. from a destination
/// discovered by the scan) is assumed to cost as much as the outbound.
fn score(query: &ProviderQuery, quotes: &[Itinerary]) -> Option<Decimal> {
    let (first, rest) = query.legs.split_first()?;
    let outbound = leg_score(first, quotes)?;
    Some(
        outbound
            + rest
                .iter()
                .map(|leg| leg_score(leg, quotes).unwrap_or(outbound))
                .sum::<Decimal>(),
    )
}

fn cheapest_destinations(quotes: &[Itinerary], limit: usize) -> Vec<Iata> {
    let mut best: HashMap<&Iata, Decimal> = HashMap::new();
    for quote in quotes {
        let entry = best.entry(quote.destination()).or_insert(quote.price);
        *entry = (*entry).min(quote.price);
    }
    let mut ranked: Vec<(&Iata, Decimal)> = best.into_iter().collect();
    ranked.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(c, _)| c.clone())
        .collect()
}

/// Splits the budget across groups in proportion to their size, then inside each
/// group prefers historically cheap queries and explores never/least recently run ones.
fn by_history(
    candidates: Vec<Candidate>,
    budget: usize,
    history: &HashMap<String, QueryStats>,
) -> Vec<Candidate> {
    let total = candidates.len().max(1);
    let mut groups: BTreeMap<LegGroup, Vec<Candidate>> = BTreeMap::new();
    for candidate in candidates {
        groups.entry(candidate.group).or_default().push(candidate);
    }
    let mut ranked = Vec::new();
    for list in groups.into_values() {
        let share = (budget * list.len()).div_ceil(total).max(1);
        let stats = |c: &Candidate| history.get(&c.query.key()).cloned().unwrap_or_default();
        let (mut known, mut fresh): (Vec<_>, Vec<_>) = list
            .into_iter()
            .partition(|c| stats(c).best_price.is_some());
        known.sort_by_key(|c| stats(c).best_price);
        fresh.sort_by_key(|c| stats(c).last_run_at);
        let exploit = (share * EXPLOIT_PERCENT).div_ceil(100).min(known.len());
        let explore = (share - exploit).min(fresh.len());
        let exploit = (share - explore).min(known.len());
        let mut chosen: Vec<Candidate> = known.into_iter().take(exploit).collect();
        chosen.extend(fresh.into_iter().take(explore));
        ranked.push(chosen);
    }
    round_robin(&mut ranked, budget)
}

fn round_robin(lists: &mut [Vec<Candidate>], budget: usize) -> Vec<Candidate> {
    for list in lists.iter_mut() {
        list.reverse();
    }
    let mut out = Vec::new();
    while out.len() < budget {
        let before = out.len();
        for list in lists.iter_mut() {
            if out.len() < budget
                && let Some(candidate) = list.pop()
            {
                out.push(candidate);
            }
        }
        if out.len() == before {
            break;
        }
    }
    out
}

/// Merges identical queries across searches so each runs once.
pub fn dedup(plans: &[(SearchId, Vec<ProviderQuery>)]) -> Vec<(ProviderQuery, Vec<SearchId>)> {
    let mut order: Vec<ProviderQuery> = Vec::new();
    let mut owners: HashMap<ProviderQuery, Vec<SearchId>> = HashMap::new();
    for (search, queries) in plans {
        for query in queries {
            let entry = owners.entry(query.clone()).or_insert_with(|| {
                order.push(query.clone());
                Vec::new()
            });
            if !entry.contains(search) {
                entry.push(*search);
            }
        }
    }
    order
        .into_iter()
        .map(|q| {
            let searches = owners.remove(&q).unwrap_or_default();
            (q, searches)
        })
        .collect()
}
