use chrono::NaiveDate;
use domain::{Iata, ProviderQuery, QueryLeg};

use crate::config::Config;
use crate::format;

pub struct SearchArgs {
    pub from: Vec<Iata>,
    pub to: Vec<Iata>,
    pub date: NaiveDate,
    pub return_date: Option<NaiveDate>,
    pub max_stops: u8,
    pub passengers: u8,
    pub currency: String,
    pub bags: u8,
}

/// Runs one live search and prints the normalised itineraries.
pub async fn run(config: &Config, args: SearchArgs) -> anyhow::Result<()> {
    let mut legs = vec![QueryLeg::on(args.from.clone(), args.to.clone(), args.date)];
    if let Some(back) = args.return_date {
        legs.push(QueryLeg::on(args.to, args.from, back));
    }
    let query = ProviderQuery {
        legs,
        max_stops: args.max_stops,
        passengers: args.passengers,
        currency: args.currency.to_ascii_uppercase(),
        checked_bags: args.bags,
    };
    let provider = config.google_flights();
    println!("{} · {}", provider.id(), query.describe());
    let mut itineraries = provider.search(&query).await?;
    itineraries.sort_by_key(|i| i.price);
    if itineraries.is_empty() {
        println!("nessun volo trovato");
    }
    for it in &itineraries {
        println!("{}", format::itinerary_line(it));
    }
    if let Some(url) = itineraries.first().and_then(|i| i.booking_url.as_ref()) {
        println!("\nlink: {url}");
    }
    Ok(())
}
