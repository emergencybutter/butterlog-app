use crate::airports::AirportsDatabase;
use crate::config::ConfigManager;
use crate::models::{FlightEvent, FlightMetrics};
use chrono::{FixedOffset, NaiveDateTime, TimeZone, Utc};
use directories::UserDirs;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct AircraftStats {
    pub aircraft_type: String,
    pub total_hours_all: f64,
    pub total_fuel_all: f64,
    pub total_flights_all: i32,
    pub total_hours_completed: f64,
    pub total_fuel_completed: f64,
    pub total_flights_completed: i32,
    pub last_airport: String,
}

pub fn update_aircraft_stats(
    app: &AppHandle,
    aircraft_type: &str,
    duration_mins: f64,
    fuel_used: f64,
    last_airport: &str,
    is_completed: bool,
) -> anyhow::Result<()> {
    let app_data_dir = app.path().app_data_dir()?;
    let db_path = app_data_dir.join("aircraft_stats.db");
    let conn = Connection::open(db_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS aircraft_stats (
            aircraft_type TEXT PRIMARY KEY,
            total_hours_all REAL DEFAULT 0,
            total_fuel_all REAL DEFAULT 0,
            total_flights_all INTEGER DEFAULT 0,
            total_hours_completed REAL DEFAULT 0,
            total_fuel_completed REAL DEFAULT 0,
            total_flights_completed INTEGER DEFAULT 0,
            last_airport TEXT
        )",
        [],
    )?;

    let hours = duration_mins / 60.0;

    conn.execute(
        "INSERT INTO aircraft_stats (aircraft_type, total_hours_all, total_fuel_all, total_flights_all, last_airport)
         VALUES (?1, ?2, ?3, 1, ?4)
         ON CONFLICT(aircraft_type) DO UPDATE SET
            total_hours_all = total_hours_all + ?2,
            total_fuel_all = total_fuel_all + ?3,
            total_flights_all = total_flights_all + 1,
            last_airport = ?4",
        params![aircraft_type, hours, fuel_used, last_airport],
    )?;

    if is_completed {
        conn.execute(
            "UPDATE aircraft_stats SET
                total_hours_completed = total_hours_completed + ?2,
                total_fuel_completed = total_fuel_completed + ?3,
                total_flights_completed = total_flights_completed + 1
             WHERE aircraft_type = ?1",
            params![aircraft_type, hours, fuel_used],
        )?;
    }

    crate::append_log(app, format!("[Stats] Updated stats for {}: +{:.2}h, +{:.1} fuel, last airport: {}", aircraft_type, hours, fuel_used, last_airport));

    Ok(())
}

#[tauri::command]
pub async fn get_aircraft_stats(app: AppHandle) -> Result<Vec<AircraftStats>, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let db_path = app_data_dir.join("aircraft_stats.db");
    
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT * FROM aircraft_stats ORDER BY total_hours_all DESC")
        .map_err(|e| {
            let err = e.to_string();
            crate::append_log(&app, format!("[Stats] Database error (prepare): {}", err));
            err
        })?;

    let rows = stmt
        .query_map([], |row| {
            Ok(AircraftStats {
                aircraft_type: row.get(0)?,
                total_hours_all: row.get(1)?,
                total_fuel_all: row.get(2)?,
                total_flights_all: row.get(3)?,
                total_hours_completed: row.get(4)?,
                total_fuel_completed: row.get(5)?,
                total_flights_completed: row.get(6)?,
                last_airport: row.get(7)?,
            })
        })
        .map_err(|e| {
            let err = e.to_string();
            crate::append_log(&app, format!("[Stats] Database error (query_map): {}", err));
            err
        })?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(row.map_err(|e| e.to_string())?);
    }
    Ok(stats)
}

pub fn init_sqlite_db(conn: &Connection) -> rusqlite::Result<()> {
    // Enable WAL mode
    conn.pragma_update(None, "journal_mode", "WAL")?;
    
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
            is_on_ground REAL, altitude_agl REAL DEFAULT 0.0,
            gforce REAL,
            pressure_altitude REAL,
            density_altitude REAL,
            pressurization_cabin_altitude REAL,
            xp_prop_rpm REAL DEFAULT 0.0, xp_gear_ratio REAL DEFAULT 0.0
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

pub fn insert_sqlite_row(
    conn: &Connection,
    now_str: &str,
    m: &FlightMetrics,
) -> rusqlite::Result<()> {
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
            is_on_ground,
            altitude_agl,
            gforce, pressure_altitude, density_altitude, pressurization_cabin_altitude,
            xp_prop_rpm, xp_gear_ratio
            ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21,
            ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41,
            ?42, ?43, ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57, ?58, ?59, ?60, ?61,
            ?62,
            ?63, 
            ?64, 
            ?65, 
            ?66, 
            ?67, 
            ?68, 
            ?69, 
            ?70
        )",
        params![
            now_str, m.latitude, m.longitude, m.indicated_altitude, m.altimeter_setting, m.gps_altitude_msl, m.outside_air_temp,
            m.indicated_airspeed, m.ground_speed, m.vertical_speed, m.pitch_angle, m.roll_angle, m.lateral_acceleration, m.normal_acceleration,
            m.heading, m.track, m.volts_1, m.volts_2, m.amps_1, m.fuel_quantity_left, m.fuel_quantity_right,
            m.engine_1_fuel_flow, m.engine_1_oil_temp, m.engine_1_oil_pressure, m.engine_1_manifold_pressure, m.engine_1_rpm, m.engine_1_percent_power,
            m.engine_1_cht_1, m.engine_1_cht_2, m.engine_1_cht_3, m.engine_1_cht_4, m.engine_1_cht_5, m.engine_1_cht_6,
            m.engine_1_egt_1, m.engine_1_egt_2, m.engine_1_egt_3, m.engine_1_egt_4, m.engine_1_egt_5, m.engine_1_egt_6,
            m.engine_1_tit_1, m.engine_1_tit_2, m.gps_altitude_wgs84, m.true_airspeed, m.hsi_source, m.selected_course, m.nav_1_frequency,
            m.nav_2_frequency, m.com_1_frequency, m.com_2_frequency, m.horizontal_cdi, m.vertical_cdi, m.wind_speed, m.wind_direction,
            m.waypoint_distance, m.waypoint_bearing, m.magnetic_variation, m.autopilot_active, m.roll_mode, m.pitch_mode,
            m.roll_command, m.pitch_command, m.vertical_speed_target, 
            m.is_on_ground,
            m.altitude_agl,
            m.gforce, 
            m.pressure_altitude, 
            m.density_altitude, 
            m.pressurization_cabin_altitude,
            m.xp_prop_rpm, 
            m.xp_gear_ratio
        ],
    )?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FlightSummary {
    pub filename: String,
    pub start_icao: String,
    pub start_airport_name: String,
    pub end_icao: String,
    pub end_airport_name: String,
    pub start_time: String,
    pub end_time: String,
    // JSON (and therefore Tauri's invoke) carries these as plain numbers
    #[ts(type = "number")]
    pub duration_minutes: i64,
    #[ts(type = "number")]
    pub file_size_bytes: u64,
    pub aircraft_title: String,
    #[serde(default)]
    pub livery: String,
    #[serde(default)]
    pub resolved_icao: String,
    #[serde(default)]
    pub resolved_airline: String,
    pub atc_model: String,
    pub atc_id: String,
    pub max_altitude: f64,
    pub max_ground_speed: f64,
    pub fuel_consumed: f64,
    pub events: Vec<FlightEvent>,
    pub screenshot_count: usize,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, ts_rs::TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FlightLogRow {
    pub timestamp: String,
    pub metrics: FlightMetrics,
}

#[tauri::command]
pub async fn get_flight_data(
    app: AppHandle,
    filename: String,
    regen_flag: tauri::State<'_, crate::RegenerateSummaryFlag>,
) -> Result<Vec<FlightLogRow>, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());

    let path = log_dir.join(&filename);
    if !path.exists() {
        return Err("File not found".to_string());
    }

    if regen_flag.0 {
        crate::append_log(&app, format!("Regenerating summary for flight: {}", filename));
        if let Err(e) = regenerate_flight_summary(&app, &path) {
            crate::append_log(&app, format!("Failed to regenerate summary: {}", e));
        }
    }

    let conn = Connection::open(path).map_err(|e| {
        let err = e.to_string();
        crate::append_log(&app, format!("Database error (open): {}", err));
        err
    })?;
    let mut stmt = conn
        .prepare("SELECT * FROM metrics ORDER BY timestamp ASC")
        .map_err(|e| {
            let err = e.to_string();
            crate::append_log(&app, format!("Database error (prepare): {}", err));
            err
        })?;

    let rows = stmt
        .query_map([], |row| {
            Ok(FlightLogRow {
                timestamp: row.get(0)?,
                metrics: map_row_to_metrics(row)?,
            })
        })
        .map_err(|e| {
            let err = e.to_string();
            crate::append_log(&app, format!("Database error (query_map): {}", err));
            err
        })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }

    Ok(result)
}

