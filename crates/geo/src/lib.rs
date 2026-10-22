mod import;
mod index;
mod text;

pub use import::import_sources;
pub use index::{Airport, AirportKind, GeoIndex, Suggestion, SuggestionKind};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GeoError {
    #[error("cannot read reference data: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed reference data: {0}")]
    Csv(#[from] csv::Error),
    #[error("aeroporto sconosciuto: {0}")]
    UnknownAirport(String),
    #[error("nessun aeroporto trovato per la città «{0}»")]
    UnknownCity(String),
    #[error("nessun aeroporto commerciale per il paese «{0}»")]
    UnknownCountry(String),
    #[error("nessun aeroporto commerciale entro {0:.0} km")]
    EmptyRadius(f64),
    #[error("«Ovunque» si risolve solo tramite una fonte che supporta destinazioni aperte")]
    OpenDestination,
}
