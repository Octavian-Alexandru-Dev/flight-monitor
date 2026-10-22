CREATE TABLE saved_searches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    definition TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_run_at TEXT,
    next_run_at TEXT
);

CREATE TABLE search_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    search_id INTEGER NOT NULL REFERENCES saved_searches (id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    finished_at TEXT NOT NULL,
    queries_planned INTEGER NOT NULL,
    queries_executed INTEGER NOT NULL,
    queries_failed INTEGER NOT NULL,
    best_price_cents INTEGER,
    currency TEXT NOT NULL,
    options TEXT NOT NULL,
    calendar TEXT NOT NULL
);
CREATE INDEX idx_search_runs_search ON search_runs (search_id, started_at);

CREATE TABLE query_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    query_key TEXT NOT NULL,
    description TEXT NOT NULL,
    started_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    outcome TEXT NOT NULL,
    error TEXT,
    result_count INTEGER NOT NULL
);
CREATE INDEX idx_query_runs_key ON query_runs (query_key, started_at);
CREATE INDEX idx_query_runs_provider ON query_runs (provider, started_at);

CREATE TABLE price_observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    query_run_id INTEGER NOT NULL REFERENCES query_runs (id) ON DELETE CASCADE,
    itinerary_key TEXT NOT NULL,
    origin TEXT NOT NULL,
    destination TEXT NOT NULL,
    departure_date TEXT NOT NULL,
    return_date TEXT,
    price_cents INTEGER NOT NULL,
    currency TEXT NOT NULL,
    provider TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    itinerary TEXT NOT NULL
);
CREATE INDEX idx_observations_route ON price_observations (origin, destination, currency);
CREATE INDEX idx_observations_key ON price_observations (itinerary_key, observed_at);
CREATE INDEX idx_observations_run ON price_observations (query_run_id);

CREATE TABLE alerts_sent (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    search_id INTEGER NOT NULL REFERENCES saved_searches (id) ON DELETE CASCADE,
    option_key TEXT NOT NULL,
    price_cents INTEGER NOT NULL,
    reasons TEXT NOT NULL,
    message TEXT NOT NULL,
    sent_at TEXT NOT NULL
);
CREATE INDEX idx_alerts_search ON alerts_sent (search_id, option_key);

CREATE TABLE provider_health (
    provider TEXT PRIMARY KEY NOT NULL,
    status TEXT NOT NULL,
    consecutive_failures INTEGER NOT NULL,
    last_error TEXT,
    last_success_at TEXT,
    last_failure_at TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE app_state (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
