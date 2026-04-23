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
    pub alt_b: f64,
    pub baro_a: f64,
    pub alt_msl: f64,
    pub oat: f64,
    pub ias: f64,
    pub gnd_spd: f64,
    pub v_spd: f64,
    pub pitch: f64,
    pub roll: f64,
    pub lat_ac: f64,
    pub norm_ac: f64,
    pub hdg: f64,
    pub trk: f64,
    pub volt1: f64,
    pub volt2: f64,
    pub amp1: f64,
    pub f_qty_l: f64,
    pub f_qty_r: f64,
    pub e1_fflow: f64,
    pub e1_oil_t: f64,
    pub e1_oil_p: f64,
    pub e1_map: f64,
    pub e1_rpm: f64,
    pub e1_pwr: f64,
    pub e1_cht1: f64,
    pub e1_cht2: f64,
    pub e1_cht3: f64,
    pub e1_cht4: f64,
    pub e1_cht5: f64,
    pub e1_cht6: f64,
    pub e1_egt1: f64,
    pub e1_egt2: f64,
    pub e1_egt3: f64,
    pub e1_egt4: f64,
    pub e1_egt5: f64,
    pub e1_egt6: f64,
    pub e1_tit1: f64,
    pub e1_tit2: f64,
    pub alt_gps: f64,
    pub tas: f64,
    pub hsis: f64,
    pub crs: f64,
    pub nav1: f64,
    pub nav2: f64,
    pub com1: f64,
    pub com2: f64,
    pub hcdi: f64,
    pub vcdi: f64,
    pub wnd_spd: f64,
    pub wnd_dr: f64,
    pub wpt_dst: f64,
    pub wpt_brg: f64,
    pub mag_var: f64,
    pub afcs_on: f64,
    pub roll_m: f64,
    pub pitch_m: f64,
    pub roll_c: f64,
    pub pitch_c: f64,
    pub v_spd_g: f64,
    pub gps_fix: f64,
    pub hal: f64,
    pub val: f64,
    pub hpl_was: f64,
    pub hpl_fd: f64,
    pub vpl_was: f64,
    pub sim_on_ground: f64,
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

        // ... existing metrics definitions ...
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
                                
                                let has_movement = data.gnd_spd.abs() > 0.1 || data.v_spd.abs() > 10.0;
                                
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
                latitude REAL, longitude REAL, alt_b REAL, baro_a REAL, alt_msl REAL, oat REAL,
                ias REAL, gnd_spd REAL, v_spd REAL, pitch REAL, roll REAL, lat_ac REAL, norm_ac REAL,
                hdg REAL, trk REAL, volt1 REAL, volt2 REAL, amp1 REAL, f_qty_l REAL, f_qty_r REAL,
                e1_fflow REAL, e1_oil_t REAL, e1_oil_p REAL, e1_map REAL, e1_rpm REAL, e1_pwr REAL,
                e1_cht1 REAL, e1_cht2 REAL, e1_cht3 REAL, e1_cht4 REAL, e1_cht5 REAL, e1_cht6 REAL,
                e1_egt1 REAL, e1_egt2 REAL, e1_egt3 REAL, e1_egt4 REAL, e1_egt5 REAL, e1_egt6 REAL,
                e1_tit1 REAL, e1_tit2 REAL, alt_gps REAL, tas REAL, hsis REAL, crs REAL, nav1 REAL,
                nav2 REAL, com1 REAL, com2 REAL, hcdi REAL, vcdi REAL, wnd_spd REAL, wnd_dr REAL,
                wpt_dst REAL, wpt_brg REAL, mag_var REAL, afcs_on REAL, roll_m REAL, pitch_m REAL,
                roll_c REAL, pitch_c REAL, v_spd_g REAL, gps_fix REAL, hal REAL, val REAL,
                hpl_was REAL, hpl_fd REAL, vpl_was REAL, sim_on_ground REAL
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
            "INSERT INTO metrics (
                timestamp, latitude, longitude, alt_b, baro_a, alt_msl, oat,
                ias, gnd_spd, v_spd, pitch, roll, lat_ac, norm_ac,
                hdg, trk, volt1, volt2, amp1, f_qty_l, f_qty_r,
                e1_fflow, e1_oil_t, e1_oil_p, e1_map, e1_rpm, e1_pwr,
                e1_cht1, e1_cht2, e1_cht3, e1_cht4, e1_cht5, e1_cht6,
                e1_egt1, e1_egt2, e1_egt3, e1_egt4, e1_egt5, e1_egt6,
                e1_tit1, e1_tit2, alt_gps, tas, hsis, crs, nav1,
                nav2, com1, com2, hcdi, vcdi, wnd_spd, wnd_dr,
                wpt_dst, wpt_brg, mag_var, afcs_on, roll_m, pitch_m,
                roll_c, pitch_c, v_spd_g, gps_fix, hal, val,
                hpl_was, hpl_fd, vpl_was, sim_on_ground
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
                ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41,
                ?42, ?43, ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57, ?58, ?59, ?60, ?61,
                ?62, ?63, ?64, ?65, ?66, ?67, ?68, ?69, ?70, ?71
            )",
            params![
                now.format("%Y-%m-%d %H:%M:%S").to_string(), m.latitude, m.longitude, m.alt_b, m.baro_a, m.alt_msl, m.oat,
                m.ias, m.gnd_spd, m.v_spd, m.pitch, m.roll, m.lat_ac, m.norm_ac,
                m.hdg, m.trk, m.volt1, m.volt2, m.amp1, m.f_qty_l, m.f_qty_r,
                m.e1_fflow, m.e1_oil_t, m.e1_oil_p, m.e1_map, m.e1_rpm, m.e1_pwr,
                m.e1_cht1, m.e1_cht2, m.e1_cht3, m.e1_cht4, m.e1_cht5, m.e1_cht6,
                m.e1_egt1, m.e1_egt2, m.e1_egt3, m.e1_egt4, m.e1_egt5, m.e1_egt6,
                m.e1_tit1, m.e1_tit2, m.alt_gps, m.tas, m.hsis, m.crs, m.nav1,
                m.nav2, m.com1, m.com2, m.hcdi, m.vcdi, m.wnd_spd, m.wnd_dr,
                m.wpt_dst, m.wpt_brg, m.mag_var, m.afcs_on, m.roll_m, m.pitch_m,
                m.roll_c, m.pitch_c, m.v_spd_g, m.gps_fix, m.hal, m.val,
                m.hpl_was, m.hpl_fd, m.vpl_was, m.sim_on_ground
            ],
        )?;
        Ok(())
    }
}
