use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, Utc};
use domain::{Decimal, Direction, Itinerary, LocationSet, SearchDefinition, TripMode, TripOption};
use geo::GeoIndex;
use planner::{Advice, Verdict};

const MONTHS: [&str; 12] = [
    "gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic",
];
const WEEKDAYS: [&str; 7] = ["lun", "mar", "mer", "gio", "ven", "sab", "dom"];

pub fn money(amount: Decimal, currency: &str) -> String {
    let rounded = amount.round_dp(0).normalize();
    let digits = rounded.abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(c);
    }
    let sign = if rounded.is_sign_negative() { "-" } else { "" };
    match currency {
        "EUR" => format!("{sign}€{grouped}"),
        "USD" => format!("{sign}${grouped}"),
        "GBP" => format!("{sign}£{grouped}"),
        _ => format!("{sign}{grouped} {currency}"),
    }
}

pub fn day_short(date: NaiveDate) -> String {
    format!(
        "{} {} {}",
        WEEKDAYS[date.weekday().num_days_from_monday() as usize],
        date.day(),
        MONTHS[date.month0() as usize]
    )
}

pub fn day_long(date: NaiveDate) -> String {
    format!(
        "{} {} {}",
        date.day(),
        MONTHS[date.month0() as usize],
        date.year()
    )
}

fn time_of(at: NaiveDateTime) -> String {
    at.format("%H:%M").to_string()
}

pub fn duration(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m:02}"),
    }
}

/// "tra 3 h", "12 min fa", "ieri"...
pub fn relative(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (at - now).num_minutes();
    let (future, m) = (minutes > 0, minutes.abs());
    let amount = match m {
        0 => return "adesso".into(),
        1..=59 => format!("{m} min"),
        60..=1439 => format!("{} h", (m + 30) / 60),
        1440..=2879 => {
            return if future {
                "domani".into()
            } else {
                "ieri".into()
            };
        }
        _ => format!("{} giorni", m / 1440),
    };
    if future {
        format!("tra {amount}")
    } else {
        format!("{amount} fa")
    }
}

pub fn local_time(at: DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    format!(
        "{} {}",
        day_short(local.date_naive()),
        local.format("%H:%M")
    )
}

pub fn place(set: &LocationSet, geo: &GeoIndex) -> String {
    match set {
        LocationSet::Airport(code) => code.to_string(),
        LocationSet::Airports(codes) => codes
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        LocationSet::City(name) => name.clone(),
        LocationSet::Country(code) => geo.country_name(code).unwrap_or(code).to_string(),
        LocationSet::Radius { lat, lon, km } => match geo.nearest(*lat, *lon) {
            Some(airport) => format!("{km:.0} km da {}", airport.iata),
            None => format!("{km:.0} km da {lat:.2}, {lon:.2}"),
        },
        LocationSet::Anywhere => "Ovunque".into(),
    }
}

pub fn route(def: &SearchDefinition, geo: &GeoIndex) -> String {
    let arrow = if def.mode.is_round_trip() {
        "⇄"
    } else {
        "→"
    };
    let mut text = format!(
        "{} {arrow} {}",
        place(&def.origin, geo),
        place(&def.destination, geo)
    );
    if def.is_open_jaw() {
        text.push_str(&format!(
            ", ritorno {} → {}",
            place(def.return_origin(), geo),
            place(def.return_destination(), geo)
        ));
    }
    text
}

pub fn dates(def: &SearchDefinition) -> String {
    let (first, last) = (def.outbound.first(), def.outbound.last());
    let out = if first == last {
        day_long(first)
    } else {
        format!("{} - {}", day_short(first), day_short(last))
    };
    match def.stay {
        Some(stay) if def.mode.is_round_trip() => {
            format!("{out} · {}-{} notti", stay.min_nights, stay.max_nights)
        }
        _ => out,
    }
}

pub fn mode_label(mode: TripMode) -> &'static str {
    mode.label()
}

#[derive(Clone, Debug)]
pub struct Delta {
    pub text: String,
    pub down: bool,
}

