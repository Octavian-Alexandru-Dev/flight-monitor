use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Iata;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LocationSet {
    Airport(Iata),
    Airports(Vec<Iata>),
    City(String),
    /// ISO 3166-1 alpha-2 country code.
    Country(String),
    Radius {
        lat: f64,
        lon: f64,
        km: f64,
    },
    /// Open destination: only resolvable through a provider that supports it.
    Anywhere,
}

impl fmt::Display for LocationSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Airport(code) => write!(f, "{code}"),
            Self::Airports(codes) => {
                let joined: Vec<&str> = codes.iter().map(Iata::as_str).collect();
                write!(f, "{}", joined.join(", "))
            }
            Self::City(name) => write!(f, "{name}"),
            Self::Country(code) => write!(f, "{code}"),
            Self::Radius { lat, lon, km } => write!(f, "{km:.0} km da {lat:.3}, {lon:.3}"),
            Self::Anywhere => write!(f, "Ovunque"),
        }
    }
}
