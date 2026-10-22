use domain::{Decimal, Itinerary};

pub fn price(amount: Decimal, currency: &str) -> String {
    let rounded = amount.round_dp(2).normalize();
    let symbol = match currency {
        "EUR" => "€",
        "USD" => "$",
        "GBP" => "£",
        "JPY" => "¥",
        _ => "",
    };
    if symbol.is_empty() {
        format!("{rounded} {currency}")
    } else {
        format!("{symbol}{rounded}")
    }
}

pub fn duration(minutes: i64) -> String {
    format!("{}h{:02}", minutes / 60, minutes % 60)
}

pub fn stops(count: usize) -> String {
    match count {
        0 => "diretto".into(),
        1 => "1 scalo".into(),
        n => format!("{n} scali"),
    }
}

/// One-line summary used by the CLI and Telegram messages.
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
        price(it.price, &it.currency),
        it.origin(),
        it.destination(),
        it.departure().format("%d/%m %H:%M"),
        it.arrival().format("%d/%m %H:%M"),
        duration(it.total_minutes()),
        stops(it.stops()),
        flights.join(" "),
    )
}
