use crate::models::FlightMetrics;
use std::path::PathBuf;
use tauri::AppHandle;

pub mod msfs;
pub mod xplane;

pub trait SimMonitor: Send + Sync {
    fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()>;
    fn stop(&self);
    fn get_metrics(&self) -> FlightMetrics;
    fn is_connected(&self) -> bool;
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
