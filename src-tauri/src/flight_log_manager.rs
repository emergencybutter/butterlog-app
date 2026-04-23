use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use chrono::NaiveDateTime;
use crate::config::ConfigManager;
use tauri::{AppHandle, Manager};
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
