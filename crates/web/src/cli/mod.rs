mod import_geo;
mod search;

use std::path::PathBuf;

use anyhow::Context;
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use domain::{Iata, SearchDefinition, SearchId};

use crate::config::Config;
use crate::scheduler::{self, SchedulerSettings};
use crate::{app, format};

#[derive(Parser)]
#[command(
    name = "flight-monitor",
    about = "Monitora i prezzi dei voli su ricerche salvate"
)]
pub struct Cli {
    #[command(flatten)]
    config: Config,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the scheduler and the web interface (default).
    Serve,
    /// Run searches once: the due ones, all active ones, or a single one.
    RunOnce {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        search: Option<i64>,
    },
    /// Save searches from a JSON file (one definition or an array).
    Seed { file: PathBuf },
    /// Print the Telegram daily summary (and send it with --send).
    Summary {
        #[arg(long)]
        send: bool,
    },
    /// Rebuild the airport, country and airline datasets from OurAirports/OpenFlights.
    ImportGeo {
        /// Use already downloaded raw files instead of fetching them.
        #[arg(long)]
        sources: Option<PathBuf>,
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
        let config = self.config;
        match self.command.unwrap_or(Command::Serve) {
            Command::Serve => serve(config).await,
            Command::RunOnce { all, search } => run_once(&config, all, search).await,
            Command::Seed { file } => seed(&config, &file).await,
            Command::Summary { send } => {
                let engine = app::engine(&config).await?;
                let message = scheduler::summary(&engine).await?;
                println!("{message}");
                if send {
                    engine.notifier().send(&message).await?;
                }
                Ok(())
            }
            Command::ImportGeo { sources } => import_geo::run(&config.data_dir, sources).await,
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
                search::run(&config, args).await
            }
        }
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    let engine = app::engine(&config).await?;
    let settings = SchedulerSettings {
        daily_summary_hour: config.daily_summary_hour,
        retention_days: config.retention_days,
    };
    tracing::info!("scheduler started");
    scheduler::run(engine, settings).await;
    Ok(())
}

async fn run_once(config: &Config, all: bool, search: Option<i64>) -> anyhow::Result<()> {
    let engine = app::engine(config).await?;
    let report = match (search, all) {
        (Some(id), _) => {
            let record = engine.db.get_search(SearchId(id)).await?;
            engine.run_searches(vec![record.search]).await?
        }
        (None, true) => {
            let active = engine
                .db
                .list_searches()
                .await?
                .into_iter()
                .filter(|r| r.search.active)
                .map(|r| r.search)
                .collect();
            engine.run_searches(active).await?
        }
        (None, false) => engine.run_due().await?,
    };
    println!(
        "{} ricerche · {} query eseguite · {} dalla cache · {} fallite · {} alert",
        report.searches, report.executed, report.cached, report.failed, report.alerts
    );
    for record in engine.db.list_searches().await? {
        let Some(run) = engine.db.latest_search_run(record.search.id).await? else {
            continue;
        };
        let best = run
            .best_price
            .map_or_else(|| "—".to_string(), |p| format::price(p, &run.currency));
        println!(
            "#{} {}: {best} ({} opzioni)",
            record.search.id.0,
            record.search.definition.name,
            run.options.len()
        );
    }
    Ok(())
}

async fn seed(config: &Config, file: &PathBuf) -> anyhow::Result<()> {
    let raw = tokio::fs::read_to_string(file)
        .await
        .with_context(|| format!("reading {}", file.display()))?;
    let definitions: Vec<SearchDefinition> = match serde_json::from_str(&raw) {
        Ok(many) => many,
        Err(_) => vec![serde_json::from_str(&raw).context("parsing search definition")?],
    };
    let engine = app::engine(config).await?;
    for definition in definitions {
        definition.validate()?;
        let estimate = engine.planner().estimate(&definition)?;
        let id = engine.db.create_search(&definition).await?;
        println!(
            "#{} {} salvata: ~{} query live per ciclo ({} combinazioni)",
            id.0, definition.name, estimate.live_per_cycle, estimate.candidates
        );
    }
    Ok(())
}
