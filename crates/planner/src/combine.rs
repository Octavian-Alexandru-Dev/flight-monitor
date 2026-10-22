use std::collections::{BTreeMap, HashSet};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use domain::{
    CalendarCell, Decimal, Direction, GroupGoal, Iata, Itinerary, SearchDefinition, Ticket,
    TripMode, TripOption, ViaHub, Warning,
};
use geo::GeoIndex;

use crate::{Resolved, passes_constraints};

const MAX_OPTIONS: usize = 50;
/// Cheapest journeys per direction considered when pairing or grouping.
const POOL: usize = 40;
/// Return options kept per outbound journey, so results are not all one flight.
const PER_OUTBOUND: usize = 3;
const STOPOVER_MINUTES: i64 = 20 * 60;
const OVERNIGHT_MINUTES: i64 = 6 * 60;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub options: Vec<TripOption>,
    pub calendar: Vec<CalendarCell>,
}

/// One direction of travel for one traveler: one ticket, or two joined at a hub.
#[derive(Clone, Debug)]
struct Journey {
    tickets: Vec<Itinerary>,
    warnings: Vec<Warning>,
}

impl Journey {
    fn single(it: &Itinerary) -> Self {
        let mut warnings = Vec::new();
        for layover in it.layovers() {
            if layover.changes_airport() {
                warnings.push(Warning::AirportChange {
                    from: layover.arrival_airport.clone(),
                    to: layover.departure_airport.clone(),
                });
            }
            if layover.overnight {
                warnings.push(Warning::OvernightLayover {
                    airport: layover.arrival_airport.clone(),
                });
            }
        }
        Self {
            tickets: vec![it.clone()],
            warnings,
        }
    }

    fn first(&self) -> &Itinerary {
        &self.tickets[0]
    }

    fn last(&self) -> &Itinerary {
        &self.tickets[self.tickets.len() - 1]
    }

    fn price(&self) -> Decimal {
        self.tickets.iter().map(|t| t.price).sum()
    }

    fn date(&self) -> NaiveDate {
        self.first().departure_date()
    }

    fn arrival(&self) -> NaiveDateTime {
        self.last().arrival()
    }

    /// Nights spent at layovers or hubs, each one a potential hotel stay.
    fn nights_away(&self) -> i64 {
        let within: i64 = self
            .tickets
            .iter()
            .flat_map(|t| {
                t.segments()
                    .windows(2)
                    .map(|w| nights_between(w[0].arrival, w[1].departure))
                    .collect::<Vec<_>>()
            })
            .sum();
        let between: i64 = self
            .tickets
            .windows(2)
            .map(|w| nights_between(w[0].arrival(), w[1].departure()))
            .sum();
        within + between
    }
}

fn nights_between(from: NaiveDateTime, to: NaiveDateTime) -> i64 {
    if (to - from).num_minutes() < OVERNIGHT_MINUTES {
        return 0;
    }
    let three_am = NaiveTime::from_hms_opt(3, 0, 0).unwrap_or_default();
    from.date()
        .iter_days()
        .map(|d| d.and_time(three_am))
        .take_while(|t| *t <= to)
        .filter(|t| *t >= from)
        .count() as i64
}

struct Sets<'a> {
    def: &'a SearchDefinition,
    resolved: &'a Resolved,
    geo: &'a GeoIndex,
    itineraries: Vec<&'a Itinerary>,
    out_dates: HashSet<NaiveDate>,
    back_dates: HashSet<NaiveDate>,
}