#[tauri::command]
pub async fn get_flight_data_since(
    app: AppHandle,
    filename: String,
    since: String,
) -> Result<Vec<FlightLogRow>, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());
    let path = log_dir.join(&filename);
    if !path.exists() {
        return Err("File not found".to_string());
    }
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT * FROM metrics WHERE timestamp > ?1 ORDER BY timestamp ASC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![since], |row| {
            Ok(FlightLogRow {
                timestamp: row.get(0)?,
                metrics: map_row_to_metrics(row)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

fn regenerate_flight_summary(app: &AppHandle, path: &PathBuf) -> anyhow::Result<()> {
    let conn = Connection::open(path)?;
    
    // Preserve aircraft_title if it exists
    let aircraft_title: String = conn.query_row(
        "SELECT value FROM summary WHERE key = 'aircraft_title'",
        [],
        |r| r.get(0)
    ).unwrap_or_else(|_| "Unknown Aircraft".to_string());

    let mut stmt = conn.prepare("SELECT * FROM metrics ORDER BY timestamp ASC")?;
    let rows = stmt.query_map([], |row| {
        Ok(FlightLogRow {
            timestamp: row.get(0)?,
            metrics: map_row_to_metrics(row)?,
        })
    })?;

    let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
    
    for row in rows {
        let row = row?;
        analyzer.update(&row.metrics, &row.timestamp);
    }

    // Determine ICAOs and Names
    let (start_icao, start_name, end_icao, end_name) =
        if let (Some(db), Some(r_db)) = (app.try_state::<crate::airports::AirportsDatabase>(), app.try_state::<crate::runways::RunwaysDatabase>()) {
            // Recalculate landing performance
            analyzer.finalize_landing_performance(&db, &r_db, Some(&conn));

            let s_icao = analyzer.find_start_icao(&db);
            let e_icao = analyzer.find_end_icao(&db);
            let s_name = if s_icao == "Airborne" { "Airborne".to_string() } else {
                db.get_by_ident(&s_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string())
            };
            let e_name = if e_icao == "Airborne" { "Airborne".to_string() } else {
                db.get_by_ident(&e_icao).map(|a| a.name.clone()).unwrap_or_else(|| "Unknown".to_string())
            };
            (s_icao, s_name, e_icao, e_name)
        } else {
            ("XXXX".to_string(), "Unknown".to_string(), "XXXX".to_string(), "Unknown".to_string())
        };

    let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
    let mut summary_data = vec![
        ("departure_icao", start_icao),
        ("departure_name", start_name),
        ("arrival_icao", end_icao),
        ("arrival_name", end_name),
        ("aircraft_title", aircraft_title),
        ("max_altitude", analyzer.max_alt.to_string()),
        ("max_ground_speed", analyzer.max_gs.to_string()),
        ("fuel_consumed", fuel_consumed.to_string()),
        ("flight_events", serde_json::to_string(&analyzer.events).unwrap_or_default()),
    ];

    if let Some(landing) = analyzer.events.iter().find(|e| e.event_type == "landing") {
        if let Some(v) = landing.touchdown_fpm { summary_data.push(("touchdown_fpm", v.to_string())); }
        if let Some(v) = landing.landing_g { summary_data.push(("landing_g", v.to_string())); }
        if let Some(v) = landing.offset_percent { summary_data.push(("landing_offset_pct", v.to_string())); }
        if let Some(v) = landing.threshold_dist_ft { summary_data.push(("landing_dist_ft", v.to_string())); }
        if let Some(v) = landing.vs_variance { summary_data.push(("vs_variance", v.to_string())); }
        if let Some(v) = landing.ias_variance { summary_data.push(("ias_variance", v.to_string())); }
        if let Some(v) = landing.calculate_landing_score() { summary_data.push(("landing_score", v.to_string())); }
        
        crate::append_log(app, format!("[Regen] Recalculated landing: {} fpm, score: {:?}", 
            landing.touchdown_fpm.unwrap_or(0.0), 
            landing.calculate_landing_score()));
    }

    for (k, v) in summary_data {
        conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params![k, v])?;
    }

    crate::append_log(app, format!("[Regen] Re-calculated summary for {}", path.display()));
    let _ = app.emit("flight-logs-updated", ());
    Ok(())
}


pub fn map_row_to_metrics(row: &rusqlite::Row) -> rusqlite::Result<FlightMetrics> {
    Ok(FlightMetrics {
        latitude: row.get(1)?,
        longitude: row.get(2)?,
        indicated_altitude: row.get(3)?,
        altimeter_setting: row.get(4)?,
        gps_altitude_msl: row.get(5)?,
        outside_air_temp: row.get(6)?,
        indicated_airspeed: row.get(7)?,
        ground_speed: row.get(8)?,
        vertical_speed: row.get(9)?,
        pitch_angle: row.get(10)?,
        roll_angle: row.get(11)?,
        lateral_acceleration: row.get(12)?,
        normal_acceleration: row.get(13)?,
        heading: row.get(14)?,
        track: row.get(15)?,
        volts_1: row.get(16)?,
        volts_2: row.get(17)?,
        amps_1: row.get(18)?,
        fuel_quantity_left: row.get(19)?,
        fuel_quantity_right: row.get(20)?,
        engine_1_fuel_flow: row.get(21)?,
        engine_1_oil_temp: row.get(22)?,
        engine_1_oil_pressure: row.get(23)?,
        engine_1_manifold_pressure: row.get(24)?,
        engine_1_rpm: row.get(25)?,
        engine_1_percent_power: row.get(26)?,
        engine_1_cht_1: row.get(27)?,
        engine_1_cht_2: row.get(28)?,
        engine_1_cht_3: row.get(29)?,
        engine_1_cht_4: row.get(30)?,
        engine_1_cht_5: row.get(31)?,
        engine_1_cht_6: row.get(32)?,
        engine_1_egt_1: row.get(33)?,
        engine_1_egt_2: row.get(34)?,
        engine_1_egt_3: row.get(35)?,
        engine_1_egt_4: row.get(36)?,
        engine_1_egt_5: row.get(37)?,
        engine_1_egt_6: row.get(38)?,
        engine_1_tit_1: row.get(39)?,
        engine_1_tit_2: row.get(40)?,
        gps_altitude_wgs84: row.get(41)?,
        true_airspeed: row.get(42)?,
        hsi_source: row.get(43)?,
        selected_course: row.get(44)?,
        nav_1_frequency: row.get(45)?,
        nav_2_frequency: row.get(46)?,
        com_1_frequency: row.get(47)?,
        com_2_frequency: row.get(48)?,
        horizontal_cdi: row.get(49)?,
        vertical_cdi: row.get(50)?,
        wind_speed: row.get(51)?,
        wind_direction: row.get(52)?,
        waypoint_distance: row.get(53)?,
        waypoint_bearing: row.get(54)?,
        magnetic_variation: row.get(55)?,
        autopilot_active: row.get(56)?,
        roll_mode: row.get(57)?,
        pitch_mode: row.get(58)?,
        roll_command: row.get(59)?,
        pitch_command: row.get(60)?,
        vertical_speed_target: row.get(61)?,
        is_on_ground: row.get(62)?,
        altitude_agl: row.get(63)?,
        gforce: row.get(64)?,
        pressure_altitude: row.get(65)?,
        density_altitude: row.get(66)?,
        pressurization_cabin_altitude: row.get(67)?,
        xp_prop_rpm: row.get(68)?,
        xp_gear_ratio: row.get(69)?,
        parking_brake: 0.0,
        // Light states aren't persisted in flight logs; default them.
        ..Default::default()
    })
}

/// Cache of parsed flight summaries keyed by path, invalidated by file mtime.
/// scan_logs runs on every history refresh; without this every flight DB would
/// be re-parsed each time.
static SUMMARY_CACHE: parking_lot::Mutex<Option<std::collections::HashMap<PathBuf, (std::time::SystemTime, FlightSummary)>>> =
    parking_lot::Mutex::new(None);

pub fn scan_logs(app: AppHandle) -> Result<Vec<FlightSummary>, String> {
    crate::append_log(&app, format!("[Logs] Scanning logs."));
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());

    if !log_dir.exists() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();
    let entries = fs::read_dir(log_dir).map_err(|e| e.to_string())?;
    let entries_vec: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    let total = entries_vec.len();

    let mut cache_guard = SUMMARY_CACHE.lock();
    let cache = cache_guard.get_or_insert_with(std::collections::HashMap::new);
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for (i, entry) in entries_vec.into_iter().enumerate() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("db") {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
            let mtime = entry.metadata().and_then(|m| m.modified()).ok();
            seen.insert(path.clone());

            let cached = mtime.and_then(|mt| {
                cache.get(&path).filter(|(cached_mt, _)| *cached_mt == mt).map(|(_, s)| s.clone())
            });

            if let Some(mut summary) = cached {
                // Screenshot count lives in screenshots.db, which can change
                // without touching the flight DB's mtime — always refresh it.
                if let Some(mgr) = app.try_state::<crate::screenshot_manager::ScreenshotManager>() {
                    summary.screenshot_count = mgr
                        .get_screenshots_for_flight(&filename.replace(".db", ""))
                        .map(|s| s.len())
                        .unwrap_or(summary.screenshot_count);
                }
                summaries.push(summary);
            } else if let Some(summary) = parse_db_file(&app, &path) {
                if let Some(mt) = mtime {
                    cache.insert(path.clone(), (mt, summary.clone()));
                }
                summaries.push(summary);
            } else {
                crate::append_log(&app, format!("[Debug] Failed to parse log file: {}", filename));
            }
        }

        // Emit progress every 5 files or at the end
        if (i + 1) % 5 == 0 || (i + 1) == total {
            let _ = app.emit("scan-progress", serde_json::json!({
                "current": i + 1,
                "total": total
            }));
        }
    }

    // Drop cache entries for files that no longer exist
    cache.retain(|path, _| seen.contains(path));
    drop(cache_guard);

    // Sort by start time descending
    summaries.sort_by(|a, b| b.start_time.cmp(&a.start_time));

    Ok(summaries)
}

