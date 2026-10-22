use chrono::NaiveDate;
use domain::{Decimal, TripOption};

use crate::Trigger;

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn money(amount: Decimal, currency: &str) -> String {
    let amount = amount.round_dp(2).normalize();
    match currency {
        "EUR" => format!("€{amount}"),
        "USD" => format!("${amount}"),
        "GBP" => format!("£{amount}"),
        _ => format!("{amount} {currency}"),
    }
}

fn day(date: NaiveDate) -> String {
    date.format("%d/%m").to_string()
}

fn reason(trigger: &Trigger, currency: &str) -> String {
    match trigger {
        Trigger::BelowPrice { threshold } => {
            format!("Sotto la soglia di {}", money(*threshold, currency))
        }
        Trigger::Drop { percent, previous } => format!(
            "Calo del {}% (prima {})",
            percent.normalize(),
            money(*previous, currency)
        ),
        Trigger::NewMinimum { previous } => {
            format!(
                "Nuovo minimo storico (prima {})",
                money(*previous, currency)
            )
        }
        Trigger::Anomaly { mean } => {
            format!("Molto sotto la media storica di {}", money(*mean, currency))
        }
    }
}

/// Telegram message for a price alert. `link` points back to the web UI, if reachable.
pub fn alert_message(
    search: &str,
    option: &TripOption,
    triggers: &[Trigger],
    link: Option<&str>,
) -> String {
    let currency = &option.currency;
    let mut lines = Vec::new();
    if triggers.iter().any(Trigger::is_priority) {
        lines.push("🔥 <b>OCCASIONE</b>".to_string());
    }
    lines.push(format!("✈️ <b>{}</b>", escape(search)));
    let dates = match (option.outbound_date(), option.return_date()) {
        (Some(out), Some(back)) => format!("{} → {}", day(out), day(back)),
        (Some(out), None) => day(out),
        _ => String::new(),
    };
    lines.push(format!(
        "<b>{}</b> · {dates}",
        money(option.total_price, currency)
    ));
    if !option.extra_cost.is_zero() {
        lines.push(format!(
            "Porta a porta: {}",
            money(option.door_to_door(), currency)
        ));
    }
    lines.extend(
        triggers
            .iter()
            .map(|t| format!("• {}", reason(t, currency))),
    );
    lines.push(String::new());
    for ticket in &option.tickets {
        let it = &ticket.itinerary;
        let who = ticket
            .traveler
            .as_deref()
            .map(|n| format!("{}: ", escape(n)))
            .unwrap_or_default();
        let flights: Vec<String> = it.segments().iter().map(|s| s.code()).collect();
        let line = format!(
            "{who}{} → {} {} ({}) {}",
            it.origin(),
            it.destination(),
            it.departure().format("%d/%m %H:%M"),
            flights.join("+"),
            money(it.price, &it.currency),
        );
        match &it.booking_url {
            Some(url) => lines.push(format!("<a href=\"{}\">{line}</a>", escape(url.as_str()))),
            None => lines.push(line),
        }
    }
    for warning in option.warnings.iter().filter(|w| w.is_risk()) {
        lines.push(format!("⚠️ {}", escape(&warning.message())));
    }
    if let Some(link) = link {
        lines.push(format!("\n<a href=\"{}\">Dettagli</a>", escape(link)));
    }
    lines.join("\n")
}

pub fn health_message(provider: &str, degraded: bool, detail: &str) -> String {
    if degraded {
        format!(
            "🛑 <b>Fonte degradata: {}</b>\n{}\nI risultati mancanti non significano «nessun volo».",
            escape(provider),
            escape(detail)
        )
    } else {
        format!("✅ <b>Fonte ripristinata: {}</b>", escape(provider))
    }
}

pub struct SummaryLine {
    pub search: String,
    pub best: Option<Decimal>,
    pub day_before: Option<Decimal>,
    pub currency: String,
    pub verdict: String,
}

pub fn daily_summary(lines: &[SummaryLine], degraded: &[String]) -> String {
    let mut out = vec!["☀️ <b>Riepilogo giornaliero</b>".to_string()];
    if lines.is_empty() {
        out.push("Nessuna ricerca attiva.".into());
    }
    for line in lines {
        let price = line
            .best
            .map_or_else(|| "n/d".to_string(), |p| money(p, &line.currency));
        let change = match (line.best, line.day_before) {
            (Some(now), Some(before)) if now < before => {
                format!(" ▼ {}", money(before - now, &line.currency))
            }
            (Some(now), Some(before)) if now > before => {
                format!(" ▲ {}", money(now - before, &line.currency))
            }
            _ => String::new(),
        };
        out.push(format!(
            "• {}: <b>{price}</b>{change} · {}",
            escape(&line.search),
            escape(&line.verdict)
        ));
    }
    if !degraded.is_empty() {
        out.push(format!(
            "🛑 Fonti degradate: {}",
            escape(&degraded.join(", "))
        ));
    }
    out.join("\n")
}