impl Sets<'_> {
    fn to_destination(&self, code: &Iata) -> bool {
        self.resolved
            .destinations
            .as_ref()
            .is_none_or(|d| d.contains(code))
    }

    fn one_way(
        &self,
        from: impl Fn(&Iata) -> bool,
        to: impl Fn(&Iata) -> bool,
        dates: &HashSet<NaiveDate>,
    ) -> Vec<&Itinerary> {
        self.itineraries
            .iter()
            .copied()
            .filter(|i| {
                i.return_date.is_none()
                    && from(i.origin())
                    && to(i.destination())
                    && dates.contains(&i.departure_date())
            })
            .collect()
    }

    fn outbound(&self, origins: &[Iata]) -> Vec<Journey> {
        let direct = self.one_way(
            |o| origins.contains(o),
            |d| self.to_destination(d),
            &self.out_dates,
        );
        let mut journeys: Vec<Journey> = direct.into_iter().map(Journey::single).collect();
        if let Some(via) = &self.def.via {
            let hubs = &self.resolved.hubs;
            let first = self.one_way(
                |o| origins.contains(o),
                |d| hubs.contains(d),
                &self.out_dates,
            );
            let all_dates = self
                .itineraries
                .iter()
                .map(|i| i.departure_date())
                .collect();
            let second = self.one_way(|o| hubs.contains(o), |d| self.to_destination(d), &all_dates);
            journeys.extend(join_at_hub(&first, &second, via, self.geo));
        }
        journeys
    }

    fn returns(&self, destinations: &[Iata]) -> Vec<Journey> {
        let origins = &self.resolved.return_origins;
        let direct = self.one_way(
            |o| origins.contains(o),
            |d| destinations.contains(d),
            &self.back_dates,
        );
        let mut journeys: Vec<Journey> = direct.into_iter().map(Journey::single).collect();
        if let Some(via) = &self.def.via {
            let hubs = &self.resolved.hubs;
            let first = self.one_way(
                |o| origins.contains(o),
                |d| hubs.contains(d),
                &self.back_dates,
            );
            let all_dates = self
                .itineraries
                .iter()
                .map(|i| i.departure_date())
                .collect();
            let second = self.one_way(
                |o| hubs.contains(o),
                |d| destinations.contains(d),
                &all_dates,
            );
            journeys.extend(join_at_hub(&first, &second, via, self.geo));
        }
        journeys
    }

    fn round_trips(&self) -> Vec<Journey> {
        let Some(stay) = self.def.stay else {
            return Vec::new();
        };
        self.itineraries
            .iter()
            .filter(|i| {
                i.return_date
                    .is_some_and(|back| stay.contains((back - i.departure_date()).num_days()))
                    && self.resolved.origins.contains(i.origin())
                    && self.to_destination(i.destination())
                    && self.out_dates.contains(&i.departure_date())
            })
            .map(|i| Journey::single(i))
            .collect()
    }
}

fn cheapest(mut journeys: Vec<Journey>, limit: usize) -> Vec<Journey> {
    journeys.sort_by_key(Journey::price);
    journeys.truncate(limit);
    journeys
}

fn join_at_hub(
    first: &[&Itinerary],
    second: &[&Itinerary],
    via: &ViaHub,
    geo: &GeoIndex,
) -> Vec<Journey> {
    let firsts = cheapest(first.iter().map(|i| Journey::single(i)).collect(), POOL);
    let seconds = cheapest(second.iter().map(|i| Journey::single(i)).collect(), POOL);
    let mut out = Vec::new();
    for a in &firsts {
        for b in &seconds {
            let (arrive, depart) = (a.first(), b.first());
            let hub = arrive.destination();
            if hub != depart.origin() && !geo.same_city(hub, depart.origin()) {
                continue;
            }
            let minutes = (depart.departure() - arrive.arrival()).num_minutes();
            if minutes < i64::from(via.min_connection_minutes)
                || minutes > i64::from(via.max_connection_minutes)
            {
                continue;
            }
            let mut warnings = [a.warnings.clone(), b.warnings.clone()].concat();
            if minutes >= STOPOVER_MINUTES {
                warnings.push(Warning::Stopover {
                    hub: hub.clone(),
                    hours: minutes / 60,
                });
            } else {
                warnings.push(Warning::SelfTransfer {
                    hub: hub.clone(),
                    minutes,
                });
                if nights_between(arrive.arrival(), depart.departure()) > 0 {
                    warnings.push(Warning::OvernightLayover {
                        airport: hub.clone(),
                    });
                }
            }
            if hub != depart.origin() {
                warnings.push(Warning::AirportChange {
                    from: hub.clone(),
                    to: depart.origin().clone(),
                });
            }
            out.push(Journey {
                tickets: vec![arrive.clone(), depart.clone()],
                warnings,
            });
        }
    }
    out
}

struct Party {
    traveler: Option<String>,
    outbound: Journey,
    back: Option<Journey>,
}

impl Party {
    fn price(&self) -> Decimal {
        self.outbound.price() + self.back.as_ref().map_or(Decimal::ZERO, Journey::price)
    }
}