/// How far a log's last sample may be from where the aircraft is now and still be the same
/// flight: [`RESUME_BASE_NM`] and [`RESUME_BASE_FT`], plus what the aircraft could have
/// covered at its last ground and vertical speed while the app was away, with a margin.
/// A 777 at 550 kt flies the 10 NM in 65 s, so a reconnect that took longer used to start a
/// new log (IronClaw737's KSFO flight, 2026-11-07, split three ways). The time away only
/// counts up to [`RESUME_MAX_GAP_S`]: past that a flight only resumes where it stopped,
/// as a paused one does.
const RESUME_BASE_NM: f64 = 10.0;
const RESUME_BASE_FT: f64 = 1000.0;
const RESUME_MARGIN: f64 = 1.5;
const RESUME_MAX_GAP_S: i64 = 30 * 60;

fn resume_matches(dist_nm: f64, alt_diff_ft: f64, last: &FlightMetrics, elapsed_s: i64) -> bool {
    let away_s = if elapsed_s <= RESUME_MAX_GAP_S { elapsed_s.max(0) as f64 } else { 0.0 };
    let reach_nm = last.ground_speed.abs() * away_s / 3600.0 * RESUME_MARGIN;
    let reach_ft = last.vertical_speed.abs() * away_s / 60.0 * RESUME_MARGIN;
    dist_nm <= RESUME_BASE_NM + reach_nm && alt_diff_ft <= RESUME_BASE_FT + reach_ft
}

pub fn try_find_resume_flight(
        app: &AppHandle,
        current_metrics: &FlightMetrics,
        aircraft_title: &str,
   ) -> Option<PathBuf> {
    let app_data_dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => return None,
    };
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());

    if !log_dir.exists() {
        return None;
    }

    let entries = fs::read_dir(log_dir).ok()?;
    let mut files: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("db"))
        .collect();


    // Sort by modification time (newest first)
    files.sort_by(|a, b| {
        let ma = a.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let mb = b.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        mb.cmp(&ma)
    });

    for entry in files.iter().take(3) { // Check 3 most recent
        let path = entry.path();
        if let Some(summary) = parse_db_file(app, &path) {
            // Must be airborne end
            if summary.end_icao != "Airborne" {
                continue;
            }

            // Aircraft must match
            if summary.aircraft_title != aircraft_title {
                continue;
            }

            // Open DB and get last metrics
            if let Ok(conn) = Connection::open(&path) {
                let last_metrics_res = conn.query_row(
                    "SELECT * FROM metrics ORDER BY timestamp DESC LIMIT 1",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, map_row_to_metrics(row)?))
                );

                if let Ok((last_ts, last_m)) = last_metrics_res {
                    let dist = crate::sim_monitor::calculate_distance(
                        current_metrics.latitude,
                        current_metrics.longitude,
                        last_m.latitude,
                        last_m.longitude
                    );

                    let alt_diff = (current_metrics.gps_altitude_msl - last_m.gps_altitude_msl).abs();

                    // Unix seconds; an unreadable time counts as no time at all.
                    let last_epoch = ts_to_epoch(&last_ts);
                    let elapsed_s = if last_epoch > 0 { Utc::now().timestamp() - last_epoch } else { 0 };

                    // No lower bound on dist/alt_diff: a pause freezes the aircraft in
                    // place, so a reconnect at the identical position must still resume the
                    // same flight rather than spawn a new log.
                    if resume_matches(dist, alt_diff, &last_m, elapsed_s) {
                        crate::append_log(app, format!("[Resumption] Found match: {}. Dist: {:.1}nm, Alt Diff: {:.0}ft, {}s since its last sample", summary.filename, dist, alt_diff, elapsed_s));
                        return Some(path);
                    }
                    crate::append_log(app, format!("[Resumption] No match: {}. Dist: {:.1}nm, Alt Diff: {:.0}ft, {}s since its last sample", summary.filename, dist, alt_diff, elapsed_s));
                }
            }
        }
    }

    None
}

fn parse_utc_offset(offset_str: &str) -> Option<FixedOffset> {
    let offset_str = offset_str.trim();
    if offset_str.is_empty() {
        return Some(FixedOffset::east_opt(0)?);
    }

    let sign = if offset_str.starts_with('-') { -1 } else { 1 };
    let parts: Vec<&str> = offset_str
        .trim_start_matches(|c| c == '+' || c == '-')
        .split(':')
        .collect();

    if parts.len() == 2 {
        let hours: i32 = parts[0].parse().ok()?;
        let minutes: i32 = parts[1].parse().ok()?;
        let total_seconds = sign * (hours * 3600 + minutes * 60);
        FixedOffset::east_opt(total_seconds)
    } else if parts.len() == 1 {
        let hours: i32 = parts[0].parse().ok()?;
        let total_seconds = sign * hours * 3600;
        FixedOffset::east_opt(total_seconds)
    } else {
        None
    }
}

