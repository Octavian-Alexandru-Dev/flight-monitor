use domain::{Itinerary, ItineraryConstraints};

/// Applies the constraints a source cannot enforce on its own.
pub fn passes_constraints(it: &Itinerary, c: &ItineraryConstraints) -> bool {
    if it.stops() > usize::from(c.max_stops) {
        return false;
    }
    let layovers = it.layovers();
    if let Some(min) = c.min_layover_minutes
        && layovers.iter().any(|l| l.minutes < i64::from(min))
    {
        return false;
    }
    if let Some(max) = c.max_layover_minutes
        && layovers.iter().any(|l| l.minutes > i64::from(max))
    {
        return false;
    }
    if let Some(max) = c.max_total_minutes
        && it.total_minutes() > i64::from(max)
    {
        return false;
    }
    if it.segments().iter().any(|s| {
        c.excluded_airlines
            .iter()
            .any(|x| x.eq_ignore_ascii_case(&s.airline))
    }) {
        return false;
    }
    let departs = it.departure().time();
    !c.excluded_departure_hours
        .iter()
        .any(|r| r.contains(departs))
}