pub fn build_options(
    def: &SearchDefinition,
    resolved: &Resolved,
    itineraries: &[Itinerary],
    geo: &GeoIndex,
) -> Outcome {
    let sets = Sets {
        def,
        resolved,
        geo,
        itineraries: itineraries
            .iter()
            .filter(|i| passes_constraints(i, &def.constraints))
            .collect(),
        out_dates: def.outbound.dates().into_iter().collect(),
        back_dates: def.return_dates().into_iter().collect(),
    };
    let mut calendar: BTreeMap<(NaiveDate, Option<NaiveDate>), Decimal> = BTreeMap::new();
    let mut note = |out: NaiveDate, back: Option<NaiveDate>, price: Decimal| {
        let cell = calendar.entry((out, back)).or_insert(price);
        *cell = (*cell).min(price);
    };

    let parties: Vec<Vec<Party>> = match def.mode {
        TripMode::OneWay => sets
            .outbound(&resolved.origins)
            .into_iter()
            .map(|j| {
                vec![Party {
                    traveler: None,
                    outbound: j,
                    back: None,
                }]
            })
            .collect(),
        TripMode::RoundTripCoupled => sets
            .round_trips()
            .into_iter()
            .map(|j| {
                vec![Party {
                    traveler: None,
                    outbound: j,
                    back: None,
                }]
            })
            .collect(),
        TripMode::RoundTripDecoupled => {
            let outs = sets.outbound(&resolved.origins);
            let backs = sets.returns(&resolved.return_destinations);
            if resolved.companions.is_empty() {
                calendar_for_pairs(def, &outs, &backs, &mut note);
            }
            pair(def, outs, backs)
                .into_iter()
                .map(|(o, b)| {
                    vec![Party {
                        traveler: None,
                        outbound: o,
                        back: Some(b),
                    }]
                })
                .collect()
        }
    };
    let parties = if resolved.companions.is_empty() {
        parties
    } else {
        with_companions(&sets, parties)
    };

    let mut options: Vec<TripOption> = parties
        .into_iter()
        .map(|party| to_option(def, party))
        .filter(|o| {
            def.constraints
                .max_price
                .is_none_or(|max| o.total_price <= max)
        })
        .collect();
    if def.mode != TripMode::RoundTripDecoupled || !resolved.companions.is_empty() {
        for option in &options {
            if let Some(out) = option.outbound_date() {
                note(out, option.return_date(), option.total_price);
            }
        }
    }
    let by_door = !def.costs.is_empty();
    options.sort_by(|a, b| {
        let key = |o: &TripOption| {
            if by_door {
                o.door_to_door()
            } else {
                o.total_price
            }
        };
        key(a).cmp(&key(b)).then_with(|| a.key().cmp(&b.key()))
    });
    options.dedup_by(|a, b| a.key() == b.key());
    options.truncate(MAX_OPTIONS);

    Outcome {
        options,
        calendar: calendar
            .into_iter()
            .map(|((outbound, inbound), price)| CalendarCell {
                outbound,
                inbound,
                price,
            })
            .collect(),
    }
}

fn stay_ok(def: &SearchDefinition, out: &Journey, back: &Journey) -> bool {
    let nights = (back.date() - out.date()).num_days();
    def.stay.is_some_and(|s| s.contains(nights)) && back.first().departure() > out.arrival()
}

fn pair(
    def: &SearchDefinition,
    outs: Vec<Journey>,
    backs: Vec<Journey>,
) -> Vec<(Journey, Journey)> {
    let outs = cheapest(outs, POOL);
    let backs = cheapest(backs, POOL);
    let mut pairs = Vec::new();
    for out in &outs {
        let matching = backs
            .iter()
            .filter(|b| stay_ok(def, out, b))
            .take(PER_OUTBOUND);
        pairs.extend(matching.map(|b| (out.clone(), b.clone())));
    }
    pairs
}

/// Best price for every (outbound, return) date pair, beyond the options kept.
fn calendar_for_pairs(
    def: &SearchDefinition,
    outs: &[Journey],
    backs: &[Journey],
    note: &mut impl FnMut(NaiveDate, Option<NaiveDate>, Decimal),
) {
    let min_by_date = |journeys: &[Journey]| {
        let mut best: BTreeMap<NaiveDate, Decimal> = BTreeMap::new();
        for j in journeys {
            let cell = best.entry(j.date()).or_insert(j.price());
            *cell = (*cell).min(j.price());
        }
        best
    };
    let (outs, backs) = (min_by_date(outs), min_by_date(backs));
    for (out, out_price) in &outs {
        for (back, back_price) in &backs {
            if def
                .stay
                .is_some_and(|s| s.contains((*back - *out).num_days()))
            {
                note(*out, Some(*back), *out_price + *back_price);
            }
        }
    }
}

