mod error;

pub use error::ProviderError;

use async_trait::async_trait;
use domain::{Capabilities, Itinerary, ProviderId, ProviderQuery};

#[async_trait]
pub trait FlightProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn capabilities(&self) -> &Capabilities;
    async fn search(&self, query: &ProviderQuery) -> Result<Vec<Itinerary>, ProviderError>;
}
