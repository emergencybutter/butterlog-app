use simplesimconnect::*;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use chrono::{Local, DateTime};
use std::fs::{create_dir_all};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Emitter};
use rusqlite::{params, Connection};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct FlightMetrics {
    pub latitude: f64,
    pub longitude: f64,
    pub indicated_altitude: f64,
    pub altimeter_setting: f64,
    pub gps_altitude_msl: f64,
    pub outside_air_temp: f64,
    pub indicated_airspeed: f64,
    pub ground_speed: f64,
    pub vertical_speed: f64,
    pub pitch_angle: f64,
    pub roll_angle: f64,
    pub lateral_acceleration: f64,
    pub normal_acceleration: f64,
    pub heading: f64,
    pub track: f64,
    pub volts_1: f64,
    pub volts_2: f64,
    pub amps_1: f64,
    pub fuel_quantity_left: f64,
    pub fuel_quantity_right: f64,
    pub engine_1_fuel_flow: f64,
    pub engine_1_oil_temp: f64,
    pub engine_1_oil_pressure: f64,
    pub engine_1_manifold_pressure: f64,
    pub engine_1_rpm: f64,
    pub engine_1_percent_power: f64,
    pub engine_1_cht_1: f64,
    pub engine_1_cht_2: f64,
    pub engine_1_cht_3: f64,
    pub engine_1_cht_4: f64,
    pub engine_1_cht_5: f64,
    pub engine_1_cht_6: f64,
    pub engine_1_egt_1: f64,
    pub engine_1_egt_2: f64,
    pub engine_1_egt_3: f64,
    pub engine_1_egt_4: f64,
    pub engine_1_egt_5: f64,
    pub engine_1_egt_6: f64,
    pub engine_1_tit_1: f64,
    pub engine_1_tit_2: f64,
    pub gps_altitude_wgs84: f64,
    pub true_airspeed: f64,
    pub hsi_source: f64,
    pub selected_course: f64,
    pub nav_1_frequency: f64,
    pub nav_2_frequency: f64,
    pub com_1_frequency: f64,
    pub com_2_frequency: f64,
    pub horizontal_cdi: f64,
    pub vertical_cdi: f64,
    pub wind_speed: f64,
    pub wind_direction: f64,
    pub waypoint_distance: f64,
    pub waypoint_bearing: f64,
    pub magnetic_variation: f64,
    pub autopilot_active: f64,
    pub roll_mode: f64,
    pub pitch_mode: f64,
    pub roll_command: f64,
    pub pitch_command: f64,
    pub vertical_speed_target: f64,
    pub gps_fix_type: f64,
    pub horizontal_alarm_limit: f64,
    pub vertical_alarm_limit: f64,
    pub horizontal_protection_level_waas: f64,
    pub horizontal_protection_level_fd: f64,
    pub vertical_protection_level_waas: f64,
    pub is_on_ground: f64,
}

