use crate::config::ConfigManager;
use crate::models::FlightMetrics;
use crate::sim_monitor::calculate_distance;
use crate::UnifiedMonitor;
use serde::Deserialize;
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[derive(Debug, Deserialize)]
struct VatsimData {
    pilots: Vec<VatsimPilot>,
}

#[derive(Debug, Deserialize)]
struct VatsimPilot {
    callsign: String,
    latitude: f64,
    longitude: f64,
    altitude: f64,
    heading: f64,
    groundspeed: f64,
    flight_plan: Option<VatsimFlightPlan>,
}

#[derive(Debug, Deserialize)]
struct VatsimFlightPlan {
    #[serde(default)]
    aircraft: String,
    #[serde(default)]
    aircraft_short: String,
}

pub struct VatsimManager;

impl VatsimManager {
    pub fn new() -> Self {
        Self
    }

    pub fn start(&self, app: AppHandle) {
        std::thread::spawn(move || {
            let client = match reqwest::blocking::Client::builder()
                .user_agent("butterlog-app/0.3.3")
                .timeout(Duration::from_secs(10))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    crate::append_log(&app, format!("[VATSIM] Failed to build HTTP client: {}", e));
                    return;
                }
            };

            loop {
                // Read configuration
                let config = app.state::<ConfigManager>().get_config();

                if config.enable_vatsim_traffic {
                    // Check if monitor is connected
                    let unified = app.state::<UnifiedMonitor>();
                    if let Some(monitor) = unified.get_connected_monitor() {
                        if monitor.is_connected() {
                            let self_metrics = monitor.get_metrics();
                            // Validate user coordinate
                            if self_metrics.latitude != 0.0 || self_metrics.longitude != 0.0 {
                                match client.get("https://data.vatsim.net/v3/vatsim-data.json").send() {
                                    Ok(resp) => {
                                        if resp.status().is_success() {
                                            match resp.json::<VatsimData>() {
                                                Ok(data) => {
                                                    let mut count = 0;
                                                    for pilot in data.pilots {
                                                        let dist = calculate_distance(
                                                            self_metrics.latitude,
                                                            self_metrics.longitude,
                                                            pilot.latitude,
                                                            pilot.longitude,
                                                        );
                                                        if dist <= 20.0 {
                                                            count += 1;
                                                            let title = pilot.flight_plan
                                                                .as_ref()
                                                                .map(|fp| {
                                                                    if !fp.aircraft_short.is_empty() {
                                                                        fp.aircraft_short.clone()
                                                                    } else {
                                                                        fp.aircraft.clone()
                                                                    }
                                                                })
                                                                .unwrap_or_else(|| "".to_string());

                                                            let mut remote_metrics = FlightMetrics::default();
                                                            remote_metrics.latitude = pilot.latitude;
                                                            remote_metrics.longitude = pilot.longitude;
                                                            remote_metrics.gps_altitude_msl = pilot.altitude;
                                                            remote_metrics.heading = pilot.heading;
                                                            remote_metrics.ground_speed = pilot.groundspeed;

                                                            monitor.update_remote_aircraft(
                                                                &pilot.callsign,
                                                                &title,
                                                                &remote_metrics,
                                                            );
                                                        }
                                                    }
                                                    crate::append_log(&app, format!("[VATSIM] Synced traffic. Found {} pilots within 20 NM", count));
                                                }
                                                Err(e) => {
                                                    crate::append_log(&app, format!("[VATSIM] Failed to parse VATSIM JSON: {}", e));
                                                }
                                            }
                                        } else {
                                            crate::append_log(&app, format!("[VATSIM] HTTP request failed with status: {}", resp.status()));
                                        }
                                    }
                                    Err(e) => {
                                        crate::append_log(&app, format!("[VATSIM] Failed to fetch VATSIM network data: {}", e));
                                    }
                                }
                            }
                        }
                    }
                }

                // Run every 15 seconds
                std::thread::sleep(Duration::from_secs(15));
            }
        });
    }
}
