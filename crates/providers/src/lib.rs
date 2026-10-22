mod error;
mod google_flights;
mod travelpayouts;
mod worker;

pub use error::ProviderError;
pub use google_flights::{GoogleFlights, GoogleReply};
pub use travelpayouts::{Travelpayouts, parse_response as parse_travelpayouts};
pub use worker::ExternalWorker;

use async_trait::async_trait;
use domain::{Capabilities, Itinerary, ProviderId, ProviderQuery};

#[async_trait]
pub trait FlightProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> &Capabilities;
    async fn search(&self, query: &ProviderQuery) -> Result<Vec<Itinerary>, ProviderError>;
}
