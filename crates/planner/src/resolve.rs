use domain::{Iata, LocationSet, SearchDefinition};
use geo::GeoIndex;

/// A search's location sets turned into airport lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub origins: Vec<Iata>,
    /// `None` for an open ("anywhere") destination.
    pub destinations: Option<Vec<Iata>>,
    pub return_origins: Vec<Iata>,
    pub return_destinations: Vec<Iata>,
    pub hubs: Vec<Iata>,
    pub companions: Vec<(String, Vec<Iata>)>,
}

impl Resolved {
    pub fn all_airports(&self) -> usize {
        self.origins.len() + self.destinations.as_ref().map_or(0, Vec::len)
    }
}

pub fn resolve(def: &SearchDefinition, geo: &GeoIndex) -> Result<Resolved, geo::GeoError> {
    let destinations = match def.destination {
        LocationSet::Anywhere => None,
        ref set => Some(geo.resolve(set)?),
    };
    let return_origins = match (&def.return_origin, &destinations) {
        (Some(set), _) => geo.resolve(set)?,
        (None, Some(destinations)) => destinations.clone(),
        (None, None) => Vec::new(),
    };
    Ok(Resolved {
        origins: geo.resolve(&def.origin)?,
        return_destinations: geo.resolve(def.return_destination())?,
        return_origins,
        destinations,
        hubs: match &def.via {
            Some(via) => geo.resolve(&via.hubs)?,
            None => Vec::new(),
        },
        companions: def
            .companions
            .iter()
            .map(|c| Ok((c.name.clone(), geo.resolve(&c.origin)?)))
            .collect::<Result<_, geo::GeoError>>()?,
    })
}
