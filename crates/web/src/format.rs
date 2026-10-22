use domain::Itinerary;
pub use domain::format_money as money;

pub fn duration(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

pub fn stops(count: usize) -> String {
    match count {
        0 => "Diretto".into(),
        1 => "1 scalo".into(),
        n => format!("{n} scali"),
    }
}

/// One-line summary used by the CLI.
pub fn itinerary_line(it: &Itinerary) -> String {
    let flights: Vec<String> = it.segments().iter().map(|s| s.code()).collect();
    let layovers: Vec<String> = it
        .layovers()
        .iter()
        .map(|l| format!("{} {}", l.arrival_airport, duration(l.minutes)))
        .collect();
    let via = if layovers.is_empty() {
        String::new()
    } else {
        format!(" via {}", layovers.join(", "))
    };
    format!(
        "{} {}→{} {} → {} ({}, {}{via}) {}",
        money(it.price, &it.currency),
        it.origin(),
        it.destination(),
        it.departure().format("%d/%m %H:%M"),
        it.arrival().format("%d/%m %H:%M"),
        duration(it.total_minutes()),
        stops(it.stops()).to_lowercase(),
        flights.join(" "),
    )
}