fn with_companions(sets: &Sets<'_>, parties: Vec<Vec<Party>>) -> Vec<Vec<Party>> {
    let decoupled = sets.def.mode == TripMode::RoundTripDecoupled;
    let companions: Vec<(String, Vec<Journey>, Vec<Journey>)> = sets
        .resolved
        .companions
        .iter()
        .map(|(name, origins)| {
            let outs = cheapest(sets.outbound(origins), POOL);
            let backs = if decoupled {
                cheapest(sets.returns(origins), POOL)
            } else {
                Vec::new()
            };
            (name.clone(), outs, backs)
        })
        .collect();
    let mut main: Vec<Vec<Party>> = parties;
    main.sort_by_key(|p| p.iter().map(Party::price).sum::<Decimal>());
    main.truncate(POOL);

    let max_spread = match sets.def.group_goal {
        GroupGoal::MinTotalCost => 24 * 60,
        GroupGoal::AlignArrivals { max_spread_minutes } => i64::from(max_spread_minutes),
    };
    let align = matches!(sets.def.group_goal, GroupGoal::AlignArrivals { .. });
    let mut out = Vec::new();
    'parties: for mut party in main {
        let Some(lead) = party.first() else { continue };
        let lead_out = lead.outbound.clone();
        let lead_back = lead.back.clone();
        for (name, outs, backs) in &companions {
            let meets = |j: &Journey| {
                sets.geo
                    .same_city(j.last().destination(), lead_out.last().destination())
                    && (j.arrival() - lead_out.arrival()).num_minutes().abs() <= max_spread
            };
            let rank = |j: &Journey| {
                let spread = (j.arrival() - lead_out.arrival()).num_minutes().abs();
                if align {
                    (spread, j.price())
                } else {
                    (0, j.price())
                }
            };
            let Some(outbound) = outs.iter().filter(|j| meets(j)).min_by_key(|j| rank(j)) else {
                continue 'parties;
            };
            let back = match &lead_back {
                Some(lead_back) => {
                    let leaves = |j: &&Journey| {
                        j.date() == lead_back.date()
                            && sets
                                .geo
                                .same_city(j.first().origin(), lead_back.first().origin())
                    };
                    match backs.iter().filter(leaves).min_by_key(|j| j.price()) {
                        Some(j) => Some(j.clone()),
                        None => continue 'parties,
                    }
                }
                None => None,
            };
            party.push(Party {
                traveler: Some(name.clone()),
                outbound: outbound.clone(),
                back,
            });
        }
        out.push(party);
    }
    out
}

fn to_option(def: &SearchDefinition, party: Vec<Party>) -> TripOption {
    let mut tickets = Vec::new();
    let mut warnings = Vec::new();
    let mut nights = 0;
    let arrivals: Vec<NaiveDateTime> = party.iter().map(|p| p.outbound.arrival()).collect();
    for member in &party {
        for (direction, journey) in [
            (Direction::Outbound, Some(&member.outbound)),
            (Direction::Return, member.back.as_ref()),
        ] {
            let Some(journey) = journey else { continue };
            nights += journey.nights_away();
            for warning in &journey.warnings {
                if !warnings.contains(warning) {
                    warnings.push(warning.clone());
                }
            }
            tickets.extend(journey.tickets.iter().map(|it| Ticket {
                traveler: member.traveler.clone(),
                direction,
                itinerary: it.clone(),
            }));
        }
    }
    if let (Some(first), Some(last)) = (arrivals.iter().min(), arrivals.iter().max()) {
        let spread = (*last - *first).num_minutes();
        if spread > 60 {
            warnings.push(Warning::ArrivalSpread { minutes: spread });
        }
    }
    let mut option = TripOption::new(tickets, def.currency.clone());
    option.warnings = warnings;
    option.extra_cost = door_to_door_extra(def, &party, nights);
    option
}

/// Ground transport to/from the home airports plus hotel nights at layovers.
fn door_to_door_extra(def: &SearchDefinition, party: &[Party], nights: i64) -> Decimal {
    let Some(lead) = party.first() else {
        return Decimal::ZERO;
    };
    let access = |code: &Iata| {
        def.costs
            .airport_access
            .get(code)
            .copied()
            .unwrap_or_default()
    };
    let mut extra = access(lead.outbound.first().origin());
    if let Some(back) = &lead.back {
        extra += access(back.last().destination());
    } else if lead.outbound.first().return_date.is_some() {
        extra += access(lead.outbound.first().origin());
    }
    if let Some(hotel) = def.costs.overnight_layover {
        extra += hotel * Decimal::from(nights);
    }
    extra
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_nights_spanning_three_am() {
        let at = |s: &str| s.parse::<NaiveDateTime>().unwrap();
        assert_eq!(
            nights_between(at("2026-12-10T22:00:00"), at("2026-12-11T08:00:00")),
            1
        );
        assert_eq!(
            nights_between(at("2026-12-10T10:00:00"), at("2026-12-10T20:00:00")),
            0
        );
        assert_eq!(
            nights_between(at("2026-12-10T10:00:00"), at("2026-12-12T10:00:00")),
            2
        );
        assert_eq!(
            nights_between(at("2026-12-11T01:00:00"), at("2026-12-11T05:00:00")),
            0
        );
    }
}
