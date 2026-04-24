use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use chrono::NaiveDateTime;
use crate::config::ConfigManager;
use tauri::{AppHandle, Manager, Emitter};
use rusqlite::{Connection, Row, params};
use crate::simconnect_monitor::FlightMetrics;
use directories::UserDirs;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlightSummary {
    pub filename: String,
    pub start_icao: String,
    pub end_icao: String,
    pub start_time: String,
    pub end_time: String,
    pub duration_minutes: i64,
    pub file_size_bytes: u64,
    pub aircraft_title: String,
    pub aircraft_type: String,
    pub aircraft_model: String,
    pub max_altitude: f64,
    pub max_ground_speed: f64,
    pub fuel_consumed: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FlightLogRow {
    pub timestamp: String,
    pub metrics: FlightMetrics,
}

#[tauri::command]
pub async fn get_flight_data(app: AppHandle, filename: String) -> Result<Vec<FlightLogRow>, String> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let log_dir = app_data_dir.join("flightlogs");

    let path = log_dir.join(filename);
    if !path.exists() {
        return Err("File not found".to_string());
    }

    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT * FROM metrics ORDER BY timestamp ASC").map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map([], |row| {
        Ok(FlightLogRow {
            timestamp: row.get(0)?,
            metrics: map_row_to_metrics(row)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }

    Ok(result)
}

fn map_row_to_metrics(row: &Row) -> rusqlite::Result<FlightMetrics> {
    Ok(FlightMetrics {
        latitude: row.get(1)?, longitude: row.get(2)?, alt_b: row.get(3)?, baro_a: row.get(4)?, alt_msl: row.get(5)?, oat: row.get(6)?,
        ias: row.get(7)?, gnd_spd: row.get(8)?, v_spd: row.get(9)?, pitch: row.get(10)?, roll: row.get(11)?, lat_ac: row.get(12)?, norm_ac: row.get(13)?,
        hdg: row.get(14)?, trk: row.get(15)?, volt1: row.get(16)?, volt2: row.get(17)?, amp1: row.get(18)?, f_qty_l: row.get(19)?, f_qty_r: row.get(20)?,
        e1_fflow: row.get(21)?, e1_oil_t: row.get(22)?, e1_oil_p: row.get(23)?, e1_map: row.get(24)?, e1_rpm: row.get(25)?, e1_pwr: row.get(26)?,
        e1_cht1: row.get(27)?, e1_cht2: row.get(28)?, e1_cht3: row.get(29)?, e1_cht4: row.get(30)?, e1_cht5: row.get(31)?, e1_cht6: row.get(32)?,
        e1_egt1: row.get(33)?, e1_egt2: row.get(34)?, e1_egt3: row.get(35)?, e1_egt4: row.get(36)?, e1_egt5: row.get(37)?, e1_egt6: row.get(38)?,
        e1_tit1: row.get(39)?, e1_tit2: row.get(40)?, alt_gps: row.get(41)?, tas: row.get(42)?, hsis: row.get(43)?, crs: row.get(44)?, nav1: row.get(45)?,
        nav2: row.get(46)?, com1: row.get(47)?, com2: row.get(48)?, hcdi: row.get(49)?, vcdi: row.get(50)?, wnd_spd: row.get(51)?, wnd_dr: row.get(52)?,
        wpt_dst: row.get(53)?, wpt_brg: row.get(54)?, mag_var: row.get(55)?, afcs_on: row.get(56)?, roll_m: row.get(57)?, pitch_m: row.get(58)?,
        roll_c: row.get(59)?, pitch_c: row.get(60)?, v_spd_g: row.get(61)?, gps_fix: row.get(62)?, hal: row.get(63)?, val: row.get(64)?,
        hpl_was: row.get(65)?, hpl_fd: row.get(66)?, vpl_was: row.get(67)?, sim_on_ground: row.get(68)?,
    })
}

pub fn scan_logs(app: AppHandle) -> Result<Vec<FlightSummary>, String> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let log_dir = app_data_dir.join("flightlogs");

    if !log_dir.exists() {
        return Ok(Vec::new());
    }

    let mut summaries = Vec::new();
    let entries = fs::read_dir(log_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("db") {
                if let Some(summary) = parse_db_file(&path) {
                    summaries.push(summary);
                }
            }
        }
    }

    // Sort by start time descending
    summaries.sort_by(|a, b| b.start_time.cmp(&a.start_time));

    Ok(summaries)
}

