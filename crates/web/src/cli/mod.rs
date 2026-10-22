mod import_geo;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "flight-monitor",
    about = "Monitora i prezzi dei voli su ricerche salvate"
)]
pub struct Cli {
    /// Directory with the reference datasets.
    #[arg(long, env = "FM_DATA_DIR", default_value = "data", global = true)]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Rebuild the airport, country and airline datasets from OurAirports/OpenFlights.
    ImportGeo {
        /// Use already downloaded raw files instead of fetching them.
        #[arg(long)]
        sources: Option<PathBuf>,
    },
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Command::ImportGeo { sources } => import_geo::run(&self.data_dir, sources).await,
        }
    }
}
