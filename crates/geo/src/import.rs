use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::GeoError;
use crate::index::{Airport, AirportKind};

#[derive(Serialize)]
struct NamedRow<'a> {
    code: &'a str,
    name: &'a str,
}

/// Builds the compact reference files in `out_dir` from the raw OurAirports
/// (`airports.csv`, `countries.csv`) and OpenFlights (`airlines.dat`) dumps.
/// Returns the number of airports written.
pub fn import_sources(source_dir: &Path, out_dir: &Path) -> Result<usize, GeoError> {
    let airports = commercial_airports(&source_dir.join("airports.csv"))?;
    let mut writer = csv::Writer::from_path(out_dir.join("airports.csv"))?;
    for airport in &airports {
        writer.serialize(airport)?;
    }
    writer.flush()?;

    let mut countries = csv::Reader::from_path(source_dir.join("countries.csv"))?;
    let headers = countries.headers()?.clone();
    let mut writer = csv::Writer::from_path(out_dir.join("countries.csv"))?;
    for record in countries.records() {
        let record = record?;
        let row = record.deserialize::<BTreeMap<String, String>>(Some(&headers))?;
        let (Some(code), Some(name)) = (row.get("code"), row.get("name")) else {
            continue;
        };
        writer.serialize(NamedRow { code, name })?;
    }
    writer.flush()?;

    let mut airlines = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_path(source_dir.join("airlines.dat"))?;
    let mut seen = BTreeMap::new();
    for record in airlines.records() {
        let record = record?;
        let (Some(name), Some(iata), Some(active)) = (record.get(1), record.get(3), record.get(7))
        else {
            continue;
        };
        let valid = iata.len() == 2 && iata.bytes().all(|b| b.is_ascii_alphanumeric());
        if active == "Y" && valid {
            seen.entry(iata.to_string())
                .or_insert_with(|| name.to_string());
        }
    }
    let mut writer = csv::Writer::from_path(out_dir.join("airlines.csv"))?;
    for (code, name) in &seen {
        writer.serialize(NamedRow { code, name })?;
    }
    writer.flush()?;

    Ok(airports.len())
}

fn commercial_airports(path: &Path) -> Result<Vec<Airport>, GeoError> {
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader.headers()?.clone();
    let mut airports = Vec::new();
    for record in reader.records() {
        let row = record?.deserialize::<BTreeMap<String, String>>(Some(&headers))?;
        let field = |name: &str| row.get(name).map(String::as_str).unwrap_or_default();
        let kind = match field("type") {
            "large_airport" => AirportKind::Large,
            "medium_airport" => AirportKind::Medium,
            "small_airport" => AirportKind::Small,
            _ => continue,
        };
        let Ok(iata) = domain::Iata::new(field("iata_code")) else {
            continue;
        };
        let (Ok(lat), Ok(lon)) = (
            field("latitude_deg").parse(),
            field("longitude_deg").parse(),
        ) else {
            continue;
        };
        if field("scheduled_service") != "yes" {
            continue;
        }
        airports.push(Airport {
            iata,
            name: field("name").to_string(),
            city: field("municipality").to_string(),
            country: field("iso_country").to_string(),
            lat,
            lon,
            kind,
        });
    }
    airports.sort_by(|a, b| a.iata.cmp(&b.iata));
    airports.dedup_by(|a, b| a.iata == b.iata);
    Ok(airports)
}