pub fn parse_db_file(app: &AppHandle, path: &PathBuf) -> Option<FlightSummary> {
    let filename = path.file_name()?.to_str()?.to_string();
    let metadata = fs::metadata(path).ok()?;

    let conn = Connection::open(path).map_err(|e| {
        crate::append_log(app, format!("[Logs] Failed to open DB {}: {}", filename, e));
        e
    }).ok()?;

    // Check if the metrics table has any data. Distinguish "no rows" from a
    // query error (e.g. missing table / schema mismatch) so a log that exists on
    // disk but never shows up in the list leaves a diagnosable reason behind
    // instead of a bare "Failed to parse".
    let count: i64 = match conn.query_row("SELECT count(*) FROM metrics", [], |r| r.get(0)) {
        Ok(c) => c,
        Err(e) => {
            crate::append_log(app, format!("[Logs] Skipping {}: metrics table unreadable: {}", filename, e));
            return None;
        }
    };
    if count == 0 {
        crate::append_log(app, format!("[Logs] Skipping {}: metrics table is empty (no data points recorded)", filename));
        return None;
    }

    let get_summary = |key: &str| -> String {
        conn.query_row(
            "SELECT value FROM summary WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| {
            if !matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                crate::append_log(app, format!("[Logs] Database error (query_row {}): {}", key, e));
            }
            e
        })
        .unwrap_or_else(|_| "Unknown".to_string())
    };

    let start_icao = get_summary("departure_icao");
    let start_airport_name = if start_icao == "Airborne" {
        "Airborne".to_string()
    } else {
        get_summary("departure_name")
    };
    let end_icao = get_summary("arrival_icao");
    let end_airport_name = get_summary("arrival_name");
    let aircraft_title = get_summary("aircraft_title");
    let atc_model = get_summary("atc_model");
    let atc_id = get_summary("atc_id");
    let max_altitude = get_summary("max_altitude").parse().unwrap_or(0.0);
    let max_ground_speed = get_summary("max_ground_speed").parse().unwrap_or(0.0);
    let fuel_consumed = get_summary("fuel_consumed").parse().unwrap_or(0.0);
    let events_json = get_summary("flight_events");
    let events: Vec<FlightEvent> = serde_json::from_str(&events_json).unwrap_or_default();

    // Notes are optional and default to empty (not "Unknown") when absent.
    let notes = conn.query_row(
        "SELECT value FROM summary WHERE key = 'notes'",
        [],
        |r| r.get::<_, String>(0),
    ).optional().ok().flatten().unwrap_or_default();

    // Livery is optional and defaults to empty (not "Unknown") when absent — it can
    // be blank in-sim and is missing entirely on flights logged before this field.
    let livery = conn.query_row(
        "SELECT value FROM summary WHERE key = 'livery'",
        [],
        |r| r.get::<_, String>(0),
    ).optional().ok().flatten().unwrap_or_default();

    // ICAO type resolved from title/livery via the word index; optional, empty when
    // unmatched or for flights logged before this field existed.
    let resolved_icao = conn.query_row(
        "SELECT value FROM summary WHERE key = 'resolved_icao'",
        [],
        |r| r.get::<_, String>(0),
    ).optional().ok().flatten().unwrap_or_default();

    // Airline name identified from title/livery by the airline index; optional.
    let resolved_airline = conn.query_row(
        "SELECT value FROM summary WHERE key = 'resolved_airline'",
        [],
        |r| r.get::<_, String>(0),
    ).optional().ok().flatten().unwrap_or_default();

    let mut stmt = conn
        .prepare("SELECT MIN(timestamp), MAX(timestamp) FROM metrics")
        .map_err(|e| {
            crate::append_log(app, format!("[Logs] Database error (prepare timestamps): {}", e));
            e
        })
        .ok()?;
    let time_res: rusqlite::Result<(Option<String>, Option<String>)> =
        stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| {
            crate::append_log(app, format!("[Logs] Database error (query_row timestamps): {}", e));
            e
        });

    let (start_time, end_time) = match time_res {
        Ok((Some(s), Some(e))) => (s, e),
        _ => (
            Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        ),
    };

    let duration_minutes = if let (Ok(start_dt), Ok(end_dt)) = (
        NaiveDateTime::parse_from_str(
            &start_time.split('.').next().unwrap_or(&start_time),
            "%Y-%m-%d %H:%M:%S",
        ),
        NaiveDateTime::parse_from_str(
            &end_time.split('.').next().unwrap_or(&end_time),
            "%Y-%m-%d %H:%M:%S",
        ),
    ) {
        end_dt.signed_duration_since(start_dt).num_minutes()
    } else {
        0
    };

    let screenshot_count = if let Some(mgr) = app.try_state::<crate::screenshot_manager::ScreenshotManager>() {
        mgr.get_screenshots_for_flight(&filename.replace(".db", ""))
            .map(|scrs| scrs.len())
            .unwrap_or(0)
    } else {
        0
    };

    Some(FlightSummary {
        filename,
        start_icao,
        start_airport_name,
        end_icao,
        end_airport_name,
        start_time,
        end_time,
        duration_minutes,
        file_size_bytes: metadata.len(),
        aircraft_title,
        livery,
        resolved_icao,
        resolved_airline,
        atc_model,
        atc_id,
        max_altitude,
        max_ground_speed,
        fuel_consumed,
        events,
        screenshot_count,
        notes,
    })
}

/// Maximum allowed length (in characters) of a user-provided flight note.
pub const MAX_NOTES_LEN: usize = 500;

