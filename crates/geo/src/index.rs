use std::collections::BTreeMap;
use std::path::Path;

use domain::{Iata, LocationSet};
use serde::{Deserialize, Serialize};

use crate::GeoError;
use crate::text::{fold, names_place};

const EARTH_RADIUS_KM: f64 = 6371.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AirportKind {
    Large,
    Medium,
    Small,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Airport {
    pub iata: Iata,
    pub name: String,
    pub city: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
    pub kind: AirportKind,
}

impl Airport {
    fn is_major(&self) -> bool {
        self.kind != AirportKind::Small
    }

    fn distance_km(&self, lat: f64, lon: f64) -> f64 {
        haversine_km(self.lat, self.lon, lat, lon)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SuggestionKind {
    Airport,
    City,
    Country,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    /// What the location field should contain when this suggestion is picked.
    pub value: String,
    pub label: String,
    pub detail: String,
}

#[derive(Deserialize)]
struct MetroRow {
    code: String,
    name: String,
    aliases: String,
    airports: String,
}

struct Metro {
    code: String,
    name: String,
    keys: Vec<String>,
    airports: Vec<Iata>,
}

#[derive(Deserialize)]
struct NamedRow {
    code: String,
    name: String,
}

pub struct GeoIndex {
    airports: BTreeMap<Iata, Airport>,
    countries: BTreeMap<String, String>,
    airlines: BTreeMap<String, String>,
    metros: Vec<Metro>,
}

impl GeoIndex {
    pub fn load(dir: &Path) -> Result<Self, GeoError> {
        let airports: BTreeMap<Iata, Airport> = read_csv::<Airport>(&dir.join("airports.csv"))?
            .into_iter()
            .map(|a| (a.iata.clone(), a))
            .collect();
        let named = |file: &str| -> Result<BTreeMap<String, String>, GeoError> {
            Ok(read_csv::<NamedRow>(&dir.join(file))?
                .into_iter()
                .map(|r| (r.code, r.name))
                .collect())
        };
        let metros = read_csv::<MetroRow>(&dir.join("metro_areas.csv"))?
            .into_iter()
            .map(|row| {
                let mut keys = vec![fold(&row.code), fold(&row.name)];
                keys.extend(row.aliases.split('|').filter(|a| !a.is_empty()).map(fold));
                Metro {
                    airports: row
                        .airports
                        .split_whitespace()
                        .filter_map(|code| Iata::new(code).ok())
                        .filter(|code| airports.contains_key(code))
                        .collect(),
                    code: row.code,
                    name: row.name,
                    keys,
                }
            })
            .collect();
        Ok(Self {
            countries: named("countries.csv")?,
            airlines: named("airlines.csv")?,
            airports,
            metros,
        })
    }

    pub fn airport(&self, code: &Iata) -> Option<&Airport> {
        self.airports.get(code)
    }

    pub fn airport_count(&self) -> usize {
        self.airports.len()
    }

    pub fn country_name(&self, code: &str) -> Option<&str> {
        self.countries.get(code).map(String::as_str)
    }

    pub fn airline_name(&self, code: &str) -> Option<&str> {
        self.airlines.get(code).map(String::as_str)
    }

    pub fn resolve(&self, set: &LocationSet) -> Result<Vec<Iata>, GeoError> {
        let codes = match set {
            LocationSet::Airport(code) => vec![self.known(code)?],
            LocationSet::Airports(codes) => codes
                .iter()
                .map(|c| self.known(c))
                .collect::<Result<_, _>>()?,
            LocationSet::City(name) => self.city(name)?,
            LocationSet::Country(code) => self.country(code)?,
            LocationSet::Radius { lat, lon, km } => self.radius(*lat, *lon, *km)?,
            LocationSet::Anywhere => return Err(GeoError::OpenDestination),
        };
        Ok(self.ranked(codes))
    }

    /// Whether two airports serve the same city, so moving between them needs ground transport.
    pub fn same_city(&self, a: &Iata, b: &Iata) -> bool {
        if self
            .metros
            .iter()
            .any(|m| m.airports.contains(a) && m.airports.contains(b))
        {
            return true;
        }
        match (self.airport(a), self.airport(b)) {
            (Some(a), Some(b)) => fold(&a.city) == fold(&b.city),
            _ => false,
        }
    }

    pub fn suggest(&self, term: &str, limit: usize) -> Vec<Suggestion> {
        let needle = fold(term);
        if needle.len() < 2 {
            return Vec::new();
        }
        let mut out: Vec<(u8, Suggestion)> = Vec::new();
        for metro in &self.metros {
            if metro.keys.iter().any(|k| k.starts_with(&needle)) {
                let codes: Vec<&str> = metro.airports.iter().map(Iata::as_str).collect();
                out.push((
                    1,
                    Suggestion {
                        kind: SuggestionKind::City,
                        value: metro.name.clone(),
                        label: format!("{} (tutti gli aeroporti)", metro.name),
                        detail: format!("{} · {}", metro.code, codes.join(" ")),
                    },
                ));
            }
        }
        for airport in self.airports.values() {
            let score = if fold(airport.iata.as_str()) == needle {
                0
            } else if names_place(&needle, &fold(&airport.city)) {
                2
            } else if fold(&airport.name).contains(&needle) {
                3
            } else {
                continue;
            };
            out.push((
                score + u8::from(!airport.is_major()),
                Suggestion {
                    kind: SuggestionKind::Airport,
                    value: airport.iata.to_string(),
                    label: format!("{} · {}", airport.iata, airport.name),
                    detail: format!("{}, {}", airport.city, airport.country),
                },
            ));
        }
        for (code, name) in &self.countries {
            if fold(code) == needle || fold(name).starts_with(&needle) {
                out.push((
                    4,
                    Suggestion {
                        kind: SuggestionKind::Country,
                        value: code.clone(),
                        label: name.clone(),
                        detail: format!("Paese · {code}"),
                    },
                ));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.cmp(&b.1.label)));
        out.into_iter().take(limit).map(|(_, s)| s).collect()
    }

    fn known(&self, code: &Iata) -> Result<Iata, GeoError> {
        if self.airports.contains_key(code) {
            Ok(code.clone())
        } else {
            Err(GeoError::UnknownAirport(code.to_string()))
        }
    }

    fn city(&self, name: &str) -> Result<Vec<Iata>, GeoError> {
        let needle = fold(name);
        if let Some(metro) = self.metros.iter().find(|m| m.keys.contains(&needle)) {
            return Ok(metro.airports.clone());
        }
        let codes: Vec<Iata> = self
            .airports
            .values()
            .filter(|a| names_place(&needle, &fold(&a.city)))
            .map(|a| a.iata.clone())
            .collect();
        if codes.is_empty() {
            Err(GeoError::UnknownCity(name.to_string()))
        } else {
            Ok(codes)
        }
    }

    fn country(&self, code: &str) -> Result<Vec<Iata>, GeoError> {
        let code = code.trim().to_ascii_uppercase();
        let codes: Vec<Iata> = self
            .airports
            .values()
            .filter(|a| a.country == code && a.is_major())
            .map(|a| a.iata.clone())
            .collect();
        if codes.is_empty() {
            Err(GeoError::UnknownCountry(code))
        } else {
            Ok(codes)
        }
    }

    fn radius(&self, lat: f64, lon: f64, km: f64) -> Result<Vec<Iata>, GeoError> {
        let codes: Vec<Iata> = self
            .airports
            .values()
            .filter(|a| a.is_major() && a.distance_km(lat, lon) <= km)
            .map(|a| a.iata.clone())
            .collect();
        if codes.is_empty() {
            Err(GeoError::EmptyRadius(km))
        } else {
            Ok(codes)
        }
    }

    /// Large airports first, then alphabetical: deterministic and puts hubs up front.
    fn ranked(&self, mut codes: Vec<Iata>) -> Vec<Iata> {
        codes.sort_by_key(|c| (self.airport(c).map(|a| a.kind), c.clone()));
        codes.dedup();
        codes
    }
}

fn read_csv<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, GeoError> {
    let mut reader = csv::Reader::from_path(path)?;
    reader
        .deserialize()
        .collect::<Result<Vec<T>, _>>()
        .map_err(GeoError::from)
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (phi1, phi2) = (lat1.to_radians(), lat2.to_radians());
    let d_phi = (lat2 - lat1).to_radians();
    let d_lambda = (lon2 - lon1).to_radians();
    let a = (d_phi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (d_lambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}
