use crate::airports::AirportsDatabase;
use crate::flight_log_manager::{init_sqlite_db, insert_sqlite_row};
use crate::models::{AircraftInfo, FlightMetrics, WebhookFlightSummary, AirportInfo};
use crate::sim_monitor::{calculate_distance, SimMonitor};
use crate::webhook_manager::WebhookManager;
use crate::runways::RunwaysDatabase;
use chrono::Utc;
use rusqlite::{params, Connection};
use simplesimconnect::*;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub struct SimConnectMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    aircraft_info: Arc<Mutex<AircraftInfo>>,
    current_flight_id: Arc<Mutex<String>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
    monitoring: Arc<Mutex<bool>>,
}

impl SimConnectMonitor {
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
        sc: SimConnect,
        metrics: &Arc<Mutex<FlightMetrics>>,
        aircraft_info_mutex: &Arc<Mutex<AircraftInfo>>,
        current_flight_id_mutex: &Arc<Mutex<String>>,
        running: &Arc<Mutex<bool>>,
        monitoring: &Arc<Mutex<bool>>,
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

        // Register all fields
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
        sc.add_to_data_definition::<f64>(define_id, "RECIP ENG TURBINE INLET TEMPERATURE:1", "farenheit")?;
        sc.add_to_data_definition::<f64>(define_id, "RECIP ENG TURBINE INLET TEMPERATURE:2", "farenheit")?;
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
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT BANK HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT BANK HOLD REF", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT PITCH HOLD REF", "degrees")?;
        sc.add_to_data_definition::<f64>(define_id, "AUTOPILOT VERTICAL HOLD VAR", "feet per minute")?;
        sc.add_to_data_definition::<f64>(define_id, "SIM ON GROUND", "bool")?;
        sc.add_to_data_definition::<f64>(define_id, "PLANE ALT ABOVE GROUND", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "G FORCE", "gforce")?;
        sc.add_to_data_definition::<f64>(define_id, "PRESSURE ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "DENSITY ALTITUDE", "feet")?;
        sc.add_to_data_definition::<f64>(define_id, "PRESSURIZATION CABIN ALTITUDE", "feet")?;

        sc.add_to_data_definition::<f64>(define_id, "G FORCE", "gforce")?; // dummy for prop rpm
        sc.add_to_data_definition::<f64>(define_id, "G FORCE", "gforce")?; // dummy for gear ratio

        sc.add_to_data_definition::<[u8; 256]>(aircraft_define_id, "TITLE", "string256")?;
        sc.request_data_on_sim_object(request_id, define_id, OBJECT_ID_USER, PERIOD_VISUAL_FRAME)?;

        let mut current_log_path: Option<PathBuf> = None;
        let mut db_conn: Option<Connection> = None;
        let mut aircraft_info = AircraftInfo::default();
        let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
        let mut last_log_time = Utc::now();
        let mut flight_ongoing = false;

        let mut takeoff_snapshot: Option<FlightMetrics> = None;
        let mut landing_snapshot: Option<FlightMetrics> = None;
        let mut max_metrics: Option<FlightMetrics> = None;
        let mut takeoff_time: Option<String> = None;
        let mut landing_time: Option<String> = None;
        let mut start_time: Option<String> = None;

        let webhook_manager = app.state::<WebhookManager>();
        webhook_manager.reset();

        {
            let mut m = monitoring.lock().unwrap();
            *m = false;
        }

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
                        crate::append_log(app, format!("[{}] Received SimStart event. Starting new flight log.", Utc::now().format("%H:%M:%S")));
                        flight_ongoing = true;
                        {
                            let mut m = monitoring.lock().unwrap();
                            *m = true;
                        }
                        db_conn = None;
                        analyzer.reset();
                        aircraft_info = AircraftInfo::default();
                        webhook_manager.reset();
                        takeoff_snapshot = None;
                        landing_snapshot = None;
                        max_metrics = None;
                        takeoff_time = None;
                        landing_time = None;
                        start_time = Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string());

                        let _ = sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, PERIOD_VISUAL_FRAME);

                        let app_data_dir = app.path().app_data_dir().unwrap();
                        let internal_log_dir = app_data_dir.join("flightlogs");
                        let _ = create_dir_all(&internal_log_dir);
                        let filename = format!("butterlog_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
                        let path = internal_log_dir.join(&filename);
                        current_log_path = Some(path.clone());
                        {
                            let mut fid = current_flight_id_mutex.lock().unwrap();
                            *fid = filename.replace(".db", "");
                        }

                        if let Ok(conn) = Connection::open(&path) {
                            if let Err(e) = init_sqlite_db(&conn) {
                                crate::append_log(app, format!("[MSFS] Error initializing DB: {}", e));
                            }

                            // Set initial departure if on ground
                            let m_lock = metrics.lock().unwrap();
                            if m_lock.is_on_ground > 0.5 || m_lock.altitude_agl < 10.0 {
                                if let Some(db) = app.try_state::<AirportsDatabase>() {
                                    if let Some(nearest) = db.find_nearest(m_lock.latitude, m_lock.longitude, 1).first() {
                                        crate::append_log(app, format!("[MSFS] Identified departure: {} ({})", nearest.ident, nearest.name));
                                        if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_icao', ?1)", params![nearest.ident]) {
                                            crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                        }
                                        if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_name', ?1)", params![nearest.name]) {
                                            crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                        }
                                    }
                                }
                            }

                            // Set aircraft title if already known
                            if !aircraft_info.title.is_empty() {
                                crate::append_log(app, format!("[MSFS] Set aircraft title: {}", aircraft_info.title));
                                if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![aircraft_info.title]) {
                                    crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                }
                            }

                            db_conn = Some(conn);
                            let _ = app.emit("flight-logs-updated", ());
                        }
                    } else if event.event_id == event_sim_stop {
                        crate::append_log(app, format!("[{}] Received SimStop event. Closing and analyzing flight log.", Utc::now().format("%H:%M:%S")));
                        flight_ongoing = false;
                        {
                            let mut m = monitoring.lock().unwrap();
                            *m = false;
                        }

                        if let Some(ref conn) = db_conn {
                            if let Some(db) = app.try_state::<AirportsDatabase>() {
                                // Advanced Landing Analysis
                                if let Some(r_db) = app.try_state::<RunwaysDatabase>() {
                                    analyzer.finalize_landing_performance(&db, &r_db);
                                }

                                let start_icao = analyzer.find_start_icao(&db);
                                let end_icao = analyzer.find_end_icao(&db);
                                
                                // Final Webhook Sync
                                let summary = WebhookFlightSummary {
                                    log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                    airframe_name: aircraft_info.title.clone(),
                                    simulator: "MSFS".to_string(),
                                    simulator_version: "SimConnect".to_string(),
                                    departure: AirportInfo { icao: start_icao.clone(), name: "".to_string() },
                                    arrival: AirportInfo { icao: end_icao.clone(), name: "".to_string() },
                                    takeoff_time: takeoff_time.clone(),
                                    landing_time: landing_time.clone(),
                                    start_time: start_time.clone(),
                                    end_time: Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
                                    takeoff_snapshot,
                                    landing_snapshot,
                                    current_snapshot: metrics.lock().map(|m| *m).ok(),
                                    max_entries: max_metrics,
                                };
                                webhook_manager.sync_flight(app, &summary, db_conn.as_ref(), true);
                                webhook_manager.reset();

                                let start_name = db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());
                                let end_name = db.get_by_ident(&end_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string());

                                let mut summary_data = vec![
                                    ("departure_icao", start_icao.clone()),
                                    ("departure_name", start_name),
                                    ("arrival_icao", end_icao.clone()),
                                    ("arrival_name", end_name),
                                    ("aircraft_title", aircraft_info.title.clone()),
                                    ("max_altitude", analyzer.max_alt.to_string()),
                                    ("max_ground_speed", analyzer.max_gs.to_string()),
                                    ("fuel_consumed", (analyzer.initial_fuel - analyzer.final_fuel).to_string()),
                                ];

                                if let Some(landing) = analyzer.events.iter().find(|e| e.event_type == "landing") {
                                    if let Some(v) = landing.touchdown_fpm { summary_data.push(("touchdown_fpm", v.to_string())); }
                                    if let Some(v) = landing.landing_g { summary_data.push(("landing_g", v.to_string())); }
                                    if let Some(v) = landing.offset_percent { summary_data.push(("landing_offset_pct", v.to_string())); }
                                    if let Some(v) = landing.threshold_dist_ft { summary_data.push(("landing_dist_ft", v.to_string())); }
                                }

                                for (k, v) in summary_data {
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params![k, v]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                }
                                crate::append_log(app, "[MSFS] Saved final flight summary to database.".to_string());

                                if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["flight_events", events_json]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                }

                                let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
                                let duration_mins = analyzer.get_duration_minutes();
                                if let Err(e) = crate::flight_log_manager::update_aircraft_stats(app, &aircraft_info.title, duration_mins as f64, fuel_consumed, &end_icao, true) {
                                    crate::append_log(app, format!("[MSFS] Error updating aircraft stats: {}", e));
                                }

                                drop(db_conn.take());
                                let _ = app.emit("flight-logs-updated", ());
                            }
                        }
                        db_conn = None;
                    }
                }

                if msg.request_id() == Some(aircraft_request_id) {
                    if let Some(data) = msg.as_sim_object_data::<[u8; 256]>() {
                        let s = String::from_utf8_lossy(data);
                        let title = s.split('\0').next().unwrap_or("").trim().to_string();
                        aircraft_info.title = title.clone();
                        
                        if let Some(ref conn) = db_conn {
                            crate::append_log(app, format!("[MSFS] Set aircraft title: {}", title));
                            if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![title.clone()]) {
                                crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                            }
                        }

                        let mut info = aircraft_info_mutex.lock().unwrap();
                        info.title = title;
                    }
                }

                if let Some(data_ref) = msg.as_sim_object_data::<FlightMetrics>() {
                    let mut data_val = *data_ref;
                    //println!("Received data: {:?}", serde_json::to_string(&data_val).unwrap_or_else(|_| "Failed to serialize".into()) );
                    let data = &data_val;

                    if msg.request_id() == Some(request_id) {
                        {
                            let mut m = metrics.lock().unwrap();
                            *m = *data;
                        }

                        if !flight_ongoing && (data.is_on_ground < 0.5) {
                            flight_ongoing = true;
                            { let mut m = monitoring.lock().unwrap(); *m = true; }
                            db_conn = None;
                            analyzer.reset();
                            aircraft_info = AircraftInfo::default();
                            webhook_manager.reset();
                            max_metrics = None;
                            start_time = Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string());

                            let _ = sc.request_data_on_sim_object(aircraft_request_id, aircraft_define_id, OBJECT_ID_USER, PERIOD_VISUAL_FRAME);
                            let app_data_dir = app.path().app_data_dir().unwrap();
                            let internal_log_dir = app_data_dir.join("flightlogs");
                            let _ = create_dir_all(&internal_log_dir);
                            let filename = format!("butterlog_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
                            let path = internal_log_dir.join(&filename);
                            current_log_path = Some(path.clone());
                            if let Ok(conn) = Connection::open(&path) {
                                if let Err(e) = init_sqlite_db(&conn) {
                                    crate::append_log(app, format!("[MSFS] Error initializing DB: {}", e));
                                }

                                // Set initial departure if on ground
                                if data.is_on_ground > 0.5 || data.altitude_agl < 10.0 {
                                    if let Some(db) = app.try_state::<AirportsDatabase>() {
                                        if let Some(nearest) = db.find_nearest(data.latitude, data.longitude, 1).first() {
                                            if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_icao', ?1)", params![nearest.ident]) {
                                                crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                            }
                                            if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_name', ?1)", params![nearest.name]) {
                                                crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                            }
                                        }
                                    }
                                }

                                // Set aircraft title if already known
                                if !aircraft_info.title.is_empty() {
                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![aircraft_info.title]) {
                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                    }
                                }

                                db_conn = Some(conn);
                                let _ = app.emit("flight-logs-updated", ());
                            }
                            
                            // Process first point immediately
                            let _ = analyzer.update(data, start_time.as_ref().unwrap());
                        }

                        if flight_ongoing {
                            // Update max metrics
                            match max_metrics {
                                Some(ref mut m) => m.update_max(data),
                                None => max_metrics = Some(*data),
                            }

                            let now = Utc::now();
                            let mut sample_rate_ms = 1000;
                            if data.is_on_ground < 0.5 {
                                if let Some(db) = app.try_state::<AirportsDatabase>() {
                                    if let Some(nearest) = db.find_nearest(data.latitude, data.longitude, 1).first() {
                                        let dist = calculate_distance(data.latitude, data.longitude, nearest.latitude_deg.unwrap_or(0.0), nearest.longitude_deg.unwrap_or(0.0));
                                        if dist <= 5.0 && (data.gps_altitude_msl - nearest.elevation_ft.unwrap_or(0) as f64) <= 500.0 {
                                            sample_rate_ms = 200;
                                        }
                                    }
                                }
                            }

                            if now.signed_duration_since(last_log_time) >= chrono::Duration::milliseconds(sample_rate_ms) {
                                last_log_time = now;
                                let now_str = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                                
                                if data.ground_speed.abs() > 0.1 || data.vertical_speed.abs() > 10.0 {
                                    if let Some(new_phase) = analyzer.update(data, &now_str) {
                                        let _ = app.emit("flight-phase-change", new_phase);
                                        if new_phase == crate::models::FlightPhase::Takeoff {
                                            takeoff_snapshot = Some(*data);
                                            takeoff_time = Some(now_str.clone());
                                        } else if new_phase == crate::models::FlightPhase::Landing {
                                            landing_snapshot = Some(*data);
                                            landing_time = Some(now_str.clone());

                                            // Immediate Arrival detection
                                            if let Some(ref takeoff_ts) = analyzer.takeoff_timestamp {
                                                if let (Some(t_start), Some(t_end)) = (analyzer.parse_timestamp(takeoff_ts), analyzer.parse_timestamp(&now_str)) {
                                                    if t_end - t_start > 60 {
                                                        if let Some(db) = app.try_state::<AirportsDatabase>() {
                                                            if let Some(nearest) = db.find_nearest(data.latitude, data.longitude, 1).first() {
                                                                if let Some(ref conn) = db_conn {
                                                                    crate::append_log(app, format!("[MSFS] Identified arrival: {} ({})", nearest.ident, nearest.name));
                                                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_icao', ?1)", params![nearest.ident]) {
                                                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                                                    }
                                                                    if let Err(e) = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_name', ?1)", params![nearest.name]) {
                                                                        crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if let Some(ref conn) = db_conn {
                                        if let Err(e) = insert_sqlite_row(conn, &now_str, data) {
                                            crate::append_log(app, format!("[MSFS] Error writing to DB: {}", e));
                                        }
                                    }
                                }

                                // Sync Webhook
                                if let Some(db) = app.try_state::<AirportsDatabase>() {
                                    let summary = WebhookFlightSummary {
                                        log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                        airframe_name: aircraft_info.title.clone(),
                                        simulator: "MSFS".to_string(),
                                        simulator_version: "SimConnect".to_string(),
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
                                        current_snapshot: Some(*data),
                                        max_entries: max_metrics,
                                    };
                                    webhook_manager.sync_flight(app, &summary, db_conn.as_ref(), false);
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
    fn id(&self) -> &'static str { "msfs" }
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
        thread::spawn(move || loop {
            if !*running_clone.lock().unwrap() { break; }
            match SimConnect::open("ButterLogV2") {
                Ok(sc) => {
                    crate::append_log(&app, format!("[{}] Successfully connected to MSFS.", Utc::now().format("%Y-%m-%d %H:%M:%S")));
                    { let mut connected = connected_clone.lock().unwrap(); *connected = true; }
                    let _ = Self::run_monitor(&app, sc, &metrics, &aircraft_info, &current_flight_id, &running_clone, &monitoring_clone, log_path.as_ref());
                    { let mut connected = connected_clone.lock().unwrap(); *connected = false; }
                    { let mut monitoring = monitoring_clone.lock().unwrap(); *monitoring = false; }
                }
                Err(_) => {}
            }
            thread::sleep(Duration::from_secs(1));
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