/// Save user notes for a flight to its local DB and, when the flight has been
/// synced to the service, push them to the remote notes endpoint.
#[tauri::command]
pub async fn set_flight_notes(app: AppHandle, filename: String, notes: String) -> Result<(), String> {
    if notes.chars().count() > MAX_NOTES_LEN {
        return Err(format!("Notes must be {} characters or fewer", MAX_NOTES_LEN));
    }

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());
    let path = log_dir.join(&filename);
    if !path.exists() {
        return Err("Flight log not found".to_string());
    }

    // Persist locally and read back the remote id (if the flight was synced).
    let remote_id = {
        let conn = Connection::open(&path).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO summary (key, value) VALUES ('notes', ?1)",
            params![notes],
        ).map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT value FROM summary WHERE key = 'remote_id'",
            [],
            |r| r.get::<_, String>(0),
        ).optional().ok().flatten().and_then(|s| s.parse::<i64>().ok())
    };

    // Mirror to the service when connected and the flight exists remotely.
    if let Some(id) = remote_id {
        let config = app.state::<ConfigManager>().get_config();
        if let Some((base_url, api_token)) = config.api_auth().filter(|_| config.enable_webhook) {
            let url = format!("{}/flights/{}/notes", base_url, id);
            let client = reqwest::Client::new();
            match client.put(&url).bearer_auth(&api_token).json(&serde_json::json!({ "notes": notes })).send().await {
                Ok(res) if res.status().as_u16() == 401 => {
                    crate::force_logout(&app, "service rejected token during notes update");
                }
                Ok(res) if !res.status().is_success() => {
                    crate::append_log(&app, format!("[Notes] Service rejected notes update: {}", res.status()));
                }
                Err(e) => {
                    crate::append_log(&app, format!("[Notes] Failed to sync notes to service: {}", e));
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Which sim made a log, as `record_simulator` noted it; empty for a log from before that.
fn logged_simulator(conn: &Connection) -> String {
    conn.query_row("SELECT value FROM summary WHERE key = 'simulator'", [], |r| r.get(0))
        .unwrap_or_default()
}

/// Notes which sim is writing a log, since its metrics are in that sim's signs.
pub fn record_simulator(conn: &Connection, simulator: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO summary (key, value) VALUES ('simulator', ?1)",
        params![simulator],
    )
    .map(|_| ())
}

/// What takes a logged pitch or roll to the CSV's nose up and right wing down positive.
/// Metrics are in the sim's signs, and only X-Plane's already match. Imported logs and
/// those with no sim noted hold MSFS's.
fn export_attitude_sign(simulator: &str) -> f64 {
    if simulator == "X-Plane" { 1.0 } else { -1.0 }
}

#[tauri::command]
pub async fn export_flight_to_csv(app: AppHandle, filename: String) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    let internal_log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());

    let config = app.state::<ConfigManager>().get_config();
    let export_dir = match config.log_directory.clone() {
        Some(dir) => dir,
        None => UserDirs::new()
            .and_then(|dirs| dirs.document_dir().map(|p| p.join("butterlog")))
            .ok_or_else(|| "No export directory configured and the Documents folder could not be located".to_string())?,
    };

    if !export_dir.exists() {
        fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;
    }

    let db_path = internal_log_dir.join(&filename);
    if !db_path.exists() {
        return Err("Database file not found".to_string());
    }

    let csv_filename = filename.replace(".db", ".csv");
    let csv_path = export_dir.join(&csv_filename);

    // A flight the service made is exported from its telemetry.
    if export_service_flight(&app, &db_path, &csv_path).await {
        return Ok(csv_path.to_string_lossy().to_string());
    }

    // Get aircraft info from summary table
    let conn = Connection::open(&db_path).map_err(|e| {
        let err = e.to_string();
        crate::append_log(&app, format!("Export failed (open db): {}", err));
        err
    })?;
    let airframe_name = conn
        .query_row(
            "SELECT value FROM summary WHERE key = 'aircraft_title'",
            [],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| {
            if !matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                crate::append_log(&app, format!("[Logs] Database error (export aircraft_title): {}", e));
            }
            e
        })
        .unwrap_or_else(|_| "Simulated Aircraft".to_string());
    let attitude = export_attitude_sign(&logged_simulator(&conn));

    let data = get_flight_data(app.clone(), filename, app.state::<crate::RegenerateSummaryFlag>()).await?;

    use std::io::Write;
    let mut file = fs::File::create(&csv_path).map_err(|e| {
        let err = e.to_string();
        crate::append_log(&app, format!("Export failed (create file): {}", err));
        err
    })?;

    // Write header from FORMATS.md
    writeln!(file, "#airframe_info, log_version=\"1.00\", airframe_name=\"{}\", unit_software_part_number=\"006-BXXX9-DE\", unit_software_version=\"15.24\", system_software_part_number=\"006-BXXXX-37\", system_id=\"25XXXX67\", mode=NORMAL, simulator_id=\"ButterLogV2\",", airframe_name).map_err(|e| e.to_string())?;
    writeln!(file, "#yyy-mm-dd, hh:mm:ss,   hh:mm,  ident,      degrees,      degrees, ft Baro,  inch,  ft msl, deg C,     kt,     kt,     fpm,    deg,    deg,      G,      G,   deg,   deg, volts, volts,  amps,   gals,   gals,      gph,   deg F,     psi,     Hg,    rpm,       %,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,  ft wgs,  kt, enum,    deg,    MHz,    MHz,     MHz,     MHz,    fsd,    fsd,     kt,   deg,     nm,    deg,    deg,   bool,  enum,   enum,   deg,   deg,   fpm,   enum,   mt,    mt,     mt,    mt,     mt").map_err(|e| e.to_string())?;
    writeln!(file, "Lcl Date, Lcl Time, UTCOfst, AtvWpt,     Latitude,    Longitude,    AltB, BaroA,  AltMSL,   OAT,    IAS, GndSpd,    VSpd,  Pitch,   Roll,  LatAc, NormAc,   HDG,   TRK, volt1, volt2,  amp1,  FQtyL,  FQtyR, E1 FFlow, E1 OilT, E1 OilP, E1 MAP, E1 RPM, E1 %Pwr, E1 CHT1, E1 CHT2, E1 CHT3, E1 CHT4, E1 CHT5, E1 CHT6, E1 EGT1, E1 EGT2, E1 EGT3, E1 EGT4, E1 EGT5, E1 EGT6, E1 TIT1, E1 TIT2,  AltGPS, TAS, HSIS,    CRS,   NAV1,   NAV2,    COM1,    COM2,   HCDI,   VCDI,WndSpd,WndDr, WptDst, WptBrg, MagVar, AfcsOn, RollM, PitchM, RollC, PichC, VSpdG, GPSfix,  HAL,   VAL, HPLwas, HPLfd, VPLwas").map_err(|e| e.to_string())?;

    for row in data {
        let ts_parts: Vec<&str> = row.timestamp.split(' ').collect();
        let date = ts_parts[0];
        let time = ts_parts[1].split('.').next().unwrap_or(ts_parts[1]);
        let m = row.metrics;

        let utc_offset = "+00:00";

        writeln!(file, "{}, {}, {}, {}, {:.6}, {:.6}, {:.1}, {:.2}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.3}, {:.3}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.2}, {:.2}, {:.1}, {:.1}, {:.1}, {:.2}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {}, {:.0}, {:.3}, {:.3}, {:.3}, {:.3}, {:.2}, {:.2}, {:.1}, {:.1}, {:.2}, {:.1}, {:.2}, {}, {}, {}, {:.1}, {:.1}, {:.1}, {}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}",
            date, time, utc_offset, "", m.latitude, m.longitude, m.indicated_altitude, m.altimeter_setting, m.gps_altitude_msl, m.outside_air_temp,
            m.indicated_airspeed, m.ground_speed, m.vertical_speed, attitude * m.pitch_angle, attitude * m.roll_angle, m.lateral_acceleration, m.normal_acceleration,
            m.heading, m.track, m.volts_1, m.volts_2, m.amps_1, m.fuel_quantity_left, m.fuel_quantity_right,
            m.engine_1_fuel_flow, m.engine_1_oil_temp, m.engine_1_oil_pressure, m.engine_1_manifold_pressure, m.engine_1_rpm, m.engine_1_percent_power,
            m.engine_1_cht_1, m.engine_1_cht_2, m.engine_1_cht_3, m.engine_1_cht_4, m.engine_1_cht_5, m.engine_1_cht_6,
            m.engine_1_egt_1, m.engine_1_egt_2, m.engine_1_egt_3, m.engine_1_egt_4, m.engine_1_egt_5, m.engine_1_egt_6,
            m.engine_1_tit_1, m.engine_1_tit_2, m.gps_altitude_wgs84, m.true_airspeed, "GPS", m.selected_course,
            m.nav_1_frequency, m.nav_2_frequency, m.com_1_frequency, m.com_2_frequency, m.horizontal_cdi, m.vertical_cdi, m.wind_speed, m.wind_direction,
            m.waypoint_distance, m.waypoint_bearing, m.magnetic_variation, if m.autopilot_active > 0.5 { "1" } else { "0" },
            "NONE", "NONE", attitude * m.roll_command, attitude * m.pitch_command, m.vertical_speed_target, "3DDiff",
            "", "", "", "", ""
        ).map_err(|e| e.to_string())?;
    }

    Ok(csv_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn import_flight_from_csv(app: AppHandle, path: String) -> Result<FlightSummary, String> {
    let import_filename = PathBuf::from(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    crate::append_log(
        &app,
        format!("Starting import of flight log [{}]: {}", import_filename, path),
    );

    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Err("File is empty".to_string());
    }

    let airframe_name = parse_airframe_name(&lines);
    crate::append_log(
        &app,
        format!("[{}] Detected airframe: {}", import_filename, airframe_name),
    );

    let mut rows = Vec::new();
    let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();
    let airports_db = app.try_state::<AirportsDatabase>();

    let total_rows = lines
        .iter()
        .filter(|l| !l.starts_with('#') && !l.starts_with("Lcl Date") && !l.is_empty())
        .count();
    crate::append_log(
        &app,
        format!(
            "[{}] Successfully parsed {} data points. Saving to internal database...",
            import_filename, total_rows
        ),
    );

    let mut current_row = 0;
    for line in lines {
        if line.starts_with('#') || line.starts_with("Lcl Date") || line.is_empty() {
            continue;
        }

        if let Some(row) = parse_csv_line_to_row(line, airports_db.as_deref()) {
            if let Some(_new_phase) = analyzer.update(&row.metrics, &row.timestamp) {
            }
            rows.push(row);

            current_row += 1;
            if current_row % 500 == 0 || current_row == total_rows {
                let _ = app.emit(
                    "import-progress",
                    serde_json::json!({
                        "state": "parsing",
                        "current": current_row,
                        "total": total_rows
                    }),
                );
            }
        }
    }

    let summary = save_imported_flight(&app, &airframe_name, rows, &mut analyzer, &path)
        .map_err(|e| {
            let err = e.to_string();
            crate::append_log(
                &app,
                format!("[{}] Import failed (save_imported_flight): {}", import_filename, err),
            );
            err
        })?;

    crate::append_log(
        &app,
        format!(
            "[{}] Import complete. Identified route: {} -> {}",
            import_filename, summary.start_icao, summary.end_icao
        ),
    );

    let _ = app.emit("flight-logs-updated", ());
    Ok(summary)
}

fn parse_airframe_name(lines: &[&str]) -> String {
    for line in lines {
        if line.starts_with("#airframe_info") {
            if let Some(idx) = line.find("airframe_name=\"") {
                let rest = &line[idx + 15..];
                if let Some(end_idx) = rest.find('\"') {
                    return rest[..end_idx].to_string();
                }
            }
        }
    }
    "Unknown Aircraft".to_string()
}

fn parse_csv_line_to_row(
    line: &str,
    airports_db: Option<&AirportsDatabase>,
) -> Option<FlightLogRow> {
    let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if cols.len() < 70 {
        return None;
    }

    let date_str = cols[0];
    let time_str = cols[1];
    let offset_str = cols[2];

    let mut timestamp = format!("{} {}", date_str, time_str);

    if let Ok(naive) = NaiveDateTime::parse_from_str(&timestamp, "%Y-%m-%d %H:%M:%S") {
        if let Some(offset) = parse_utc_offset(offset_str) {
            if let Some(dt) = offset.from_local_datetime(&naive).single() {
                timestamp = dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M:%S").to_string();
            }
        }
    }

    let lat: f64 = cols[4].parse().unwrap_or(0.0);
    let lon: f64 = cols[5].parse().unwrap_or(0.0);
    let alt_msl: f64 = cols[8].parse().unwrap_or(0.0);

    // Better sim_on_ground heuristic based on airport elevation and stability
    let mut sim_on_ground = 0.0;
    let v_spd: f64 = cols[12].parse().unwrap_or(0.0);
    
    if let Some(db) = airports_db {
        if let Some(nearest) = db.find_nearest(lat, lon, 1).first() {
            let elevation = nearest.elevation_ft.unwrap_or(0) as f64;
            // If we are within 50ft of the nearest airport elevation AND vertical speed is low
            if (alt_msl - elevation).abs() < 50.0 && v_spd.abs() < 50.0 {
                sim_on_ground = 1.0;
            }
        }
    } else {
        // Fallback to legacy heuristic: low altitude and low vertical speed
        if alt_msl < 500.0 && v_spd.abs() < 50.0 {
            sim_on_ground = 1.0;
        }
    }

    let metrics = FlightMetrics {
        latitude: lat,
        longitude: lon,
        indicated_altitude: cols[6].parse().unwrap_or(0.0),
        altimeter_setting: cols[7].parse().unwrap_or(0.0),
        gps_altitude_msl: alt_msl,
        outside_air_temp: cols[9].parse().unwrap_or(0.0),
        indicated_airspeed: cols[10].parse().unwrap_or(0.0),
        ground_speed: cols[11].parse().unwrap_or(0.0),
        vertical_speed: cols[12].parse().unwrap_or(0.0),
        pitch_angle: -cols[13].parse::<f64>().unwrap_or(0.0),
        roll_angle: -cols[14].parse::<f64>().unwrap_or(0.0),
        lateral_acceleration: cols[15].parse().unwrap_or(0.0),
        normal_acceleration: cols[16].parse().unwrap_or(0.0),
        heading: cols[17].parse().unwrap_or(0.0),
        track: cols[18].parse().unwrap_or(0.0),
        volts_1: cols[19].parse().unwrap_or(0.0),
        volts_2: cols[20].parse().unwrap_or(0.0),
        amps_1: cols[21].parse().unwrap_or(0.0),
        fuel_quantity_left: cols[22].parse().unwrap_or(0.0),
        fuel_quantity_right: cols[23].parse().unwrap_or(0.0),
        engine_1_fuel_flow: cols[24].parse().unwrap_or(0.0),
        engine_1_oil_temp: cols[25].parse().unwrap_or(0.0),
        engine_1_oil_pressure: cols[26].parse().unwrap_or(0.0),
        engine_1_manifold_pressure: cols[27].parse().unwrap_or(0.0),
        engine_1_rpm: cols[28].parse().unwrap_or(0.0),
        engine_1_percent_power: cols[29].parse().unwrap_or(0.0),
        engine_1_cht_1: cols[30].parse().unwrap_or(0.0),
        engine_1_cht_2: cols[31].parse().unwrap_or(0.0),
        engine_1_cht_3: cols[32].parse().unwrap_or(0.0),
        engine_1_cht_4: cols[33].parse().unwrap_or(0.0),
        engine_1_cht_5: cols[34].parse().unwrap_or(0.0),
        engine_1_cht_6: cols[35].parse().unwrap_or(0.0),
        engine_1_egt_1: cols[36].parse().unwrap_or(0.0),
        engine_1_egt_2: cols[37].parse().unwrap_or(0.0),
        engine_1_egt_3: cols[38].parse().unwrap_or(0.0),
        engine_1_egt_4: cols[39].parse().unwrap_or(0.0),
        engine_1_egt_5: cols[40].parse().unwrap_or(0.0),
        engine_1_egt_6: cols[41].parse().unwrap_or(0.0),
        engine_1_tit_1: cols[42].parse().unwrap_or(0.0),
        engine_1_tit_2: cols[43].parse().unwrap_or(0.0),
        gps_altitude_wgs84: cols[44].parse().unwrap_or(0.0),
        true_airspeed: cols[45].parse().unwrap_or(0.0),
        hsi_source: 0.0,
        selected_course: cols[47].parse().unwrap_or(0.0),
        nav_1_frequency: cols[48].parse().unwrap_or(0.0),
        nav_2_frequency: cols[49].parse().unwrap_or(0.0),
        com_1_frequency: cols[50].parse().unwrap_or(0.0),
        com_2_frequency: cols[51].parse().unwrap_or(0.0),
        horizontal_cdi: cols[52].parse().unwrap_or(0.0),
        vertical_cdi: cols[53].parse().unwrap_or(0.0),
        wind_speed: cols[54].parse().unwrap_or(0.0),
        wind_direction: cols[55].parse().unwrap_or(0.0),
        waypoint_distance: cols[56].parse().unwrap_or(0.0),
        waypoint_bearing: cols[57].parse().unwrap_or(0.0),
        magnetic_variation: cols[58].parse().unwrap_or(0.0),
        autopilot_active: if cols[59].to_lowercase() == "true" || cols[59] == "1" {
            1.0
        } else {
            0.0
        },
        roll_mode: 0.0,
        pitch_mode: 0.0,
        roll_command: -cols[62].parse::<f64>().unwrap_or(0.0),
        pitch_command: -cols[63].parse::<f64>().unwrap_or(0.0),
        vertical_speed_target: cols[64].parse().unwrap_or(0.0),
        is_on_ground: sim_on_ground,
        altitude_agl: 0.0,
        gforce: 1.0,
        pressure_altitude: 0.0,
        density_altitude: 0.0,
        pressurization_cabin_altitude: alt_msl,
        xp_prop_rpm: 0.0,
        xp_gear_ratio: 0.0,
        parking_brake: 0.0,
        // Light states aren't persisted in flight logs; default them.
        ..Default::default()
    };

    Some(FlightLogRow { timestamp, metrics })
}

fn save_imported_flight(
    app: &AppHandle,
    aircraft_title: &str,
    rows: Vec<FlightLogRow>,
    analyzer: &mut crate::flight_analyzer::FlightAnalyzer,
    source_path: &str,
) -> anyhow::Result<FlightSummary> {
    let app_data_dir = app.path().app_data_dir()?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());
    fs::create_dir_all(&log_dir)?;

    let first_ts = &rows
        .first()
        .ok_or_else(|| anyhow::anyhow!("Imported CSV contained no data rows"))?
        .timestamp;
    // Standardize timestamp for filename: 2026-04-20 12:34:56 -> 20260420_123456
    let filename_ts = first_ts.replace('-', "").replace(':', "").replace(' ', "_");
    let temp_filename = format!("butterlog_import_{}.db", filename_ts);
    let path = log_dir.join(&temp_filename);

    let mut conn = Connection::open(&path)?;

    // Create tables using centralized function
    init_sqlite_db(&conn)?;

    // Insert metrics using a transaction for massive performance boost
    let total_rows = rows.len();
    {
        let tx = conn.transaction().map_err(|e| anyhow::anyhow!(e))?;
        for (i, row) in rows.iter().enumerate() {
            insert_sqlite_row(&tx, &row.timestamp, &row.metrics).map_err(|e| anyhow::anyhow!(e))?;

            let current = i + 1;
            if current % 1000 == 0 || current == total_rows {
                let _ = app.emit(
                    "import-progress",
                    serde_json::json!({
                        "state": "saving",
                        "current": current,
                        "total": total_rows
                    }),
                );
            }
        }
        tx.commit().map_err(|e| anyhow::anyhow!(e))?;
    }

    // Emit finalizing state
    let _ = app.emit(
        "import-progress",
        serde_json::json!({
            "state": "finalizing",
            "current": total_rows,
            "total": total_rows
        }),
    );

    // Determine ICAOs and Names
    let (start_icao, start_name, end_icao, end_name) =
        if let Some(db) = app.try_state::<crate::airports::AirportsDatabase>() {
            // Advanced Landing Analysis for Imports
            if let Some(r_db) = app.try_state::<crate::runways::RunwaysDatabase>() {
                analyzer.finalize_landing_performance(&db, &r_db, Some(&conn));
            }

            let s_icao = analyzer.find_start_icao(&db);
            let e_icao = analyzer.find_end_icao(&db);
            let s_name = if s_icao == "Airborne" {
                "Airborne".to_string()
            } else {
                db.get_by_ident(&s_icao)
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string())
            };
            let e_name = if e_icao == "Airborne" {
                "Airborne".to_string()
            } else {
                db.get_by_ident(&e_icao)
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string())
            };
            (s_icao, s_name, e_icao, e_name)
        } else {
            (
                "XXXX".to_string(),
                "Unknown".to_string(),
                "XXXX".to_string(),
                "Unknown".to_string(),
            )
        };

    // Populate summary
    let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
    let duration_mins = analyzer.get_duration_minutes();
    let import_time = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut summary_data = vec![
        ("departure_icao", start_icao.clone()),
        ("departure_name", start_name.clone()),
        ("arrival_icao", end_icao.clone()),
        ("arrival_name", end_name.clone()),
        ("aircraft_title", aircraft_title.to_string()),
        ("max_altitude", analyzer.max_alt.to_string()),
        ("max_ground_speed", analyzer.max_gs.to_string()),
        ("fuel_consumed", fuel_consumed.to_string()),
        ("source_path", source_path.to_string()),
        ("import_timestamp", import_time),
        (
            "flight_events",
            serde_json::to_string(&analyzer.events).unwrap_or_default(),
        ),
    ];

    // Conditionally add landing metrics if found
    if let Some(landing) = analyzer.events.iter().find(|e| e.event_type == "landing") {
        if let Some(v) = landing.touchdown_fpm { summary_data.push(("touchdown_fpm", v.to_string())); }
        if let Some(v) = landing.landing_g { summary_data.push(("landing_g", v.to_string())); }
        if let Some(v) = landing.offset_percent { summary_data.push(("landing_offset_pct", v.to_string())); }
        if let Some(v) = landing.threshold_dist_ft { summary_data.push(("landing_dist_ft", v.to_string())); }
        if let Some(v) = landing.vs_variance { summary_data.push(("vs_variance", v.to_string())); }
        if let Some(v) = landing.ias_variance { summary_data.push(("ias_variance", v.to_string())); }
        if let Some(v) = landing.calculate_landing_score() { summary_data.push(("landing_score", v.to_string())); }
    }
    for (k, v) in summary_data {
        conn.execute(
            "INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)",
            params![k, v],
        )?;
    }

    let is_completed = start_icao != "Airborne" && end_icao != "Airborne" && end_icao != "Unknown";
    if let Err(e) = update_aircraft_stats(
        app,
        aircraft_title,
        duration_mins as f64,
        fuel_consumed,
        &end_icao,
        is_completed,
    ) {
        crate::append_log(app, format!("Failed to update aircraft stats: {}", e));
    }

    let (first_ts_val, last_ts_val): (String, String) = {
        let mut stmt = conn
            .prepare("SELECT MIN(timestamp), MAX(timestamp) FROM metrics")
            .map_err(|e| {
                crate::append_log(app, format!("[Logs] Database error (prepare timestamps): {}", e));
                e
            })?;
        stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| {
                crate::append_log(app, format!("[Logs] Database error (query_row timestamps): {}", e));
                e
            })?
    };

    drop(conn);

    // Scan for screenshots after DB is closed and flushed
    if let Err(e) = crate::screenshot_manager::scan_screenshots_for_flight(
        app,
        &temp_filename.replace(".db", ""),
        aircraft_title,
        &first_ts_val,
        &last_ts_val,
    ) {
        crate::append_log(app, format!("Failed to scan screenshots: {}", e));
    }

    parse_db_file(app, &path)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse the imported flight summary"))
}

