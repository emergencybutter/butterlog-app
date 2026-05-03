use crate::airports::AirportsDatabase;
use crate::flight_log_manager::{init_sqlite_db, insert_sqlite_row};
use crate::models::{AircraftInfo, FlightMetrics, WebhookFlightSummary, AirportInfo};
use crate::sim_monitor::SimMonitor;
use crate::webhook_manager::WebhookManager;
use crate::runways::RunwaysDatabase;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::fs::create_dir_all;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub struct XPlaneMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    aircraft_info: Arc<Mutex<AircraftInfo>>,
    current_flight_id: Arc<Mutex<String>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
    monitoring: Arc<Mutex<bool>>,
}

impl XPlaneMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(FlightMetrics::default())),
            aircraft_info: Arc::new(Mutex::new(AircraftInfo::default())),
            current_flight_id: Arc::new(Mutex::new(String::new())),
            running: Arc::new(Mutex::new(false)),
            connected: Arc::new(Mutex::new(false)),
            monitoring: Arc::new(Mutex::new(false)),
        }
    }

    fn run_monitor(
        app: &AppHandle,
        socket: UdpSocket,
        metrics: &Arc<Mutex<FlightMetrics>>,
        aircraft_info_mutex: &Arc<Mutex<AircraftInfo>>,
        current_flight_id_mutex: &Arc<Mutex<String>>,
        running: &Arc<Mutex<bool>>,
        connected: &Arc<Mutex<bool>>,
        monitoring: &Arc<Mutex<bool>>,
        _requested_log_path: Option<&PathBuf>,
    ) -> anyhow::Result<()> {
        let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
        let mut db_conn: Option<Connection> = None;
        let mut last_log_time = Utc::now();
        let mut current_log_path: Option<PathBuf> = None;
        let mut aircraft_info = AircraftInfo::default();
        let mut flight_ongoing = false;

        let mut takeoff_snapshot: Option<FlightMetrics> = None;
        let mut landing_snapshot: Option<FlightMetrics> = None;
        let mut max_metrics: Option<FlightMetrics> = None;
        let mut takeoff_time: Option<String> = None;
        let mut landing_time: Option<String> = None;
        let mut start_time: Option<String> = None;

        let webhook_manager = app.state::<WebhookManager>();
        webhook_manager.reset();

        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        let mut buf = [0u8; 65535];

        loop {
            if !*running.lock().unwrap() { break; }

            match socket.recv_from(&mut buf) {
                Ok((size, _)) => {
                    { let mut c = connected.lock().unwrap(); *c = true; }
                    let data_str = String::from_utf8_lossy(&buf[..size]);
                    if let Ok(data) = serde_json::from_str::<Value>(&data_str) {
                        let mut m = FlightMetrics::default();
                        
                        m.latitude = data["sim/flightmodel/position/latitude"].as_f64().unwrap_or(0.0);
                        m.longitude = data["sim/flightmodel/position/longitude"].as_f64().unwrap_or(0.0);
                        m.gps_altitude_msl = data["sim/flightmodel/position/elevation"].as_f64().unwrap_or(0.0) * 3.28084;
                        m.indicated_airspeed = data["sim/flightmodel/position/indicated_airspeed"].as_f64().unwrap_or(0.0);
                        m.ground_speed = data["sim/flightmodel/position/groundspeed"].as_f64().unwrap_or(0.0) * 1.94384;
                        m.vertical_speed = data["sim/flightmodel/position/vh_ind"].as_f64().unwrap_or(0.0) * 196.85;
                        m.is_on_ground = if data["sim/flightmodel/failures/onground_any"].as_i64().unwrap_or(0) > 0 { 1.0 } else { 0.0 };
                        m.altitude_agl = data["sim/flightmodel/position/y_agl"].as_f64().unwrap_or(0.0) * 3.28084;
                        
                        // Fallback for is_on_ground
                        if m.is_on_ground < 0.5 && m.altitude_agl < 5.0 {
                            m.is_on_ground = 1.0;
                        }

                        m.heading = data["sim/flightmodel/position/mag_psi"].as_f64().unwrap_or(0.0);
                        m.fuel_quantity_left = data["sim/flightmodel/weight/m_fuel1"].as_f64().unwrap_or(0.0) * 0.1498; 
                        m.fuel_quantity_right = data["sim/flightmodel/weight/m_fuel2"].as_f64().unwrap_or(0.0) * 0.1498;
                        m.normal_acceleration = data["sim/flightmodel/forces/g_nrm"].as_f64().unwrap_or(1.0);

                        { let mut metrics_lock = metrics.lock().unwrap(); *metrics_lock = m; }

                        if !flight_ongoing && (m.is_on_ground < 0.5 || m.ground_speed > 10.0) {
                            flight_ongoing = true;
                            { let mut mon = monitoring.lock().unwrap(); *mon = true; }
                            db_conn = None;
                            analyzer.reset();
                            webhook_manager.reset();
                            max_metrics = None;
                            start_time = Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string());

                            let app_data_dir = app.path().app_data_dir().unwrap();
                            let internal_log_dir = app_data_dir.join("flightlogs");
                            let _ = create_dir_all(&internal_log_dir);
                            let filename = format!("butterlog_xp_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
                            let path = internal_log_dir.join(&filename);
                            current_log_path = Some(path.clone());
                            { let mut fid = current_flight_id_mutex.lock().unwrap(); *fid = filename.replace(".db", ""); }

                            if let Ok(conn) = Connection::open(&path) {
                                let _ = init_sqlite_db(&conn);

                                // Set initial departure if on ground
                                if m.is_on_ground > 0.5 {
                                    if let Some(db) = app.try_state::<AirportsDatabase>() {
                                        if let Some(nearest) = db.find_nearest(m.latitude, m.longitude, 1).first() {
                                            crate::append_log(app, format!("[X-Plane] Identified departure: {} ({})", nearest.ident, nearest.name));
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_icao', ?1)", params![nearest.ident]);
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_name', ?1)", params![nearest.name]);
                                        }
                                    }
                                }

                                db_conn = Some(conn);
                                let _ = app.emit("flight-logs-updated", ());
                            }

                            if let Some(title) = data["sim/aircraft/view/acf_title"].as_str() {
                                let title_str = title.to_string();
                                aircraft_info.title = title_str.clone();
                                
                                if let Some(ref conn) = db_conn {
                                    crate::append_log(app, format!("[X-Plane] Set aircraft title: {}", title_str));
                                    let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![title_str.clone()]);
                                }

                                let mut info = aircraft_info_mutex.lock().unwrap();
                                info.title = title_str;
                            }
                            
                            // Process first point immediately
                            let _ = analyzer.update(&m, start_time.as_ref().unwrap());
                        }

                        if flight_ongoing {
                            // Update max metrics
                            match max_metrics {
                                Some(ref mut max_m) => max_m.update_max(&m),
                                None => max_metrics = Some(m),
                            }

                            let now = Utc::now();
                            if now.signed_duration_since(last_log_time) >= chrono::Duration::seconds(1) {
                                last_log_time = now;
                                let now_str = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

                                if m.ground_speed.abs() > 0.1 || m.vertical_speed.abs() > 10.0 {
                                    if let Some(new_phase) = analyzer.update(&m, &now_str) {
                                        let _ = app.emit("flight-phase-change", new_phase);
                                        if new_phase == crate::models::FlightPhase::Takeoff {
                                            takeoff_snapshot = Some(m);
                                            takeoff_time = Some(now_str.clone());
                                        } else if new_phase == crate::models::FlightPhase::Landing {
                                            landing_snapshot = Some(m);
                                            landing_time = Some(now_str.clone());
                                        }
                                    }
                                    if let Some(ref conn) = db_conn { let _ = insert_sqlite_row(conn, &now_str, &m); }
                                }

                                if let Some(db) = app.try_state::<AirportsDatabase>() {
                                    let summary = WebhookFlightSummary {
                                        log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                        airframe_name: aircraft_info.title.clone(),
                                        simulator: "X-Plane".to_string(),
                                        simulator_version: "12".to_string(),
                                        departure: AirportInfo { 
                                            icao: analyzer.find_start_icao(&db), 
                                            name: db.get_by_ident(&analyzer.find_start_icao(&db)).map(|a| a.name.clone()).unwrap_or_default()
                                        },
                                        arrival: AirportInfo { 
                                            icao: analyzer.find_end_icao(&db), 
                                            name: db.get_by_ident(&analyzer.find_end_icao(&db)).map(|a| a.name.clone()).unwrap_or_default()
                                        },
                                        takeoff_time: takeoff_time.clone(),
                                        landing_time: landing_time.clone(),
                                        start_time: start_time.clone(),
                                        end_time: Some(now_str),
                                        takeoff_snapshot,
                                        landing_snapshot,
                                        current_snapshot: Some(m),
                                        max_entries: max_metrics,
                                    };
                                    webhook_manager.sync_flight(app, &summary, db_conn.as_ref(), false);
                                }
                            }
                        }
                    }
                }
                Err(_) => { { let mut c = connected.lock().unwrap(); *c = false; } }
            }
        }

        if flight_ongoing {
            if let Some(db) = app.try_state::<AirportsDatabase>() {
                // Advanced Landing Analysis
                if let Some(r_db) = app.try_state::<RunwaysDatabase>() {
                    analyzer.finalize_landing_performance(&db, &r_db);
                }

                let now_str = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                let summary = WebhookFlightSummary {
                    log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                    airframe_name: aircraft_info.title.clone(),
                    simulator: "X-Plane".to_string(),
                    simulator_version: "12".to_string(),
                    departure: AirportInfo { 
                        icao: analyzer.find_start_icao(&db), 
                        name: db.get_by_ident(&analyzer.find_start_icao(&db)).map(|a| a.name.clone()).unwrap_or_default()
                    },
                    arrival: AirportInfo { 
                        icao: analyzer.find_end_icao(&db), 
                        name: db.get_by_ident(&analyzer.find_end_icao(&db)).map(|a| a.name.clone()).unwrap_or_default()
                    },
                    takeoff_time: takeoff_time.clone(),
                    landing_time: landing_time.clone(),
                    start_time: start_time.clone(),
                    end_time: Some(now_str),
                    takeoff_snapshot,
                    landing_snapshot,
                    current_snapshot: metrics.lock().map(|m| *m).ok(),
                    max_entries: max_metrics,
                };
                webhook_manager.sync_flight(app, &summary, db_conn.as_ref(), true);
                crate::append_log(app, "[X-Plane] Finalized flight sync.".to_string());
            }
        }
        webhook_manager.reset();
        Ok(())
    }
}