#[derive(Debug, Clone, Default)]
pub struct AircraftInfo {
    pub title: String,
    pub atc_type: String,
    pub atc_model: String,
}

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

    pub fn start(&self, app: AppHandle, log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running {
            return Ok(());
        }
        *running = true;

        let metrics = self.metrics.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();

        thread::spawn(move || {
            let app_data_dir = app.path().app_data_dir().unwrap();
            let internal_log_dir = app_data_dir.join("flightlogs");
            crate::append_log(&app, format!("[{}] SimConnect monitor thread started. Internal log directory: {:?}", Local::now().format("%Y-%m-%d %H:%M:%S"), internal_log_dir));
            loop {
                if !*running_clone.lock().unwrap() {
                    break;
                }

                match SimConnect::open("ButterLogV2") {
                    Ok(sc) => {
                        crate::append_log(&app, format!("[{}] Successfully connected to SimConnect.", Local::now().format("%Y-%m-%d %H:%M:%S")));
                        {
                            let mut connected = connected_clone.lock().unwrap();
                            *connected = true;
                        }

                        if let Err(e) = Self::run_monitor(&app, sc, &metrics, &running_clone, log_path.as_ref()) {
                            crate::append_log(&app, format!("[{}] Monitor error: {}", Local::now().format("%Y-%m-%d %H:%M:%S"), e));
                        }

                        {
                            let mut connected = connected_clone.lock().unwrap();
                            *connected = false;
                        }
                        crate::append_log(&app, format!("[{}] Disconnected from SimConnect.", Local::now().format("%Y-%m-%d %H:%M:%S")));
                    }
                    Err(_) => {
                        // Silently retry every second if SimConnect is not available
                    }
                }

                thread::sleep(Duration::from_secs(1));
            }
            crate::append_log(&app, format!("[{}] SimConnect monitor thread exiting.", Local::now().format("%Y-%m-%d %H:%M:%S")));
        });

        Ok(())
    }

    pub fn stop(&self) {
        let mut running = self.running.lock().unwrap();
        *running = false;
    }

    pub fn get_metrics(&self) -> FlightMetrics {
        *self.metrics.lock().unwrap()
    }

    pub fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
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

        // Register all fields in the exact order they appear in FlightMetrics struct
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
                        
                        // Close existing connection if any
                        db_conn = None;
                        analyzer.reset();
                        aircraft_info = AircraftInfo::default();

                        // Request aircraft info
                        if let Err(e) = sc.request_data_on_sim_object(
                            aircraft_request_id,
                            aircraft_define_id,
                            OBJECT_ID_USER,
                            PERIOD_VISUAL_FRAME,
                        ) {
                            crate::append_log(app, format!("Failed to request aircraft info: {}", e));
                        }

                        // Create new log file in internal directory
                        let app_data_dir = app.path().app_data_dir().unwrap();
                        let internal_log_dir = app_data_dir.join("flightlogs");
                        create_dir_all(&internal_log_dir)?;
                        let filename = format!("butterlog_{}.db", Local::now().format("%Y%m%d_%H%M%S"));
                        let path = internal_log_dir.join(filename);
                        current_log_path = Some(path.clone());
                        
                        match Connection::open(&path) {
                            Ok(conn) => {
                                if let Err(e) = Self::init_sqlite_db(&conn) {
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
                        
                        // Perform analysis and populate summary BEFORE closing connection
                        if let Some(ref conn) = db_conn {
                             if let Some(db) = app.try_state::<crate::airports::AirportsDatabase>() {
                                let start_icao = analyzer.find_start_icao(&db);
                                let end_icao = analyzer.find_end_icao(&db);
                                
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["departure_icao", start_icao]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["arrival_icao", end_icao]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["aircraft_title", aircraft_info.title]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["aircraft_type", aircraft_info.atc_type]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["aircraft_model", aircraft_info.atc_model]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["max_altitude", analyzer.max_alt.to_string()]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["max_ground_speed", analyzer.max_gs.to_string()]);
                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["fuel_consumed", (analyzer.initial_fuel - analyzer.final_fuel).to_string()]);
                                if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                    let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["flight_events", events_json]);
                                }

                                // Rename the file with ICAOs
                                if let Some(path) = current_log_path.take() {
                                    if let Some(old_filename) = path.file_name().and_then(|f| f.to_str()) {
                                        let new_filename = old_filename.replace("butterlog_", &format!("butterlog_{}_{}_", start_icao, end_icao));
                                        let new_path = path.with_file_name(new_filename);
                                        
                                        // We need to close the connection before renaming on Windows
                                        drop(db_conn.take());
                                        
                                        match std::fs::rename(&path, &new_path) {
                                            Ok(_) => {
                                                crate::append_log(app, format!("Flight log renamed to: {:?}", new_path.file_name().unwrap()));
                                                let _ = app.emit("flight-logs-updated", ());
                                            }
                                            Err(e) => {
                                                crate::append_log(app, format!("Failed to rename log file: {}", e));
                                            }
                                        }
                                    }
                                }
                             }
                        }
                        
                        db_conn = None;
                    }
                }

                if msg.request_id() == Some(aircraft_request_id) {
                    if let Some(data) = msg.as_sim_object_data::<[u8; 768]>() { // 3 * 256
                        aircraft_info.title = String::from_utf8_lossy(&data[0..256]).trim_matches(char::from(0)).trim().to_string();
                        aircraft_info.atc_type = String::from_utf8_lossy(&data[256..512]).trim_matches(char::from(0)).trim().to_string();
                        aircraft_info.atc_model = String::from_utf8_lossy(&data[512..768]).trim_matches(char::from(0)).trim().to_string();
                        crate::append_log(app, format!("Aircraft info received: {} ({} {})", aircraft_info.title, aircraft_info.atc_type, aircraft_info.atc_model));
                    }
                }

                if let Some(data) = msg.as_sim_object_data::<FlightMetrics>() {
                    if msg.request_id() == Some(request_id) {
                        let mut m = metrics.lock().unwrap();
                        *m = *data;

                        // Log to SQLite every second ONLY if flight is ongoing and there is movement
                        if flight_ongoing {
                            let now = Local::now();
                            if now.signed_duration_since(last_log_time) >= chrono::Duration::seconds(1) {
                                last_log_time = now;
                                
                                let has_movement = data.ground_speed.abs() > 0.1 || data.vertical_speed.abs() > 10.0;
                                
                                if has_movement {
                                    if let Some(new_phase) = analyzer.update(data) {
                                        crate::append_log(app, format!("[{}] Flight phase changed to: {:?}", now.format("%H:%M:%S"), new_phase));
                                        let _ = app.emit("flight-phase-change", new_phase);

                                        // When taking off, determine departure airport and save to summary
                                        if new_phase == crate::flight_analyzer::FlightPhase::Takeoff {
                                            if let (Some(ref conn), Some(db)) = (&db_conn, app.try_state::<crate::airports::AirportsDatabase>()) {
                                                let start_icao = analyzer.find_start_icao(&db);
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["departure_icao", start_icao]);
                                                crate::append_log(app, format!("Takeoff detected. Departure airport set to: {}", start_icao));
                                            }
                                        }
                                    }

                                    if let Some(ref conn) = db_conn {
                                        if let Err(e) = Self::insert_sqlite_row(conn, &now, data) {
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

    fn init_sqlite_db(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS metrics (
                timestamp TEXT PRIMARY KEY,
                latitude REAL, longitude REAL, 
                indicated_altitude REAL, altimeter_setting REAL, gps_altitude_msl REAL, outside_air_temp REAL,
                indicated_airspeed REAL, ground_speed REAL, vertical_speed REAL, pitch_angle REAL, roll_angle REAL, 
                lateral_acceleration REAL, normal_acceleration REAL,
                heading REAL, track REAL, volts_1 REAL, volts_2 REAL, amps_1 REAL, 
                fuel_quantity_left REAL, fuel_quantity_right REAL,
                engine_1_fuel_flow REAL, engine_1_oil_temp REAL, engine_1_oil_pressure REAL, 
                engine_1_manifold_pressure REAL, engine_1_rpm REAL, engine_1_percent_power REAL,
                engine_1_cht_1 REAL, engine_1_cht_2 REAL, engine_1_cht_3 REAL, engine_1_cht_4 REAL, engine_1_cht_5 REAL, engine_1_cht_6 REAL,
                engine_1_egt_1 REAL, engine_1_egt_2 REAL, engine_1_egt_3 REAL, engine_1_egt_4 REAL, engine_1_egt_5 REAL, engine_1_egt_6 REAL,
                engine_1_tit_1 REAL, engine_1_tit_2 REAL, 
                gps_altitude_wgs84 REAL, true_airspeed REAL, hsi_source REAL, selected_course REAL, 
                nav_1_frequency REAL, nav_2_frequency REAL, com_1_frequency REAL, com_2_frequency REAL, 
                horizontal_cdi REAL, vertical_cdi REAL, wind_speed REAL, wind_direction REAL,
                waypoint_distance REAL, waypoint_bearing REAL, magnetic_variation REAL, 
                autopilot_active REAL, roll_mode REAL, pitch_mode REAL,
                roll_command REAL, pitch_command REAL, vertical_speed_target REAL, 
                gps_fix_type REAL, horizontal_alarm_limit REAL, vertical_alarm_limit REAL,
                horizontal_protection_level_waas REAL, horizontal_protection_level_fd REAL, vertical_protection_level_waas REAL, 
                is_on_ground REAL
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS summary (
                key TEXT PRIMARY KEY,
                value TEXT
            )",
            [],
        )?;
        Ok(())
    }

    fn insert_sqlite_row(conn: &Connection, now: &DateTime<Local>, m: &FlightMetrics) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO metrics (
                timestamp, latitude, longitude, 
                indicated_altitude, altimeter_setting, gps_altitude_msl, outside_air_temp,
                indicated_airspeed, ground_speed, vertical_speed, pitch_angle, roll_angle, 
                lateral_acceleration, normal_acceleration,
                heading, track, volts_1, volts_2, amps_1, 
                fuel_quantity_left, fuel_quantity_right,
                engine_1_fuel_flow, engine_1_oil_temp, engine_1_oil_pressure, 
                engine_1_manifold_pressure, engine_1_rpm, engine_1_percent_power,
                engine_1_cht_1, engine_1_cht_2, engine_1_cht_3, engine_1_cht_4, engine_1_cht_5, engine_1_cht_6,
                engine_1_egt_1, engine_1_egt_2, engine_1_egt_3, engine_1_egt_4, engine_1_egt_5, engine_1_egt_6,
                engine_1_tit_1, engine_1_tit_2, 
                gps_altitude_wgs84, true_airspeed, hsi_source, selected_course, 
                nav_1_frequency, nav_2_frequency, com_1_frequency, com_2_frequency, 
                horizontal_cdi, vertical_cdi, wind_speed, wind_direction,
                waypoint_distance, waypoint_bearing, magnetic_variation, 
                autopilot_active, roll_mode, pitch_mode,
                roll_command, pitch_command, vertical_speed_target, 
                gps_fix_type, horizontal_alarm_limit, vertical_alarm_limit,
                horizontal_protection_level_waas, horizontal_protection_level_fd, vertical_protection_level_waas, 
                is_on_ground
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
                ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41,
                ?42, ?43, ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57, ?58, ?59, ?60, ?61,
                ?62, ?63, ?64, ?65, ?66, ?67, ?68, ?69
            )",
            params![
                now.format("%Y-%m-%d %H:%M:%S").to_string(), m.latitude, m.longitude, m.indicated_altitude, m.altimeter_setting, m.gps_altitude_msl, m.outside_air_temp,
                m.indicated_airspeed, m.ground_speed, m.vertical_speed, m.pitch_angle, m.roll_angle, m.lateral_acceleration, m.normal_acceleration,
                m.heading, m.track, m.volts_1, m.volts_2, m.amps_1, m.fuel_quantity_left, m.fuel_quantity_right,
                m.engine_1_fuel_flow, m.engine_1_oil_temp, m.engine_1_oil_pressure, m.engine_1_manifold_pressure, m.engine_1_rpm, m.engine_1_percent_power,
                m.engine_1_cht_1, m.engine_1_cht_2, m.engine_1_cht_3, m.engine_1_cht_4, m.engine_1_cht_5, m.engine_1_cht_6,
                m.engine_1_egt_1, m.engine_1_egt_2, m.engine_1_egt_3, m.engine_1_egt_4, m.engine_1_egt_5, m.engine_1_egt_6,
                m.engine_1_tit_1, m.engine_1_tit_2, m.gps_altitude_wgs84, m.true_airspeed, m.hsi_source, m.selected_course, m.nav_1_frequency,
                m.nav_2_frequency, m.com_1_frequency, m.com_2_frequency, m.horizontal_cdi, m.vertical_cdi, m.wind_speed, m.wind_direction,
                m.waypoint_distance, m.waypoint_bearing, m.magnetic_variation, m.autopilot_active, m.roll_mode, m.pitch_mode,
                m.roll_command, m.pitch_command, m.vertical_speed_target, m.gps_fix_type, m.horizontal_alarm_limit, m.vertical_alarm_limit,
                m.horizontal_protection_level_waas, m.horizontal_protection_level_fd, m.vertical_protection_level_waas, m.is_on_ground
            ],
        )?;
        Ok(())
    }
}
