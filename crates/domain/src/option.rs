use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{Iata, Itinerary};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Outbound,
    Return,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    pub traveler: Option<String>,
    pub direction: Direction,
    pub itinerary: Itinerary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Warning {
    SelfTransfer { hub: Iata, minutes: i64 },
    AirportChange { from: Iata, to: Iata },
    OvernightLayover { airport: Iata },
    Stopover { hub: Iata, hours: i64 },
    ArrivalSpread { minutes: i64 },
}

impl Warning {
    pub fn message(&self) -> String {
        match self {
            Self::SelfTransfer { hub, minutes } => format!(
                "Biglietti separati a {hub} ({}h{:02}): nessuna protezione sulla coincidenza",
                minutes / 60,
                minutes % 60
            ),
            Self::AirportChange { from, to } => {
                format!("Cambio aeroporto {from} → {to}: serve un trasferimento")
            }
            Self::OvernightLayover { airport } => {
                format!("Scalo notturno a {airport}: valuta un pernottamento")
            }
            Self::Stopover { hub, hours } => format!("Stopover di {hours}h a {hub}"),
            Self::ArrivalSpread { minutes } => {
                format!("Arrivi distanziati di {}h{:02}", minutes / 60, minutes % 60)
            }
        }
    }

    pub fn is_risk(&self) -> bool {
        !matches!(self, Self::Stopover { .. })
    }
}

/// Best price for a departure date (and return date, for round trips) in the latest run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalendarCell {
    pub outbound: NaiveDate,
    pub inbound: Option<NaiveDate>,
    pub price: Decimal,
}

/// A complete, purchasable way to travel: one or more tickets bought separately.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TripOption {
    pub tickets: Vec<Ticket>,
    pub total_price: Decimal,
    /// Door-to-door extras (airport access, overnight stays) on top of the tickets.
    pub extra_cost: Decimal,
    pub currency: String,
    pub warnings: Vec<Warning>,
}

impl TripOption {
    pub fn new(tickets: Vec<Ticket>, currency: String) -> Self {
        let total_price = tickets.iter().map(|t| t.itinerary.price).sum();
        Self {
            tickets,
            total_price,
            extra_cost: Decimal::ZERO,
            currency,
            warnings: Vec::new(),
        }
    }

    pub fn door_to_door(&self) -> Decimal {
        self.total_price + self.extra_cost
    }

    pub fn key(&self) -> String {
        let keys: Vec<String> = self
            .tickets
            .iter()
            .map(|t| match &t.traveler {
                Some(name) => format!("{name}:{}", t.itinerary.key()),
                None => t.itinerary.key(),
            })
            .collect();
        keys.join(" & ")
    }

    pub fn outbound_date(&self) -> Option<NaiveDate> {
        self.tickets
            .iter()
            .find(|t| t.direction == Direction::Outbound)
            .map(|t| t.itinerary.departure_date())
    }

    pub fn return_date(&self) -> Option<NaiveDate> {
        self.tickets
            .iter()
            .find(|t| t.direction == Direction::Return)
            .map(|t| t.itinerary.departure_date())
            .or_else(|| self.tickets.iter().find_map(|t| t.itinerary.return_date))
    }

    pub fn travelers(&self) -> Vec<Option<&str>> {
        let mut names: Vec<Option<&str>> = Vec::new();
        for ticket in &self.tickets {
            let name = ticket.traveler.as_deref();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    pub fn tickets_of(&self, traveler: Option<&str>) -> Vec<&Ticket> {
        self.tickets
            .iter()
            .filter(|t| t.traveler.as_deref() == traveler)
            .collect()
    }
}