// ── Track thinning, for the live track upload ─────────────────────────────────

pub fn ts_to_epoch(ts: &str) -> i64 {
    NaiveDateTime::parse_from_str(ts.split('.').next().unwrap_or(ts), "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0)
}

// Adaptive, dynamics-aware thinning, two tiers:
//  - straight and level: keep one per STEADY_SECS;
//  - everything else — climbing, descending, turning — keep one per ACTIVE_SECS,
//    which is the logger's own rate, so those phases are kept at full fidelity.
//
// "Straight and level" is judged from the aircraft's *instantaneous* state (vertical
// speed and bank angle), not from how far it has drifted since the last kept sample.
// Those are not the same thing: a steady 500 fpm climb only drifts past a 100 ft
// altitude tolerance every 12 s, so a drift-only test would thin a climb to one
// sample per 12 s instead of the per-second detail we want.
//
// The drift check is kept as a backstop on top of it, so a slow wander that never
// trips the VS/bank thresholds — or a sim reporting an unreliable vertical speed —
// still forces a sample rather than drawing a 5-minute straight line through a curve.
//
// The takeoff/landing windows are forced out of the steady tier so a rollout or
// taxi — level, wings level, but the most interesting part of the flight — is never
// thinned to one sample per STEADY_SECS.
const NEAR_SECS: i64 = 60;
const STEADY_SECS: i64 = 300;
const ACTIVE_SECS: i64 = 1;
const LEVEL_VS_FPM: f64 = 100.0;
const LEVEL_ROLL_DEG: f64 = 3.0;
const DRIFT_ALT_FT: f64 = 100.0;
const DRIFT_HDG_DEG: f64 = 10.0;

/// Smallest angular difference between two headings in degrees (0..=180).
fn hdg_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 { 360.0 - d } else { d }
}

