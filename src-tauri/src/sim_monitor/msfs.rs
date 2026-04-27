use simplesimconnect::*;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use chrono::{Local};
use std::fs::{create_dir_all};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Emitter};
use rusqlite::{params, Connection};
use crate::models::{FlightMetrics, AircraftInfo};
use crate::sim_monitor::{SimMonitor, calculate_distance};
use crate::flight_log_manager::{init_sqlite_db, insert_sqlite_row};
use crate::airports::AirportsDatabase;

pub struct SimConnectMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
}

impl SimConnectMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(FlightMetrics::default())),
            running: Arc::new(Mutex::new(false)),
            connected: Arc::new(Mutex::new(false)),
        }
    }

    fn run_monitor(
        app: &AppHandle,
        sc: SimConnect,
        metrics: &Arc<Mutex<FlightMetrics>>,
        running: &Arc<Mutex<bool>>,
        _requested_log_path: Option<&PathBuf>,
    ) -> anyhow::Result<()> {
        let define_id = 1;
        let aircraft_define_id = 2;
        let request_id = 1;
        let aircraft_request_id = 2;
        let event_sim_start = 1;
        let event_sim_stop = 2;

        sc.subscribe_to_system_event(event_sim_start, "SimStart")?;
        sc.subscribe_to_system_event(event_sim_stop, "SimStop")?;

        // Register all fields in the exact order they appear in FlightMetrics struct (excluding X-Plane specific ones)
        sc.add_to_data_definition::<f64>(define_id, "PLANE LATITUDE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE LONGITUDE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "INDICATED ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "KOHLSMAN SETTING HG", "inHg")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "AMBIENT TEMPERATURE", "celsius")?;
        sc.add_to_data_definition::<f64>(define_id, "AIRSPEED INDICATED", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "GROUND VELOCITY", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "VERTICAL SPEED", "feet per minute")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE PITCH DEGREES", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE BANK DEGREES", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "ACCELERATION BODY X", "G force")?;
        sc.add_to_data_definition::<f64>(define_id, "ACCELERATION BODY Z", "G force")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE HEADING DEGREES TRUE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS GROUND TRUE TRACK", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "ELECTRICAL MAIN BUS VOLTAGE:1", "volts")?;
        sc.add_to_data_definition::<f64>(define_id, "ELECTRICAL MAIN BUS VOLTAGE:2", "volts")?;
        sc.add_to_data_definition::<f64>(define_id, "ELECTRICAL MAIN BUS AMPS:1", "amps")?;
        sc.add_to_data_definition::<f64>(define_id, "FUEL LEFT QUANTITY", "gallons")?;
        sc.add_to_data_definition::<f64>(define_id, "FUEL RIGHT QUANTITY", "gallons")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG FUEL FLOW GPH:1", "gallons per hour")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG OIL TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG OIL PRESSURE:1", "psi")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG MANIFOLD PRESSURE:1", "inHg")?;
        sc.add_to_data_definition::<f64>(define_id, "GENERAL ENG RPM:1", "rpm")?;
        sc.add_to_data_definition::<f64>(define_id, "GENERAL ENG PCT MAX RPM:1", "percent")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:2", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:3", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:4", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:5", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG CYLINDER HEAD TEMPERATURE:6", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:2", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:3", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:4", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:5", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG EXHAUST GAS TEMPERATURE:6", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG TURBINE INLET TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "ENG TURBINE INLET TEMPERATURE:2", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS POSITION ALT", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "AIRSPEED TRUE", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS DRIVES NAV1", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV OBS:1", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV ACTIVE FREQUENCY:1", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV ACTIVE FREQUENCY:2", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "COM ACTIVE FREQUENCY:1", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "COM ACTIVE FREQUENCY:2", "MHz")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV CDI:1", "number")?;
        sc.add_to_data_definition::<f64>(define_id, "NAV GSI:1", "number")?;
        sc.add_to_data_definition::<f64>(define_id, "AMBIENT WIND VELOCITY", "knots")?;
        sc.add_to_data_definition::<f64>(define_id, "AMBIENT WIND DIRECTION", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "nautical miles")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS WP BEARING", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "MAGVAR", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT MASTER", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT ROLL HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT BANK HOLD ANGLE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD ANGLE", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT VERTICAL HOLD VAR", "feet per minute")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS FIX TYPE", "enum")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS HORIZONTAL ERROR", "meters")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS VERTICAL ERROR", "meters")?;
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "meters")?; // Placeholder for HPLwas
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "meters")?; // Placeholder for HPLfd
        sc.add_to_data_definition::<f64>(define_id, "GPS WP DISTANCE", "meters")?; // Placeholder for VPLwas
        sc.add_to_data_definition::<f64>(define_id, "SIM ON GROUND", "bool")?;

        // Aircraft info definitions
        sc.add_to_data_definition::<[u8; 256]>(aircraft_define_id, "TITLE", "")?;
        sc.add_to_data_definition::<[u8; 256]>(aircraft_define_id, "ATC TYPE", "")?;
        sc.add_to_data_definition::<[u8; 256]>(aircraft_define_id, "ATC MODEL", "")?;

        sc.request_data_on_sim_object(
            request_id,
            define_id,
            OBJECT_ID_USER,
            PERIOD_VISUAL_FRAME,
        )?;

        let mut current_log_path: Option<PathBuf> = None;
        let mut db_conn: Option<Connection> = None;
        let mut aircraft_info = AircraftInfo::default();

        let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
        let mut last_log_time = Local::now();
        let mut flight_ongoing = false;

        loop {
            if !*running.lock().unwrap() {
                break;
            }

            while let Some(msg) = sc.get_next_dispatch()? {
                if msg.is_quit() {
                    return Ok(());
                }

                if let Some(event) = msg.as_event() {
                    if event.event_id == event_sim_start {
                        crate::append_log(app, format!("[{}] Received SimStart event. Starting new flight log (SQLite).", Local::now().format("%H:%M:%S")));
                        flight_ongoing = true;
                        
                        db_conn = None;
                        analyzer.reset();
                        aircraft_info = AircraftInfo::default();

                        if let Err(e) = sc.request_data_on_sim_object(
                            aircraft_request_id,
                            aircraft_define_id,
                            OBJECT_ID_USER,
                            PERIOD_VISUAL_FRAME,
                        ) {
                            crate::append_log(app, format!("Failed to request aircraft info: {}", e));
                        }

                        let app_data_dir = app.path().app_data_dir().unwrap();
                        let internal_log_dir = app_data_dir.join("flightlogs");
                        create_dir_all(&internal_log_dir)?;
                        let filename = format!("butterlog_{}.db", Local::now().format("%Y%m%d_%H%M%S"));
                        let path = internal_log_dir.join(filename);
                        current_log_path = Some(path.clone());
                        
                        match Connection::open(&path) {
                            Ok(conn) => {
                                if let Err(e) = init_sqlite_db(&conn) {
                                    crate::append_log(app, format!("Failed to initialize SQLite DB: {}", e));
                                } else {
                                    db_conn = Some(conn);
                                    crate::append_log(app, format!("New internal flight log created at: {:?}", path));
                                }
                            }
                            Err(e) => {
                                crate::append_log(app, format!("Failed to create new SQLite log file: {}", e));
                            }
                        }
                    } else if event.event_id == event_sim_stop {
                        crate::append_log(app, format!("[{}] Received SimStop event. Closing and analyzing flight log.", Local::now().format("%H:%M:%S")));
                        flight_ongoing = false;
                        
                        if let Some(ref conn) = db_conn {
                             if let Some(db) = app.try_state::<AirportsDatabase>() {
                                let start_icao = analyzer.find_start_icao(&db);
                                let end_icao = analyzer.find_end_icao(&db);
                                let start_name = db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                let end_name = db.get_by_ident(&end_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["departure_icao", start_icao]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["departure_name", start_name]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["arrival_icao", end_icao]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["arrival_name", end_name]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["aircraft_title", aircraft_info.title]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["aircraft_type", aircraft_info.atc_type]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["aircraft_model", aircraft_info.atc_model]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["max_altitude", analyzer.max_alt.to_string()]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["max_ground_speed", analyzer.max_gs.to_string()]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["fuel_consumed", (analyzer.initial_fuel - analyzer.final_fuel).to_string()]);
                                if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                    let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["flight_events", events_json]);
                                }

                                drop(db_conn.take());
                                let _ = app.emit("flight-logs-updated", ());
                             }
                        }
                        
                        db_conn = None;
                    }
                }

                if msg.request_id() == Some(aircraft_request_id) {
                    if let Some(data) = msg.as_sim_object_data::<[u8; 768]>() {
                        aircraft_info.title = String::from_utf8_lossy(&data[0..256]).trim_matches(char::from(0)).trim().to_string();
                        aircraft_info.atc_type = String::from_utf8_lossy(&data[256..512]).trim_matches(char::from(0)).trim().to_string();
                        aircraft_info.atc_model = String::from_utf8_lossy(&data[512..768]).trim_matches(char::from(0)).trim().to_string();
                    }
                }

                if let Some(data) = msg.as_sim_object_data::<FlightMetrics>() {
                    if msg.request_id() == Some(request_id) {
                        let mut m = metrics.lock().unwrap();
                        *m = *data;

                        // Check if a flight is ongoing but we haven't started logging (e.g. app started mid-flight)
                        if !flight_ongoing && (data.is_on_ground < 0.5 || data.ground_speed > 10.0) {
                            crate::append_log(app, format!("[{}] Detected ongoing flight on startup. Starting log.", Local::now().format("%H:%M:%S")));
                            flight_ongoing = true;
                            db_conn = None;
                            analyzer.reset();
                            aircraft_info = AircraftInfo::default();

                            let _ = sc.request_data_on_sim_object(
                                aircraft_request_id,
                                aircraft_define_id,
                                OBJECT_ID_USER,
                                PERIOD_VISUAL_FRAME,
                            );

                            let app_data_dir = app.path().app_data_dir().unwrap();
                            let internal_log_dir = app_data_dir.join("flightlogs");
                            let _ = create_dir_all(&internal_log_dir);
                            let filename = format!("butterlog_{}.db", Local::now().format("%Y%m%d_%H%M%S"));
                            let path = internal_log_dir.join(filename);
                            current_log_path = Some(path.clone());
                            
                            if let Ok(conn) = Connection::open(&path) {
                                let _ = init_sqlite_db(&conn);
                                db_conn = Some(conn);
                            }
                        }

                        if flight_ongoing {
                            let now = Local::now();
                            
                            // Determine sample rate based on proximity and altitude
                            let mut sample_rate_ms = 1000;
                            if data.is_on_ground < 0.5 {
                                if let Some(db) = app.try_state::<AirportsDatabase>() {
                                    if let Some(nearest) = db.find_nearest(data.latitude, data.longitude, 1).first() {
                                        let dist = calculate_distance(data.latitude, data.longitude, nearest.latitude_deg.unwrap_or(0.0), nearest.longitude_deg.unwrap_or(0.0));
                                        let elevation = nearest.elevation_ft.unwrap_or(0) as f64;
                                        let agl = data.gps_altitude_msl - elevation;
                                        
                                        if dist <= 5.0 && agl <= 500.0 {
                                            sample_rate_ms = 200; // 5Hz
                                        }
                                    }
                                }
                            }

                            if now.signed_duration_since(last_log_time) >= chrono::Duration::milliseconds(sample_rate_ms) {
                                last_log_time = now;
                                
                                let has_movement = data.ground_speed.abs() > 0.1 || data.vertical_speed.abs() > 10.0;
                                
                                if has_movement {
                                    let now_str = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                                    if let Some(new_phase) = analyzer.update(data, &now_str) {
                                        let _ = app.emit("flight-phase-change", new_phase);

                                        if new_phase == crate::models::FlightPhase::Takeoff {
                                            if let (Some(ref conn), Some(db)) = (&db_conn, app.try_state::<AirportsDatabase>()) {
                                                let start_icao = analyzer.find_start_icao(&db);
                                                let start_name = db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["departure_icao", start_icao]);
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["departure_name", start_name]);
                                            }
                                        }
                                    }

                                    if let Some(ref conn) = db_conn {
                                        if let Err(e) = insert_sqlite_row(conn, &now_str, data) {
                                            crate::append_log(app, format!("Failed to insert SQLite row: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            thread::sleep(Duration::from_millis(50));
        }

        Ok(())
    }
}

impl SimMonitor for SimConnectMonitor {
    fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running {
            return Ok(());
        }
        *running = true;

        let metrics = self.metrics.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();

        thread::spawn(move || {
            loop {
                if !*running_clone.lock().unwrap() {
                    break;
                }

                match SimConnect::open("ButterLogV2") {
                    Ok(sc) => {
                        crate::append_log(&app, format!("[{}] Successfully connected to MSFS (SimConnect).", Local::now().format("%Y-%m-%d %H:%M:%S")));
                        {
                            let mut connected = connected_clone.lock().unwrap();
                            *connected = true;
                        }

                        if let Err(e) = Self::run_monitor(&app, sc, &metrics, &running_clone, log_path.as_ref()) {
                            crate::append_log(&app, format!("[{}] SimConnect monitor error: {}", Local::now().format("%Y-%m-%d %H:%M:%S"), e));
                        }

                        {
                            let mut connected = connected_clone.lock().unwrap();
                            *connected = false;
                        }
                    }
                    Err(_) => {}
                }

                thread::sleep(Duration::from_secs(1));
            }
        });

        Ok(())
    }

    fn stop(&self) {
        let mut running = self.running.lock().unwrap();
        *running = false;
    }

    fn get_metrics(&self) -> FlightMetrics {
        *self.metrics.lock().unwrap()
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
}
