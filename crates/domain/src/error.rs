use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("invalid IATA airport code `{0}`: expected 3 uppercase letters")]
    InvalidIata(String),
    #[error("invalid search: {0}")]
    InvalidSearch(String),
}
