mod advice;
mod combine;
mod filter;
mod needs;
mod plan;
mod resolve;
mod schedule;

pub use advice::{Advice, Verdict, advise};
pub use combine::{Outcome, build_options};
pub use filter::passes_constraints;
pub use plan::{Estimate, LivePlan, Planner, Strategy, dedup};
pub use resolve::{Resolved, resolve};
pub use schedule::next_run;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlanError {
    #[error(transparent)]
    Geo(#[from] geo::GeoError),
    #[error("«Ovunque» richiede una fonte di scan (token Travelpayouts)")]
    NeedsScanProvider,
}
