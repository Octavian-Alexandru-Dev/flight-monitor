use chrono::{Days, NaiveDate};
use domain::{Iata, SearchDefinition, TripMode};

use crate::Resolved;

/// Which part of a trip a set of queries serves; scan→verify keeps the best of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LegGroup {
    Outbound,
    Return,
    RoundTrip,
    ToHub,
    FromHub,
    ReturnToHub,
    ReturnFromHub,
    CompanionOut(usize),
    CompanionBack(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Need {
    OneWay {
        group: LegGroup,
        from: Vec<Iata>,
        to: Vec<Iata>,
        dates: Vec<NaiveDate>,
        passengers: u8,
    },
    RoundTrip {
        from: Vec<Iata>,
        to: Vec<Iata>,
        pairs: Vec<(NaiveDate, NaiveDate)>,
        passengers: u8,
    },
}

/// Expands a search into the legs it needs. `destinations` overrides the
/// resolved ones (used when an open destination was narrowed by a scan).
pub fn needs(def: &SearchDefinition, resolved: &Resolved, destinations: &[Iata]) -> Vec<Need> {
    let out_dates = def.outbound.dates();
    let back_dates = def.return_dates();
    let decoupled = def.mode == TripMode::RoundTripDecoupled;
    let one_way =
        |group, from: &[Iata], to: &[Iata], dates: &[NaiveDate], passengers| Need::OneWay {
            group,
            from: from.to_vec(),
            to: to.to_vec(),
            dates: dates.to_vec(),
            passengers,
        };
    let passengers = def.passengers;
    let mut needs = Vec::new();

    if def.mode == TripMode::RoundTripCoupled {
        let pairs = out_dates
            .iter()
            .flat_map(|out| {
                def.stay.into_iter().flat_map(move |stay| {
                    (stay.min_nights..=stay.max_nights)
                        .filter_map(move |n| out.checked_add_days(Days::new(u64::from(n))))
                        .map(move |back| (*out, back))
                })
            })
            .collect();
        needs.push(Need::RoundTrip {
            from: resolved.origins.clone(),
            to: destinations.to_vec(),
            pairs,
            passengers,
        });
        return needs;
    }

    needs.push(one_way(
        LegGroup::Outbound,
        &resolved.origins,
        destinations,
        &out_dates,
        passengers,
    ));
    if decoupled {
        needs.push(one_way(
            LegGroup::Return,
            &resolved.return_origins,
            &resolved.return_destinations,
            &back_dates,
            passengers,
        ));
    }

    if let Some(via) = &def.via {
        let spill = Days::new(u64::from(via.max_connection_minutes.div_ceil(24 * 60)));
        let extend = |dates: &[NaiveDate]| -> Vec<NaiveDate> {
            let mut all: Vec<NaiveDate> = dates
                .iter()
                .flat_map(|d| {
                    d.iter_days()
                        .take_while(move |x| d.checked_add_days(spill).is_some_and(|end| *x <= end))
                })
                .collect();
            all.sort();
            all.dedup();
            all
        };
        let hubs = &resolved.hubs;
        needs.push(one_way(
            LegGroup::ToHub,
            &resolved.origins,
            hubs,
            &out_dates,
            passengers,
        ));
        needs.push(one_way(
            LegGroup::FromHub,
            hubs,
            destinations,
            &extend(&out_dates),
            passengers,
        ));
        if decoupled {
            needs.push(one_way(
                LegGroup::ReturnToHub,
                &resolved.return_origins,
                hubs,
                &back_dates,
                passengers,
            ));
            needs.push(one_way(
                LegGroup::ReturnFromHub,
                hubs,
                &resolved.return_destinations,
                &extend(&back_dates),
                passengers,
            ));
        }
    }

    for (i, (_, origins)) in resolved.companions.iter().enumerate() {
        needs.push(one_way(
            LegGroup::CompanionOut(i),
            origins,
            destinations,
            &out_dates,
            1,
        ));
        if decoupled {
            needs.push(one_way(
                LegGroup::CompanionBack(i),
                &resolved.return_origins,
                origins,
                &back_dates,
                1,
            ));
        }
    }
    needs
}