fn parse_db_file(path: &PathBuf) -> Option<FlightSummary> {
    let filename = path.file_name()?.to_str()?.to_string();
    let metadata = fs::metadata(path).ok()?;
    
    let conn = Connection::open(path).ok()?;
    
    let get_summary = |key: &str| -> String {
        conn.query_row("SELECT value FROM summary WHERE key = ?1", params![key], |r| r.get::<_, String>(0))
            .unwrap_or_else(|_| "Unknown".to_string())
    };

    let start_icao = get_summary("departure_icao");
    let end_icao = get_summary("arrival_icao");
    let aircraft_title = get_summary("aircraft_title");
    let aircraft_type = get_summary("aircraft_type");
    let aircraft_model = get_summary("aircraft_model");
    let max_altitude = get_summary("max_altitude").parse().unwrap_or(0.0);
    let max_ground_speed = get_summary("max_ground_speed").parse().unwrap_or(0.0);
    let fuel_consumed = get_summary("fuel_consumed").parse().unwrap_or(0.0);

    let mut stmt = conn.prepare("SELECT MIN(timestamp), MAX(timestamp) FROM metrics").ok()?;
    let (start_time, end_time): (String, String) = stmt.query_row([], |row| {
        Ok((row.get(0)?, row.get(1)?))
    }).ok()?;
    
    let start_dt = NaiveDateTime::parse_from_str(&start_time, "%Y-%m-%d %H:%M:%S").ok()?;
    let end_dt = NaiveDateTime::parse_from_str(&end_time, "%Y-%m-%d %H:%M:%S").ok()?;
    
    let duration = end_dt.signed_duration_since(start_dt);

    Some(FlightSummary {
        filename,
        start_icao,
        end_icao,
        start_time,
        end_time,
        duration_minutes: duration.num_minutes(),
        file_size_bytes: metadata.len(),
        aircraft_title,
        aircraft_type,
        aircraft_model,
        max_altitude,
        max_ground_speed,
        fuel_consumed,
    })
}