pub fn delta(current: Option<Decimal>, previous: Option<Decimal>, currency: &str) -> Option<Delta> {
    let (now, before) = (current?, previous?);
    if now == before || before.is_zero() {
        return None;
    }
    let pct = ((now - before) * Decimal::ONE_HUNDRED / before)
        .round_dp(1)
        .abs()
        .normalize();
    let diff = money((now - before).abs(), currency);
    Some(if now < before {
        Delta {
            text: format!("-{diff} ({pct}%)"),
            down: true,
        }
    } else {
        Delta {
            text: format!("+{diff} ({pct}%)"),
            down: false,
        }
    })
}

#[derive(Clone, Debug)]
pub struct AdviceView {
    pub label: &'static str,
    pub reason: String,
    pub tone: &'static str,
    pub icon: &'static str,
}

impl From<Advice> for AdviceView {
    fn from(advice: Advice) -> Self {
        let (tone, icon) = match advice.verdict {
            Verdict::BuyNow => ("buy", "ph-shopping-cart-simple"),
            Verdict::Wait => ("wait", "ph-hourglass-medium"),
            Verdict::Watch => ("watch", "ph-eye"),
        };
        Self {
            label: advice.verdict.label(),
            reason: advice.reason,
            tone,
            icon,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TicketView {
    pub direction: &'static str,
    pub route: String,
    pub depart: String,
    pub arrive: String,
    pub plus_days: i64,
    pub duration: String,
    pub stops: String,
    pub layovers: String,
    pub flights: String,
    pub airlines: String,
    pub price: String,
    pub url: Option<String>,
    pub round_trip_fare: bool,
}

#[derive(Clone, Debug)]
pub struct TravelerView {
    pub name: String,
    pub tickets: Vec<TicketView>,
}

#[derive(Clone, Debug)]
pub struct WarningView {
    pub text: String,
    pub risk: bool,
}

#[derive(Clone, Debug)]
pub struct OptionView {
    pub price: String,
    pub door_to_door: Option<String>,
    pub dates: String,
    pub travelers: Vec<TravelerView>,
    pub warnings: Vec<WarningView>,
    pub separate_tickets: bool,
    pub multiple_tickets: bool,
}

fn stops_label(count: usize) -> String {
    match count {
        0 => "Diretto".into(),
        1 => "1 scalo".into(),
        n => format!("{n} scali"),
    }
}

fn ticket(it: &Itinerary, direction: Direction) -> TicketView {
    let mut flights: Vec<String> = it.segments().iter().map(|s| s.code()).collect();
    flights.dedup();
    let mut airlines: Vec<String> = it
        .segments()
        .iter()
        .map(|s| {
            if s.airline_name.is_empty() {
                s.airline.clone()
            } else {
                s.airline_name.clone()
            }
        })
        .collect();
    airlines.dedup();
    let layovers: Vec<String> = it
        .layovers()
        .iter()
        .map(|l| {
            let at = if l.changes_airport() {
                format!("{}→{}", l.arrival_airport, l.departure_airport)
            } else {
                l.arrival_airport.to_string()
            };
            format!("{at} {}", duration(l.minutes))
        })
        .collect();
    let mut route_codes: Vec<String> = vec![it.origin().to_string()];
    route_codes.push(it.destination().to_string());
    TicketView {
        direction: match direction {
            Direction::Outbound => "Andata",
            Direction::Return => "Ritorno",
        },
        route: route_codes.join(" → "),
        depart: format!(
            "{} {}",
            day_short(it.departure_date()),
            time_of(it.departure())
        ),
        arrive: time_of(it.arrival()),
        plus_days: (it.arrival().date() - it.departure_date()).num_days(),
        duration: duration(it.total_minutes()),
        stops: stops_label(it.stops()),
        layovers: layovers.join(", "),
        flights: flights.join(" · "),
        airlines: airlines.join(", "),
        price: money(it.price, &it.currency),
        url: it.booking_url.as_ref().map(|u| u.to_string()),
        round_trip_fare: it.return_date.is_some(),
    }
}

pub fn option_view(option: &TripOption) -> OptionView {
    let travelers = option
        .travelers()
        .into_iter()
        .map(|name| TravelerView {
            name: name.unwrap_or("Tu").to_string(),
            tickets: option
                .tickets_of(name)
                .into_iter()
                .map(|t| ticket(&t.itinerary, t.direction))
                .collect(),
        })
        .collect();
    let dates = match (option.outbound_date(), option.return_date()) {
        (Some(out), Some(back)) => format!(
            "{} → {} · {} notti",
            day_short(out),
            day_short(back),
            (back - out).num_days()
        ),
        (Some(out), None) => day_short(out),
        _ => String::new(),
    };
    OptionView {
        price: money(option.total_price, &option.currency),
        door_to_door: (!option.extra_cost.is_zero())
            .then(|| money(option.door_to_door(), &option.currency)),
        dates,
        travelers,
        warnings: option
            .warnings
            .iter()
            .map(|w| WarningView {
                text: w.message(),
                risk: w.is_risk(),
            })
            .collect(),
        multiple_tickets: option.tickets.len() > 1,
        separate_tickets: option.travelers().into_iter().any(|name| {
            let tickets = option.tickets_of(name);
            [Direction::Outbound, Direction::Return]
                .iter()
                .any(|d| tickets.iter().filter(|t| t.direction == *d).count() > 1)
        }),
    }
}

/// Short human summary of a search's filters and alerts, for the detail page.
pub fn settings_summary(def: &SearchDefinition, geo: &GeoIndex) -> Vec<String> {
    let c = &def.constraints;
    let mut out = vec![format!(
        "Max {} · {} passegger{}",
        stops_label(usize::from(c.max_stops))
            .to_lowercase()
            .replace("diretto", "0 scali"),
        def.passengers,
        if def.passengers == 1 { "o" } else { "i" }
    )];
    match (c.min_layover_minutes, c.max_layover_minutes) {
        (Some(min), Some(max)) => out.push(format!(
            "Scali tra {} e {}",
            duration(min.into()),
            duration(max.into())
        )),
        (Some(min), None) => out.push(format!("Scali di almeno {}", duration(min.into()))),
        (None, Some(max)) => out.push(format!("Scali fino a {}", duration(max.into()))),
        (None, None) => {}
    }
    if let Some(max) = c.max_total_minutes {
        out.push(format!("Viaggio max {}", duration(max.into())));
    }
    if !c.excluded_airlines.is_empty() {
        out.push(format!("Escluse: {}", c.excluded_airlines.join(", ")));
    }
    if c.checked_bags > 0 {
        out.push(format!("{} bagaglio in stiva incluso", c.checked_bags));
    }
    if let Some(max) = c.max_price {
        out.push(format!("Fino a {}", money(max, &def.currency)));
    }
    if let Some(via) = &def.via {
        out.push(format!(
            "{} via {}",
            if via.is_stopover() {
                "Stopover"
            } else {
                "Biglietti separati"
            },
            place(&via.hubs, geo)
        ));
    }
    if !def.companions.is_empty() {
        let names: Vec<&str> = def.companions.iter().map(|c| c.name.as_str()).collect();
        out.push(format!("Con {}", names.join(", ")));
    }
    if !def.costs.is_empty() {
        out.push("Costo porta a porta".into());
    }
    out
}

pub fn alerts_summary(def: &SearchDefinition) -> Vec<String> {
    def.alerts
        .iter()
        .map(|rule| match rule {
            domain::AlertRule::BelowPrice(p) => format!("Sotto {}", money(*p, &def.currency)),
            domain::AlertRule::DropPercent(p) => format!("Calo del {}%", p.normalize()),
            domain::AlertRule::NewHistoricMinimum => "Nuovo minimo storico".into(),
            domain::AlertRule::PriceAnomaly => "Prezzo anomalo".into(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_money_with_grouping() {
        assert_eq!(money(Decimal::from(1364), "EUR"), "€1.364");
        assert_eq!(money(Decimal::new(4999, 2), "EUR"), "€50");
        assert_eq!(money(Decimal::from(123456), "JPY"), "123.456 JPY");
    }

    #[test]
    fn formats_relative_times() {
        let now = Utc::now();
        assert_eq!(
            relative(now + chrono::Duration::minutes(90), now),
            "tra 2 h"
        );
        assert_eq!(
            relative(now - chrono::Duration::minutes(5), now),
            "5 min fa"
        );
    }

    #[test]
    fn formats_italian_days() {
        let d: NaiveDate = "2026-12-10".parse().unwrap();
        assert_eq!(day_short(d), "gio 10 dic");
    }
}
