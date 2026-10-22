use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::DomainError;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Iata(String);

impl Iata {
    pub fn new(code: &str) -> Result<Self, DomainError> {
        let normalized = code.trim().to_ascii_uppercase();
        if normalized.len() == 3 && normalized.bytes().all(|b| b.is_ascii_uppercase()) {
            Ok(Self(normalized))
        } else {
            Err(DomainError::InvalidIata(code.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Iata {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for Iata {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl From<Iata> for String {
    fn from(value: Iata) -> Self {
        value.0
    }
}

impl fmt::Display for Iata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_and_whitespace() {
        assert_eq!(Iata::new(" fco ").map(|c| c.0), Ok("FCO".to_string()));
    }

    #[test]
    fn rejects_malformed_codes() {
        for bad in ["FC", "FCOO", "F1O", ""] {
            assert!(Iata::new(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn deserialization_validates() {
        assert!(serde_json::from_str::<Iata>("\"MXP\"").is_ok());
        assert!(serde_json::from_str::<Iata>("\"MX\"").is_err());
    }
}