#[tauri::command]
pub async fn export_flight_to_csv(app: AppHandle, filename: String) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().unwrap();
    let internal_log_dir = app_data_dir.join("flightlogs");

    let config = app.state::<ConfigManager>().get_config();
    let export_dir = config.log_directory.clone().unwrap_or_else(|| {
        UserDirs::new().unwrap().document_dir().unwrap().join("butterlog")
    });
    
    if !export_dir.exists() {
        fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;
    }

    let db_path = internal_log_dir.join(&filename);
    if !db_path.exists() {
        return Err("Database file not found".to_string());
    }

    let csv_filename = filename.replace(".db", ".csv");
    let csv_path = export_dir.join(&csv_filename);
    
    // Get aircraft info from summary table
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let airframe_name = conn.query_row("SELECT value FROM summary WHERE key = 'aircraft_title'", [], |r| r.get::<_, String>(0))
        .unwrap_or_else(|_| "Simulated Aircraft".to_string());

    let data = get_flight_data(app, filename).await?;
    
    use std::io::Write;
    let mut file = fs::File::create(&csv_path).map_err(|e| e.to_string())?;

    // Write header from FORMATS.md
    writeln!(file, "#airframe_info, log_version=\"1.00\", airframe_name=\"{}\", unit_software_part_number=\"006-BXXX9-DE\", unit_software_version=\"15.24\", system_software_part_number=\"006-BXXXX-37\", system_id=\"25XXXX67\", mode=NORMAL, simulator_id=\"ButterLogV2\",", airframe_name).map_err(|e| e.to_string())?;
    writeln!(file, "#yyy-mm-dd, hh:mm:ss,   hh:mm,  ident,      degrees,      degrees, ft Baro,  inch,  ft msl, deg C,     kt,     kt,     fpm,    deg,    deg,      G,      G,   deg,   deg, volts, volts,  amps,   gals,   gals,      gph,   deg F,     psi,     Hg,    rpm,       %,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,   deg F,  ft wgs,  kt, enum,    deg,    MHz,    MHz,     MHz,     MHz,    fsd,    fsd,     kt,   deg,     nm,    deg,    deg,   bool,  enum,   enum,   deg,   deg,   fpm,   enum,   mt,    mt,     mt,    mt,     mt").map_err(|e| e.to_string())?;
    writeln!(file, "Lcl Date, Lcl Time, UTCOfst, AtvWpt,     Latitude,    Longitude,    AltB, BaroA,  AltMSL,   OAT,    IAS, GndSpd,    VSpd,  Pitch,   Roll,  LatAc, NormAc,   HDG,   TRK, volt1, volt2,  amp1,  FQtyL,  FQtyR, E1 FFlow, E1 OilT, E1 OilP, E1 MAP, E1 RPM, E1 %Pwr, E1 CHT1, E1 CHT2, E1 CHT3, E1 CHT4, E1 CHT5, E1 CHT6, E1 EGT1, E1 EGT2, E1 EGT3, E1 EGT4, E1 EGT5, E1 EGT6, E1 TIT1, E1 TIT2,  AltGPS, TAS, HSIS,    CRS,   NAV1,   NAV2,    COM1,    COM2,   HCDI,   VCDI,WndSpd,WndDr, WptDst, WptBrg, MagVar, AfcsOn, RollM, PitchM, RollC, PichC, VSpdG, GPSfix,  HAL,   VAL, HPLwas, HPLfd, VPLwas").map_err(|e| e.to_string())?;

    for row in data {
        let ts_parts: Vec<&str> = row.timestamp.split(' ').collect();
        let date = ts_parts[0];
        let time = ts_parts[1];
        let m = row.metrics;

        let utc_offset = "+00:00"; 

        writeln!(file, "{}, {}, {}, {}, {:.6}, {:.6}, {:.1}, {:.2}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.3}, {:.3}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.2}, {:.2}, {:.1}, {:.1}, {:.1}, {:.2}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}, {}, {:.0}, {:.3}, {:.3}, {:.3}, {:.3}, {:.2}, {:.2}, {:.1}, {:.1}, {:.2}, {:.1}, {:.2}, {}, {}, {}, {:.1}, {:.1}, {:.1}, {}, {:.1}, {:.1}, {:.1}, {:.1}, {:.1}",
            date, time, utc_offset, "", m.latitude, m.longitude, m.alt_b, m.baro_a, m.alt_msl, m.oat,
            m.ias, m.gnd_spd, m.v_spd, m.pitch, m.roll, m.lat_ac, m.norm_ac,
            m.hdg, m.trk, m.volt1, m.volt2, m.amp1, m.f_qty_l, m.f_qty_r,
            m.e1_fflow, m.e1_oil_t, m.e1_oil_p, m.e1_map, m.e1_rpm, m.e1_pwr,
            m.e1_cht1, m.e1_cht2, m.e1_cht3, m.e1_cht4, m.e1_cht5, m.e1_cht6,
            m.e1_egt1, m.e1_egt2, m.e1_egt3, m.e1_egt4, m.e1_egt5, m.e1_egt6,
            m.e1_tit1, m.e1_tit2, m.alt_gps, m.tas, "GPS", m.crs,
            m.nav1, m.nav2, m.com1, m.com2, m.hcdi, m.vcdi, m.wnd_spd, m.wnd_dr,
            m.wpt_dst, m.wpt_brg, m.mag_var, if m.afcs_on > 0.5 { "1" } else { "0" },
            "NONE", "NONE", m.roll_c, m.pitch_c, m.v_spd_g, "3DDiff",
            m.hal, m.val, m.hpl_was, m.hpl_fd, m.vpl_was
        ).map_err(|e| e.to_string())?;
    }

    Ok(csv_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn import_flight_from_csv(app: AppHandle, path: String) -> Result<FlightSummary, String> {
    crate::append_log(&app, format!("Starting import of flight log from: {}", path));
    
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Err("File is empty".to_string());
    }

    let airframe_name = parse_airframe_name(&lines);
    crate::append_log(&app, format!("Detected airframe: {}", airframe_name));

    let mut rows = Vec::new();
    let mut analyzer = crate::flight_analyzer::FlightAnalyzer::new();

    for line in lines {
        if line.starts_with('#') || line.starts_with("Lcl Date") || line.is_empty() {
            continue;
        }

        if let Some(row) = parse_csv_line_to_row(line) {
            analyzer.update(&row.metrics);
            rows.push(row);
        }
    }

    if rows.is_empty() {
        return Err("No valid data points found in CSV".to_string());
    }

    crate::append_log(&app, format!("Successfully parsed {} data points. Saving to internal database...", rows.len()));

    let summary = save_imported_flight(&app, &airframe_name, rows, &analyzer, &path).map_err(|e| e.to_string())?;
    
    crate::append_log(&app, format!("Import complete. Identified route: {} -> {}", summary.start_icao, summary.end_icao));
    
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

fn parse_csv_line_to_row(line: &str) -> Option<FlightLogRow> {
    let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if cols.len() < 70 { return None; }

    let timestamp = format!("{} {}", cols[0], cols[1]);
    
    let metrics = FlightMetrics {
        latitude: cols[4].parse().unwrap_or(0.0),
        longitude: cols[5].parse().unwrap_or(0.0),
        alt_b: cols[6].parse().unwrap_or(0.0),
        baro_a: cols[7].parse().unwrap_or(0.0),
        alt_msl: cols[8].parse().unwrap_or(0.0),
        oat: cols[9].parse().unwrap_or(0.0),
        ias: cols[10].parse().unwrap_or(0.0),
        gnd_spd: cols[11].parse().unwrap_or(0.0),
        v_spd: cols[12].parse().unwrap_or(0.0),
        pitch: cols[13].parse().unwrap_or(0.0),
        roll: cols[14].parse().unwrap_or(0.0),
        lat_ac: cols[15].parse().unwrap_or(0.0),
        norm_ac: cols[16].parse().unwrap_or(0.0),
        hdg: cols[17].parse().unwrap_or(0.0),
        trk: cols[18].parse().unwrap_or(0.0),
        volt1: cols[19].parse().unwrap_or(0.0),
        volt2: cols[20].parse().unwrap_or(0.0),
        amp1: cols[21].parse().unwrap_or(0.0),
        f_qty_l: cols[22].parse().unwrap_or(0.0),
        f_qty_r: cols[23].parse().unwrap_or(0.0),
        e1_fflow: cols[24].parse().unwrap_or(0.0),
        e1_oil_t: cols[25].parse().unwrap_or(0.0),
        e1_oil_p: cols[26].parse().unwrap_or(0.0),
        e1_map: cols[27].parse().unwrap_or(0.0),
        e1_rpm: cols[28].parse().unwrap_or(0.0),
        e1_pwr: cols[29].parse().unwrap_or(0.0),
        e1_cht1: cols[30].parse().unwrap_or(0.0),
        e1_cht2: cols[31].parse().unwrap_or(0.0),
        e1_cht3: cols[32].parse().unwrap_or(0.0),
        e1_cht4: cols[33].parse().unwrap_or(0.0),
        e1_cht5: cols[34].parse().unwrap_or(0.0),
        e1_cht6: cols[35].parse().unwrap_or(0.0),
        e1_egt1: cols[36].parse().unwrap_or(0.0),
        e1_egt2: cols[37].parse().unwrap_or(0.0),
        e1_egt3: cols[38].parse().unwrap_or(0.0),
        e1_egt4: cols[39].parse().unwrap_or(0.0),
        e1_egt5: cols[40].parse().unwrap_or(0.0),
        e1_egt6: cols[41].parse().unwrap_or(0.0),
        e1_tit1: cols[42].parse().unwrap_or(0.0),
        e1_tit2: cols[43].parse().unwrap_or(0.0),
        alt_gps: cols[44].parse().unwrap_or(0.0),
        tas: cols[45].parse().unwrap_or(0.0),
        hsis: 0.0, 
        crs: cols[47].parse().unwrap_or(0.0),
        nav1: cols[48].parse().unwrap_or(0.0),
        nav2: cols[49].parse().unwrap_or(0.0),
        com1: cols[50].parse().unwrap_or(0.0),
        com2: cols[51].parse().unwrap_or(0.0),
        hcdi: cols[52].parse().unwrap_or(0.0),
        vcdi: cols[53].parse().unwrap_or(0.0),
        wnd_spd: cols[54].parse().unwrap_or(0.0),
        wnd_dr: cols[55].parse().unwrap_or(0.0),
        wpt_dst: cols[56].parse().unwrap_or(0.0),
        wpt_brg: cols[57].parse().unwrap_or(0.0),
        mag_var: cols[58].parse().unwrap_or(0.0),
        afcs_on: if cols[59].to_lowercase() == "true" || cols[59] == "1" { 1.0 } else { 0.0 },
        roll_m: 0.0,
        pitch_m: 0.0,
        roll_c: cols[62].parse().unwrap_or(0.0),
        pitch_c: cols[63].parse().unwrap_or(0.0),
        v_spd_g: cols[64].parse().unwrap_or(0.0),
        gps_fix: 0.0,
        hal: cols[66].parse().unwrap_or(0.0),
        val: cols[67].parse().unwrap_or(0.0),
        hpl_was: cols[68].parse().unwrap_or(0.0),
        hpl_fd: cols[69].parse().unwrap_or(0.0),
        vpl_was: cols[70].parse().unwrap_or(0.0),
        sim_on_ground: if cols[8].parse::<f64>().unwrap_or(0.0) < 500.0 { 1.0 } else { 0.0 },
    };

    Some(FlightLogRow { timestamp, metrics })
}

fn save_imported_flight(app: &AppHandle, aircraft_title: &str, rows: Vec<FlightLogRow>, analyzer: &crate::flight_analyzer::FlightAnalyzer, source_path: &str) -> anyhow::Result<FlightSummary> {
    let app_data_dir = app.path().app_data_dir()?;
    let log_dir = app_data_dir.join("flightlogs");
    fs::create_dir_all(&log_dir)?;

    let first_ts = &rows.first().unwrap().timestamp;
    // Standardize timestamp for filename: 2026-04-20 12:34:56 -> 20260420_123456
    let filename_ts = first_ts.replace('-', "").replace(':', "").replace(' ', "_");
    let temp_filename = format!("butterlog_import_{}.db", filename_ts);
    let path = log_dir.join(&temp_filename);

    let conn = Connection::open(&path)?;
    
    // Create tables
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

    // Insert metrics
    {
        let mut stmt = conn.prepare("INSERT OR REPLACE INTO metrics VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54, ?55, ?56, ?57, ?58, ?59, ?60, ?61, ?62, ?63, ?64, ?65, ?66, ?67, ?68, ?69)")?;
        
        for row in rows {
            let m = row.metrics;
            stmt.execute(params![
                row.timestamp, m.latitude, m.longitude, m.alt_b, m.baro_a, m.alt_msl, m.oat,
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
            ])?;
        }
    }

    // Determine ICAOs
    let (start_icao, end_icao) = if let Some(db) = app.try_state::<crate::airports::AirportsDatabase>() {
        (analyzer.find_start_icao(&db), analyzer.find_end_icao(&db))
    } else {
        ("XXXX".to_string(), "XXXX".to_string())
    };

    // Populate summary
    let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
    let import_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let summary_data = [
        ("departure_icao", start_icao.clone()),
        ("arrival_icao", end_icao.clone()),
        ("aircraft_title", aircraft_title.to_string()),
        ("aircraft_type", "Imported".to_string()),
        ("aircraft_model", "Imported".to_string()),
        ("max_altitude", analyzer.max_alt.to_string()),
        ("max_ground_speed", analyzer.max_gs.to_string()),
        ("fuel_consumed", fuel_consumed.to_string()),
        ("source_path", source_path.to_string()),
        ("import_timestamp", import_time),
    ];

    for (k, v) in summary_data {
        conn.execute("INSERT INTO summary (key, value) VALUES (?1, ?2)", params![k, v])?;
    }

    drop(conn);

    // Rename file
    let final_filename = format!("butterlog_{}_{}_{}.db", start_icao, end_icao, filename_ts);
    let final_path = log_dir.join(&final_filename);
    fs::rename(&path, &final_path)?;

    Ok(parse_db_file(&final_path).unwrap())
}