/// The thinning policy as resumable state.
///
/// The live uploader sees the flight 20 seconds at a time and keeps one policy
/// across its batches, which matters: the decision depends on the last *kept*
/// sample, so an uploader that restarted the
/// policy on every batch would keep the first sample of every batch and hold
/// cruise at one sample per flush instead of one per STEADY_SECS.
#[derive(Default)]
pub struct Downsampler {
    started: bool,
    last_emit_epoch: i64,
    /// Altitude and heading of the last kept sample.
    ref_alt: f64,
    ref_hdg: f64,
}

impl Downsampler {
    pub fn new() -> Self {
        Self { started: false, last_emit_epoch: i64::MIN, ref_alt: 0.0, ref_hdg: 0.0 }
    }

    /// Whether to keep this sample, advancing the policy when it does.
    pub fn accept(&mut self, row: &FlightLogRow, event_epochs: &[i64]) -> bool {
        let epoch = ts_to_epoch(&row.timestamp);
        let keep = if !self.started {
            true
        } else {
            let near = event_epochs.iter().any(|&e| (epoch - e).abs() <= NEAR_SECS);
            let level = row.metrics.vertical_speed.abs() <= LEVEL_VS_FPM
                && row.metrics.roll_angle.abs() <= LEVEL_ROLL_DEG;
            let drifted = (row.metrics.indicated_altitude - self.ref_alt).abs() > DRIFT_ALT_FT
                || hdg_diff(row.metrics.heading, self.ref_hdg) > DRIFT_HDG_DEG;
            let steady = level && !drifted && !near;
            let interval = if steady { STEADY_SECS } else { ACTIVE_SECS };
            // ACTIVE_SECS == 1 also gives per-second dedup: rows sharing an epoch
            // second (the logger emits extras on force_update) collapse to one.
            epoch - self.last_emit_epoch >= interval
        };
        if keep {
            self.started = true;
            self.last_emit_epoch = epoch;
            self.ref_alt = row.metrics.indicated_altitude;
            self.ref_hdg = row.metrics.heading;
        }
        keep
    }
}

#[cfg(test)]
fn downsample(rows: &[FlightLogRow], event_epochs: &[i64]) -> Vec<usize> {
    let mut policy = Downsampler::new();
    rows.iter()
        .enumerate()
        .filter_map(|(i, row)| policy.accept(row, event_epochs).then_some(i))
        .collect()
}

fn read_remote_id(path: &std::path::Path) -> Option<i64> {
    let conn = Connection::open(path).ok()?;
    conn.query_row(
        "SELECT value FROM summary WHERE key = 'remote_id'",
        [],
        |r| r.get::<_, String>(0),
    ).ok()?.parse().ok()
}

/// The flight the service made from this log's telemetry, which the app has no id for.
#[derive(Debug, serde::Deserialize)]
struct ServiceFlight {
    id: i64,
    url: String,
    export_url: String,
}

