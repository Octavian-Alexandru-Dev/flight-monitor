use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use providers::{ExternalWorker, FlightProvider, GoogleFlights};

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
    #[arg(
        long,
        env = "FM_PYTHON",
        default_value = "workers/.venv/bin/python",
        global = true
    )]
    pub python: PathBuf,
    #[arg(
        long,
        env = "FM_GOOGLE_WORKER",
        default_value = "workers/google_flights.py",
        global = true
    )]
    pub google_worker: PathBuf,
    /// Minimum pause between two Google Flights requests.
    #[arg(
        long,
        env = "FM_GOOGLE_MIN_INTERVAL_SECS",
        default_value_t = 12,
        global = true
    )]
    pub google_min_interval_secs: u64,
    #[arg(
        long,
        env = "FM_WORKER_TIMEOUT_SECS",
        default_value_t = 60,
        global = true
    )]
    pub worker_timeout_secs: u64,
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
    pub fn google_flights(&self) -> Arc<dyn FlightProvider> {
        let worker = ExternalWorker::new(
            &self.python,
            vec![self.google_worker.display().to_string()],
            Duration::from_secs(self.worker_timeout_secs),
        );
        Arc::new(GoogleFlights::new(
            worker,
            Duration::from_secs(self.google_min_interval_secs),
        ))
    }
}
