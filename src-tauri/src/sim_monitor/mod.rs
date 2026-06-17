use crate::models::FlightMetrics;
use std::path::PathBuf;
use tauri::AppHandle;

pub mod msfs;
pub mod xplane;

/// Sim-neutral identity of a remote (peer) aircraft, carried from the multiplayer
/// receiver into model selection. Bundles the raw sim fields with the sender's deduced
/// canonical identity (`resolved_icao` / `resolved_airline`) and the raw `livery`, so a
/// receiver can pick a local model + livery that best represents the friend.
#[derive(Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct RemoteAircraftIdentity {
    /// Free-text sim title (MSFS `TITLE` / X-Plane `acf_ui_name`).
    pub title: String,
    /// The sim's own ICAO type claim (MSFS `ATC MODEL` / X-Plane `acf_ICAO`); often empty.
    pub atc_model: String,
    /// ICAO type designator deduced by the sender from its title + livery. Preferred over
    /// `atc_model` when present, since the explicit field is unreliable on add-ons.
    pub resolved_icao: String,
    /// Operating airline name. Carried portably on the wire as `resolved_airline_icao`
    /// (an ICAO code); the receiver derives this name locally from its airline table to
    /// match against installed liveries / titles. May be empty.
    pub resolved_airline: String,
    /// ICAO designator of the operating airline (e.g. `UAL`) — the portable airline
    /// identity from the multiplayer wire. Used to pick the right livery on the receiver.
    pub resolved_airline_icao: String,
    /// Raw sim livery string (same-sim hint only).
    pub livery: String,
    pub object_class: String,
    pub category: String,
    pub num_engines: i32,
    pub engine_type: String,
}

pub trait SimMonitor: Send + Sync {
    fn id(&self) -> &'static str;
    fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()>;
    fn stop(&self);
    fn get_metrics(&self) -> FlightMetrics;
    fn get_aircraft_info(&self) -> crate::models::AircraftInfo;
    fn get_current_flight_id(&self) -> String;
    fn is_connected(&self) -> bool;
    fn is_monitoring(&self) -> bool;
    fn update_remote_aircraft(
        &self,
        id: &str,
        identity: &RemoteAircraftIdentity,
        metrics: &FlightMetrics,
    );
    /// Resolve the local model this monitor would use to represent the given remote
    /// aircraft, for observability (the multiplayer debug tab). Returns `None` when the
    /// receiver doesn't pick the model itself (X-Plane defers selection to its plugin).
    fn choose_remote_model(
        &self,
        _identity: &RemoteAircraftIdentity,
        _db: &crate::aircraft_characteristics::CharacteristicsDatabase,
    ) -> Option<String> {
        None
    }
}

pub fn calculate_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 3440.065; // Earth radius in nautical miles
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();

    let a = (d_lat / 2.0).sin() * (d_lat / 2.0).sin()
        + (d_lon / 2.0).sin() * (d_lon / 2.0).sin() * lat1.cos() * lat2.cos();
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}