/// Asks the service for the flight it made over this log's span (`GET /flights/covering`).
/// `None` when it made none, the app's own then being the flight, or it can't be reached.
async fn service_flight(app: &AppHandle, path: &std::path::Path) -> Option<ServiceFlight> {
    let (base_url, api_token) = app.state::<crate::config::ConfigManager>().get_config().api_auth()?;
    let (from, to) = {
        let conn = Connection::open(path).ok()?;
        let (lo, hi): (Option<String>, Option<String>) = conn
            .query_row("SELECT MIN(timestamp), MAX(timestamp) FROM metrics", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok()?;
        (ts_to_epoch(&lo?), ts_to_epoch(&hi?))
    };
    if from <= 0 || to <= from {
        return None;
    }
    let res = reqwest::Client::new()
        .get(format!("{}/flights/covering", base_url))
        .query(&[("from", from), ("to", to)])
        .bearer_auth(&api_token)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;
    match res {
        Ok(res) if res.status().is_success() => res.json::<ServiceFlight>().await.ok(),
        Ok(res) if res.status() == reqwest::StatusCode::NOT_FOUND => None,
        Ok(res) => {
            crate::append_log(app, format!("[Service] Finding the service's flight failed: {}", res.status()));
            None
        }
        Err(e) => {
            crate::append_log(app, format!("[Service] Finding the service's flight failed: {}", e));
            None
        }
    }
}

/// Writes the service's FlySto export of the flight it made to `csv_path`. `false` to
/// export the app's log instead.
async fn export_service_flight(app: &AppHandle, path: &std::path::Path, csv_path: &std::path::Path) -> bool {
    let Some(flight) = service_flight(app, path).await else { return false };
    let Some((_, api_token)) = app.state::<crate::config::ConfigManager>().get_config().api_auth() else { return false };
    let res = reqwest::Client::new()
        .get(&flight.export_url)
        .bearer_auth(&api_token)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await;
    let body = match res {
        Ok(res) if res.status().is_success() => res.bytes().await.map_err(|e| e.to_string()),
        Ok(res) => Err(res.status().to_string()),
        Err(e) => Err(e.to_string()),
    };
    match body.and_then(|b| fs::write(csv_path, &b).map_err(|e| e.to_string())) {
        Ok(()) => {
            crate::append_log(app, format!("[Export] Exported flight {} from the service", flight.id));
            true
        }
        Err(e) => {
            crate::append_log(app, format!("[Export] The service's export of flight {} failed, exporting the log: {}", flight.id, e));
            false
        }
    }
}

#[tauri::command]
pub async fn rescan_flight_screenshots(app: AppHandle, filename: String) -> Result<usize, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let log_dir = app_data_dir.join(crate::config::get_flightlogs_dir_name());
    let path = log_dir.join(&filename);
    if !path.exists() { return Err("Flight log not found".to_string()); }

    let flight_id = filename.replace(".db", "");
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;

    let (aircraft_title, start_ts, end_ts): (String, String, String) = {
        let title = conn.query_row(
            "SELECT value FROM summary WHERE key = 'aircraft_title'", [],
            |r| r.get::<_, String>(0)
        ).unwrap_or_default();
        let (s, e): (Option<String>, Option<String>) = conn
            .query_row("SELECT MIN(timestamp), MAX(timestamp) FROM metrics", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((None, None));
        (title, s.unwrap_or_default(), e.unwrap_or_default())
    };

    if start_ts.is_empty() { return Ok(0); }

    let before = {
        let mgr = app.state::<crate::screenshot_manager::ScreenshotManager>();
        mgr.get_screenshots_for_flight(&flight_id).unwrap_or_default().len()
    };

    crate::screenshot_manager::scan_screenshots_for_flight(&app, &flight_id, &aircraft_title, &start_ts, &end_ts)
        .map_err(|e| e.to_string())?;

    let after = {
        let mgr = app.state::<crate::screenshot_manager::ScreenshotManager>();
        mgr.get_screenshots_for_flight(&flight_id).unwrap_or_default().len()
    };

    Ok(after.saturating_sub(before))
}

/// The flight's page on the service, which shows all of it: the flight the app made, by
/// the id it was given, or the one the service made from the log's telemetry.
#[tauri::command]
pub async fn flight_web_url(app: AppHandle, filename: String) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = app_data_dir.join(crate::config::get_flightlogs_dir_name()).join(&filename);
    if !path.exists() {
        return Err("Flight log not found".to_string());
    }
    let (base_url, _) = app.state::<crate::config::ConfigManager>().get_config().api_auth()
        .ok_or("Not connected to ButterLog service. Log in first.")?;
    let site = base_url.trim_end_matches("/api/v0").to_string();
    if let Some(id) = read_remote_id(&path) {
        return Ok(format!("{}/content/flights/{}", site, id));
    }
    if let Some(flight) = service_flight(&app, &path).await {
        return Ok(flight.url);
    }
    // A share an older app made, whose link leads to its flight.
    let share_url: Option<String> = Connection::open(&path).ok().and_then(|conn| {
        conn.query_row("SELECT value FROM summary WHERE key = 'share_url'", [], |r| r.get::<_, String>(0)).ok()
    });
    share_url.ok_or_else(|| "This flight isn't on the web yet.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_reconnect_at_cruise_still_resumes() {
        let cruise = FlightMetrics { ground_speed: 550.0, vertical_speed: 0.0, ..Default::default() };
        // IronClaw737's 777: 78 s away and 17.6 NM on, level.
        assert!(resume_matches(17.6, 2.0, &cruise, 78));
        // Too far for the time away: another flight.
        assert!(!resume_matches(60.0, 2.0, &cruise, 78));
        // Paused, or away longer than the gap counts: only where it stopped.
        assert!(resume_matches(0.0, 0.0, &cruise, 4 * 3600));
        assert!(!resume_matches(40.0, 0.0, &cruise, 4 * 3600));
        // Descending at 2000 fpm for a minute: 2000 ft lower is the same flight.
        let descent = FlightMetrics { ground_speed: 300.0, vertical_speed: -2000.0, ..Default::default() };
        assert!(resume_matches(5.0, 2000.0, &descent, 60));
        assert!(!resume_matches(5.0, 6000.0, &descent, 60));
    }

    /// One row per second starting at epoch 0: (altitude ft, heading deg, VS fpm, roll deg).
    fn rows(samples: &[(f64, f64, f64, f64)]) -> Vec<FlightLogRow> {
        samples
            .iter()
            .enumerate()
            .map(|(i, &(alt, hdg, vs, roll))| {
                let ts = chrono::DateTime::from_timestamp(i as i64, 0)
                    .unwrap()
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string();
                let mut metrics = FlightMetrics::default();
                metrics.indicated_altitude = alt;
                metrics.heading = hdg;
                metrics.vertical_speed = vs;
                metrics.roll_angle = roll;
                FlightLogRow { timestamp: ts, metrics }
            })
            .collect()
    }

    #[test]
    fn straight_and_level_thins_to_one_per_five_minutes() {
        // 20 minutes wings-level at a held altitude: first sample plus one every 300s.
        let r = rows(&vec![(35_000.0, 90.0, 0.0, 0.0); 20 * 60]);
        assert_eq!(downsample(&r, &[]), vec![0, 300, 600, 900]);
    }

    #[test]
    fn climbing_keeps_every_second() {
        // Steady 500 fpm climb. This is the case a drift-only test gets wrong: the
        // aircraft only crosses a 100 ft tolerance every 12s, but it is plainly not
        // level, so every logged second must survive.
        let r = rows(
            &(0..600)
                .map(|i| (1_000.0 + i as f64 * 500.0 / 60.0, 90.0, 500.0, 0.0))
                .collect::<Vec<_>>(),
        );
        assert_eq!(downsample(&r, &[]).len(), 600);
    }

    #[test]
    fn banked_turn_at_constant_altitude_keeps_every_second() {
        // Level altitude and zero VS, but banked in a standard-rate turn.
        let r = rows(
            &(0..300)
                .map(|i| (5_000.0, (i as f64 * 3.0) % 360.0, 0.0, 22.0))
                .collect::<Vec<_>>(),
        );
        assert_eq!(downsample(&r, &[]).len(), 300);
    }

    #[test]
    fn shallow_drift_still_forces_a_sample() {
        // Backstop: VS and bank both read level (an unreliable-VS sim, or a wander
        // too slow to trip the thresholds) yet the altitude walks away. The drift
        // check must break the 300s tier rather than draw a straight line through it.
        let r = rows(
            &(0..600)
                .map(|i| (35_000.0 + i as f64 * 0.5, 90.0, 0.0, 0.0))
                .collect::<Vec<_>>(),
        );
        let kept = downsample(&r, &[]);
        // 0.5 ft/s crosses the 100 ft drift tolerance every ~201 samples.
        assert_eq!(&kept[..3], &[0, 201, 402]);
    }

    #[test]
    fn event_window_defeats_the_steady_tier() {
        // Rollout after touchdown: level, wings level, so the steady tier would
        // otherwise thin the most interesting part of the flight to one per 300s.
        let r = rows(&vec![(50.0, 270.0, 0.0, 0.0); 120]);
        let kept = downsample(&r, &[30]);
        // Seconds 0..=90 fall within NEAR_SECS of the event and are all kept.
        assert_eq!(&kept[..91], &(0..91).collect::<Vec<_>>()[..]);
        assert!(kept.len() < 120, "past the window it returns to the steady tier");
    }
}
