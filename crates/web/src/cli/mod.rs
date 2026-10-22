mod import_geo;
mod search;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use domain::Iata;

use crate::config::Config;

#[derive(Parser)]
#[command(
    name = "flight-monitor",
    about = "Monitora i prezzi dei voli su ricerche salvate"
)]
pub struct Cli {
    #[command(flatten)]
    config: Config,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Rebuild the airport, country and airline datasets from OurAirports/OpenFlights.
    ImportGeo {
        /// Use already downloaded raw files instead of fetching them.
        #[arg(long)]
        sources: Option<std::path::PathBuf>,
    },
    /// Run a single live search and print the normalised results.
    Search {
        /// Origin airports, comma separated (e.g. FCO,CIA).
        #[arg(long, value_delimiter = ',', required = true)]
        from: Vec<Iata>,
        /// Destination airports, comma separated.
        #[arg(long, value_delimiter = ',', required = true)]
        to: Vec<Iata>,
        #[arg(long)]
        date: NaiveDate,
        /// Search a round trip returning on this date.
        #[arg(long = "return")]
        return_date: Option<NaiveDate>,
        #[arg(long, default_value_t = 1)]
        max_stops: u8,
        #[arg(long, default_value_t = 1)]
        passengers: u8,
        #[arg(long, default_value = "EUR")]
        currency: String,
        /// Checked bags to include in the price.
        #[arg(long, default_value_t = 0)]
        bags: u8,
    },
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Command::ImportGeo { sources } => import_geo::run(&self.config.data_dir, sources).await,
            Command::Search {
                from,
                to,
                date,
                return_date,
                max_stops,
                passengers,
                currency,
                bags,
            } => {
                let args = search::SearchArgs {
                    from,
                    to,
                    date,
                    return_date,
                    max_stops,
                    passengers,
                    currency,
                    bags,
                };
                search::run(&self.config, args).await
            }
        }
    }
}