impl SimMonitor for XPlaneMonitor {
    fn id(&self) -> &'static str { "xplane" }
    fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running { return Ok(()); }
        *running = true;
        let metrics = self.metrics.clone();
        let aircraft_info = self.aircraft_info.clone();
        let current_flight_id = self.current_flight_id.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();
        let monitoring_clone = self.monitoring.clone();
        thread::spawn(move || {
            let socket = match UdpSocket::bind("0.0.0.0:49005") {
                Ok(s) => s,
                Err(e) => { crate::append_log(&app, format!("[X-Plane] Failed to bind UDP socket: {}", e)); return; }
            };
            let _ = Self::run_monitor(&app, socket, &metrics, &aircraft_info, &current_flight_id, &running_clone, &connected_clone, &monitoring_clone, log_path.as_ref());
        });
        Ok(())
    }
    fn stop(&self) { let mut running = self.running.lock().unwrap(); *running = false; }
    fn get_metrics(&self) -> FlightMetrics { *self.metrics.lock().unwrap() }
    fn get_aircraft_info(&self) -> crate::models::AircraftInfo { self.aircraft_info.lock().unwrap().clone() }
    fn get_current_flight_id(&self) -> String { self.current_flight_id.lock().unwrap().clone() }
    fn is_connected(&self) -> bool { *self.connected.lock().unwrap() }
    fn is_monitoring(&self) -> bool { *self.monitoring.lock().unwrap() }
}
