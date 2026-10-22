use std::path::{Path, PathBuf};

use anyhow::Context;

const SOURCES: [(&str, &str); 3] = [
    (
        "airports.csv",
        "https://davidmegginson.github.io/ourairports-data/airports.csv",
    ),
    (
        "countries.csv",
        "https://davidmegginson.github.io/ourairports-data/countries.csv",
    ),
    (
        "airlines.dat",
        "https://raw.githubusercontent.com/jpatokal/openflights/master/data/airlines.dat",
    ),
];

pub async fn run(data_dir: &Path, sources: Option<PathBuf>) -> anyhow::Result<()> {
    let source_dir = match sources {
        Some(dir) => dir,
        None => download().await?,
    };
    let count = geo::import_sources(&source_dir, data_dir)
        .with_context(|| format!("importing from {}", source_dir.display()))?;
    tracing::info!(airports = count, dir = %data_dir.display(), "reference data imported");
    Ok(())
}

async fn download() -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join("flight-monitor-geo");
    tokio::fs::create_dir_all(&dir).await?;
    let client = reqwest::Client::new();
    for (file, url) in SOURCES {
        tracing::info!(url, "downloading");
        let body = client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        tokio::fs::write(dir.join(file), body).await?;
    }
    Ok(dir)
}
