use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use providers::{ChromeSource, FlightProvider, GoogleFlights, HeadlessBrowser, Travelpayouts};

/// Runtime settings, read from flags or environment (`.env` is loaded at startup).
#[derive(Args, Clone, Debug)]
pub struct Config {
    #[arg(
        long,
        env = "FM_DATABASE_URL",
        default_value = "sqlite://data/flight-monitor.db",
        global = true
    )]
    pub database_url: String,
    #[arg(long, env = "FM_DATA_DIR", default_value = "data", global = true)]
    pub data_dir: PathBuf,
    #[arg(long, env = "FM_BIND", default_value = "127.0.0.1:8080", global = true)]
    pub bind: SocketAddr,
    /// Public base URL of the web UI, for links in notifications.
    #[arg(long, env = "FM_PUBLIC_URL", global = true)]
    pub public_url: Option<String>,
    /// Chrome or Chromium used when Google does not embed the results in the page.
    /// When unset, an installed Chrome is used or Chromium is downloaded into the data dir.
    #[arg(long, env = "FM_CHROME", global = true)]
    pub chrome: Option<PathBuf>,
    /// Minimum pause between two Google Flights requests.
    #[arg(
        long,
        env = "FM_GOOGLE_MIN_INTERVAL_SECS",
        default_value_t = 20,
        global = true
    )]
    pub google_min_interval_secs: u64,
    #[arg(
        long,
        env = "TELEGRAM_BOT_TOKEN",
        hide_env_values = true,
        global = true
    )]
    pub telegram_token: Option<String>,
    #[arg(long, env = "TELEGRAM_CHAT_ID", global = true)]
    pub telegram_chat_id: Option<String>,
    #[arg(
        long,
        env = "TRAVELPAYOUTS_TOKEN",
        hide_env_values = true,
        global = true
    )]
    pub travelpayouts_token: Option<String>,
    /// Identical queries newer than this are served from storage instead of the source.
    #[arg(
        long,
        env = "FM_CACHE_TTL_MINUTES",
        default_value_t = 60,
        global = true
    )]
    pub cache_ttl_minutes: i64,
    /// Live queries verified per search after a scan.
    #[arg(long, env = "FM_VERIFY_TOP_N", default_value_t = 8, global = true)]
    pub verify_top_n: usize,
    #[arg(long, env = "FM_DEFAULT_BUDGET", default_value_t = 40, global = true)]
    pub default_budget: u32,
    #[arg(
        long,
        env = "FM_DEFAULT_POLL_HOURS",
        default_value_t = 6,
        global = true
    )]
    pub default_poll_hours: u32,
    /// Local hour for the Telegram daily summary; unset disables it.
    #[arg(long, env = "FM_DAILY_SUMMARY_HOUR", global = true)]
    pub daily_summary_hour: Option<u32>,
    /// Raw observations older than this are pruned (search history is kept).
    #[arg(long, env = "FM_RETENTION_DAYS", default_value_t = 120, global = true)]
    pub retention_days: i64,
    /// OpenAI-compatible chat endpoint (e.g. a local Ollama) for natural-language searches.
    #[arg(long, env = "FM_LLM_URL", global = true)]
    pub llm_url: Option<String>,
    #[arg(long, env = "FM_LLM_MODEL", default_value = "llama3.1", global = true)]
    pub llm_model: String,
}

impl Config {
    /// The scan provider, when a Travelpayouts token is configured.
    pub fn travelpayouts(&self) -> anyhow::Result<Option<Arc<dyn FlightProvider>>> {
        match self
            .travelpayouts_token
            .as_deref()
            .filter(|t| !t.is_empty())
        {
            Some(token) => Ok(Some(Arc::new(Travelpayouts::new(token)?))),
            None => Ok(None),
        }
    }

    pub fn headless_browser(&self) -> HeadlessBrowser {
        HeadlessBrowser::new(ChromeSource {
            executable: self.chrome.clone(),
            download_dir: self.data_dir.join("chromium"),
        })
    }

    pub fn google_flights(&self) -> anyhow::Result<Arc<dyn FlightProvider>> {
        Ok(Arc::new(GoogleFlights::new(
            self.headless_browser(),
            Duration::from_secs(self.google_min_interval_secs),
        )?))
    }
}
