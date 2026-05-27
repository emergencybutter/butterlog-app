use crate::airports::AirportsDatabase;
use crate::flight_log_manager::{init_sqlite_db, insert_sqlite_row};
use crate::models::{AircraftInfo, FlightMetrics, WebhookFlightSummary, AirportInfo, ClosestAirportInfo};
use crate::sim_monitor::SimMonitor;
use crate::webhook_manager::WebhookManager;
use crate::runways::RunwaysDatabase;
use chrono::Utc;
use futures_util::{StreamExt, SinkExt};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;

fn decode_base64(s: &str) -> Option<Vec<u8>> {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut buffer = Vec::new();
    let mut temp = 0u32;
    let mut bits = 0;
    for &byte in s.as_bytes() {
        if byte == b'=' {
            break;
        }
        if let Some(pos) = CHARSET.iter().position(|&c| c == byte) {
            temp = (temp << 6) | (pos as u32);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                buffer.push((temp >> bits) as u8);
            }
        }
    }
    if buffer.is_empty() { None } else { Some(buffer) }
}

fn extract_xplane_string(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let decoded = decode_base64(s)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_else(|| s.to_string());
        Some(decoded.split('\0').next().unwrap_or("").trim().to_string())
    } else if let Some(arr) = value.as_array() {
        let bytes: Vec<u8> = arr.iter()
            .filter_map(|v| v.as_u64().map(|n| n as u8))
            .collect();
        if bytes.is_empty() {
            None
        } else {
            let s = String::from_utf8_lossy(&bytes);
            Some(s.split('\0').next().unwrap_or("").trim().to_string())
        }
    } else {
        None
    }
}

async fn fetch_xplane_dataref_string(client: &reqwest::Client, rest_url: &str, name: &str) -> Option<String> {
    let resp = client.get(rest_url).query(&[("filter[name]", name)]).send().await.ok()?;
    let json = resp.json::<Value>().await.ok()?;
    let val_ref = json["data"].as_array()?.first()?.get("value")?;
    extract_xplane_string(val_ref)
}

async fn fetch_xplane_dataref_double(client: &reqwest::Client, rest_url: &str, name: &str) -> Option<f64> {
    let resp = client.get(rest_url).query(&[("filter[name]", name)]).send().await.ok()?;
    let json = resp.json::<Value>().await.ok()?;
    let val_ref = json["data"].as_array()?.first()?.get("value")?;
    if let Some(arr) = val_ref.as_array() {
        arr.first()?.as_f64()
    } else {
        val_ref.as_f64()
    }
}


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

    async fn run_monitor_async(
        app: AppHandle,
        metrics_mutex: Arc<Mutex<FlightMetrics>>,
        aircraft_info_mutex: Arc<Mutex<AircraftInfo>>,
        current_flight_id_mutex: Arc<Mutex<String>>,
        running: Arc<Mutex<bool>>,
        connected: Arc<Mutex<bool>>,
        monitoring: Arc<Mutex<bool>>,
    ) -> anyhow::Result<()> {
        let base_host = "localhost:8086";
        let rest_url = format!("http://{}/api/v3/datarefs", base_host);
        let ws_url = format!("ws://{}/api/v3", base_host);

        // Required dataref paths for flight tracking
        let paths = vec![
            "sim/flightmodel/position/latitude",
            "sim/flightmodel/position/longitude",
            "sim/flightmodel/position/elevation",
            "sim/flightmodel/position/indicated_airspeed",
            "sim/flightmodel/position/groundspeed",
            "sim/flightmodel/position/vh_ind",
            "sim/flightmodel/failures/onground_any",
            "sim/flightmodel/position/y_agl",
            "sim/flightmodel/position/mag_psi",
            "sim/cockpit2/fuel/fuel_quantity",
            "sim/flightmodel/forces/g_nrm",
            "sim/aircraft/view/acf_ui_name",
            "sim/cockpit2/gauges/indicators/altitude_ft_pilot",
            "sim/cockpit/misc/barometer_setting",
            "sim/weather/temperature_ambient_c",
            "sim/flightmodel/position/theta",
            "sim/flightmodel/position/phi",
            "sim/flightmodel/forces/g_side",
            "sim/flightmodel/position/hpath",
            "sim/cockpit2/electrical/bus_volts",
            "sim/cockpit2/electrical/generator_amps",
            "sim/cockpit2/engine/indicators/fuel_flow_kg_sec",
            "sim/cockpit2/engine/indicators/oil_temp_deg_C",
            "sim/cockpit2/engine/indicators/oil_pressure_psi",
            "sim/cockpit2/engine/indicators/MP_in_hg",
            "sim/cockpit2/engine/indicators/engine_speed_rpm",
            "sim/cockpit2/engine/indicators/power_pct",
            "sim/cockpit2/engine/indicators/CHT_deg_C",
            "sim/cockpit2/engine/indicators/EGT_deg_C",
            "sim/cockpit2/autopilot/autopilot_on",
            "sim/cockpit2/autopilot/sync_hold_pitch_deg",
            "sim/cockpit2/autopilot/sync_hold_roll_deg",
            "sim/cockpit2/autopilot/vvi_dial_fpm",
        ];

        // 1. Discovery Phase: Fetch session-specific IDs via REST discovery
        let client = reqwest::Client::new();
        let mut path_to_id = std::collections::HashMap::new();
        
        crate::append_log(&app, format!("[X-Plane] Starting dataref discovery via REST..."));
        
        for path in &paths {
            let resp = match client.get(&rest_url).query(&[("filter[name]", *path)]).send().await {
                Ok(r) => r.json::<Value>().await?,
                Err(_e) => {
                    // This floods the logs when xplane is not running.
                    // crate::append_log(&app, format!("[X-Plane] Discovery failed for {}: {}", path, e));
                    continue;
                }
            };

            if let Some(data) = resp["data"].as_array() {
                if let Some(item) = data.first() {
                    if let Some(id) = item["id"].as_i64() {
                        path_to_id.insert(path.to_string(), id.to_string());
                    }
                }
            }
        }

        if path_to_id.is_empty() {
            // This floods the logs when xplane is not running.
            // crate::append_log(&app, "[X-Plane] ERROR: No dataref IDs discovered. Connection aborted.".to_string());
            return Err(anyhow::anyhow!("No datarefs discovered. Is X-Plane running?"));
        }

        crate::append_log(&app, format!("[X-Plane] Discovered {}/{} dataref IDs.", path_to_id.len(), paths.len()));
        for path in &paths {
            if let Some(id) = path_to_id.get(*path) {
                if path.contains("groundspeed") || path.contains("onground") {
                    crate::append_log(&app, format!("[X-Plane] Map: {} -> {}", path, id));
                }
            }
        }

        // 2. Connection Phase: Open WebSocket
        let (mut ws_stream, _) = match connect_async(&ws_url).await {
            Ok(s) => s,
            Err(e) => {
                crate::append_log(&app, format!("[X-Plane] WebSocket connection failed: {}", e));
                return Err(anyhow::anyhow!("Connection failed"));
            }
        };

        { let mut c = connected.lock().unwrap(); *c = true; }
        crate::append_log(&app, "[X-Plane] Connected to WebSocket API.".to_string());

        // 3. Subscription Phase: Send dataref_subscribe_values
        let mut sub_datarefs = Vec::new();
        for id_str in path_to_id.values() {
            if let Ok(id) = id_str.parse::<i64>() {
                sub_datarefs.push(serde_json::json!({ "id": id }));
            }
        }
        
        let subscribe_msg = serde_json::json!({
            "req_id": 1,
            "type": "dataref_subscribe_values",
            "params": {
                "datarefs": sub_datarefs
            }
        });

        if let Err(e) = ws_stream.send(Message::Text(subscribe_msg.to_string().into())).await {
            crate::append_log(&app, format!("[X-Plane] Subscription request failed: {}", e));
            return Err(anyhow::anyhow!("Subscription failed"));
        }

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

        let mut on_ground_since: Option<std::time::Instant> = None;
        let mut stationary_since: Option<std::time::Instant> = None;
        let mut last_agl = 0.0;
        let mut touchdown_time: Option<std::time::Instant> = None;
        let mut touchdown_update_done = false;
        let mut auto_finalized = false;

        let webhook_manager = app.state::<WebhookManager>();
        webhook_manager.reset();

        let mut last_debug_log = std::time::Instant::now();
        let mut m = FlightMetrics::default();
        let mut last_pos: Option<(f64, f64)> = None;
        let mut last_known_title = String::new();

        // 4. Processing Phase: Handle dataref_update_values (partial updates)
        while let Some(msg) = ws_stream.next().await {
            if !*running.lock().unwrap() { break; }

            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(data) = serde_json::from_str::<Value>(&text) {
                        if data["type"] != "dataref_update_values" { continue; }
                        let data_values = &data["data"];
                        let mut updated = false;

                        let get_path_val = |path: &str| -> Option<&Value> {
                            path_to_id.get(path).and_then(|id| data_values.get(id))
                        };

                        let get_path_double_idx = |path: &str, idx: usize| -> Option<f64> {
                            get_path_val(path).and_then(|v| {
                                if let Some(arr) = v.as_array() {
                                    arr.get(idx).and_then(|x| x.as_f64())
                                } else if idx == 0 {
                                    v.as_f64()
                                } else {
                                    None
                                }
                            })
                        };

                        let get_path_double = |path: &str| -> Option<f64> {
                            get_path_double_idx(path, 0)
                        };

                        if let Some(v) = get_path_double("sim/flightmodel/position/latitude") { m.latitude = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/longitude") { m.longitude = v; updated = true; }
                        
                        // Check for teleportation (> 1nm jump)
                        if updated {
                            if let Some((l_lat, l_lon)) = last_pos {
                                let d_lat = (m.latitude - l_lat).abs() * 60.0; // 1 deg lat = 60nm
                                let d_lon = (m.longitude - l_lon).abs() * 60.0 * l_lat.to_radians().cos();
                                let dist_sq = d_lat * d_lat + d_lon * d_lon;
                                
                                if dist_sq > 1.0 && flight_ongoing {
                                    crate::append_log(&app, format!("[X-Plane] Position jump detected ({:.2}nm). Resetting flight.", dist_sq.sqrt()));
                                    ws_stream.close(None).await.ok();
                                    return Ok(()); // Loop in start() will restart discovery and new flight
                                }
                            }
                            last_pos = Some((m.latitude, m.longitude));
                        }

                        if let Some(v) = get_path_double("sim/flightmodel/position/elevation") { m.gps_altitude_msl = v * 3.28084; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/indicated_airspeed") { m.indicated_airspeed = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/groundspeed") { m.ground_speed = v * 1.94384; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/vh_ind") { m.vertical_speed = v * 196.85; updated = true; }
                        
                        if let Some(val) = get_path_val("sim/flightmodel/failures/onground_any") {
                            let val_to_check = if let Some(arr) = val.as_array() {
                                arr.first()
                            } else {
                                Some(val)
                            };
                            if let Some(v) = val_to_check {
                                if let Some(b) = v.as_bool() {
                                    m.is_on_ground = if b { 1.0 } else { 0.0 };
                                    updated = true;
                                } else if let Some(f) = v.as_f64() {
                                    m.is_on_ground = if f > 0.5 { 1.0 } else { 0.0 };
                                    updated = true;
                                }
                            }
                        }
                        
                        if let Some(v) = get_path_double("sim/flightmodel/position/y_agl") { m.altitude_agl = v * 3.28084; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/mag_psi") { m.heading = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/fuel/fuel_quantity", 0) { m.fuel_quantity_left = v * 2.20462 * 0.1498; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/fuel/fuel_quantity", 1) { m.fuel_quantity_right = v * 2.20462 * 0.1498; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/forces/g_nrm") { m.normal_acceleration = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/gauges/indicators/altitude_ft_pilot") { m.indicated_altitude = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit/misc/barometer_setting") { m.altimeter_setting = v; updated = true; }
                        if let Some(v) = get_path_double("sim/weather/temperature_ambient_c") { m.outside_air_temp = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/theta") { m.pitch_angle = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/phi") { m.roll_angle = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/forces/g_side") { m.lateral_acceleration = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/hpath") { m.track = v; updated = true; }

                        // Electrical
                        if let Some(v) = get_path_double_idx("sim/cockpit2/electrical/bus_volts", 0) { m.volts_1 = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/electrical/bus_volts", 1) { m.volts_2 = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/electrical/generator_amps", 0) { m.amps_1 = v; updated = true; }

                        // Engine
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/fuel_flow_kg_sec", 0) { m.engine_1_fuel_flow = v * 3600.0 * 2.20462 * 0.1498; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/oil_temp_deg_C", 0) { m.engine_1_oil_temp = v * 1.8 + 32.0; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/oil_pressure_psi", 0) { m.engine_1_oil_pressure = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/MP_in_hg", 0) { m.engine_1_manifold_pressure = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/engine_speed_rpm", 0) { m.engine_1_rpm = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/power_pct", 0) { m.engine_1_percent_power = v; updated = true; }

                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/CHT_deg_C", 0) {
                            let temp_f = v * 1.8 + 32.0;
                            m.engine_1_cht_1 = temp_f;
                            m.engine_1_cht_2 = temp_f;
                            m.engine_1_cht_3 = temp_f;
                            m.engine_1_cht_4 = temp_f;
                            m.engine_1_cht_5 = temp_f;
                            m.engine_1_cht_6 = temp_f;
                            updated = true;
                        }

                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/EGT_deg_C", 0) {
                            let temp_f = v * 1.8 + 32.0;
                            m.engine_1_egt_1 = temp_f;
                            m.engine_1_egt_2 = temp_f;
                            m.engine_1_egt_3 = temp_f;
                            m.engine_1_egt_4 = temp_f;
                            m.engine_1_egt_5 = temp_f;
                            m.engine_1_egt_6 = temp_f;
                            updated = true;
                        }

                        // Autopilot
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/autopilot_on") {
                            m.autopilot_active = if (v - 2.0).abs() < 0.1 { 1.0 } else { 0.0 };
                            updated = true;
                        }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/sync_hold_pitch_deg") { m.pitch_command = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/sync_hold_roll_deg") { m.roll_command = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/vvi_dial_fpm") { m.vertical_speed_target = v; updated = true; }

                        if !updated { continue; }

                        if last_debug_log.elapsed().as_secs() >= 10 {
                            crate::append_log(&app, format!("[X-Plane] Debug: GS={:.1}, OnGround={}, Lat={:.4}, Lon={:.4}", 
                                m.ground_speed, m.is_on_ground > 0.5, m.latitude, m.longitude));
                            last_debug_log = std::time::Instant::now();
                        }

                        { let mut metrics_lock = metrics_mutex.lock().unwrap(); *metrics_lock = m; }

                        if !flight_ongoing && (m.is_on_ground < 0.5 || m.ground_speed > 10.0) {
                            if m.is_on_ground > 0.5 {
                                crate::append_log(&app, "[X-Plane] Aircraft movement detected on ground (GS > 10.0). Starting fallback flight log.".to_string());
                            }
                            flight_ongoing = true;
                            { let mut mon = monitoring.lock().unwrap(); *mon = true; }
                            db_conn = None;
                            analyzer.reset();
                            webhook_manager.reset();
                            max_metrics = None;
                            start_time = Some(Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string());

                            // Fetch aircraft title once via REST (strings often not subscribable)
                            let actual_title = fetch_xplane_dataref_string(&client, &rest_url, "sim/aircraft/view/acf_ui_name").await.unwrap_or_default();
                            let actual_atc_model = fetch_xplane_dataref_string(&client, &rest_url, "sim/aircraft/view/acf_ICAO").await.unwrap_or_default();
                            let actual_atc_id = fetch_xplane_dataref_string(&client, &rest_url, "sim/aircraft/view/acf_tailnum").await.unwrap_or_default();

                            let class_val = fetch_xplane_dataref_double(&client, &rest_url, "sim/aircraft/view/acf_class").await.unwrap_or(0.0) as i32;
                            let (object_class, category) = match class_val {
                                1 => ("helicopter".to_string(), "helicopter".to_string()),
                                2 => ("glider".to_string(), "glider".to_string()),
                                _ => ("airplane".to_string(), "airplane".to_string()),
                            };

                            let num_engines = fetch_xplane_dataref_double(&client, &rest_url, "sim/aircraft/engine/acf_num_engines").await.unwrap_or(1.0) as i32;

                            let en_type_val = fetch_xplane_dataref_double(&client, &rest_url, "sim/aircraft/prop/acf_en_type").await.unwrap_or(0.0) as i32;
                            let engine_type = match en_type_val {
                                0 => "piston".to_string(),
                                1 => "turboprop".to_string(),
                                2 => "jet".to_string(),
                                _ => "unknown".to_string(),
                            };

                            if !actual_title.is_empty() {
                                // Reset if aircraft name changes mid-flight
                                if !last_known_title.is_empty() && last_known_title != actual_title {
                                    crate::append_log(&app, format!("[X-Plane] Aircraft changed ({} -> {}). Resetting flight.", last_known_title, actual_title));
                                    ws_stream.close(None).await.ok();
                                    return Ok(());
                                }
                                
                                last_known_title = actual_title.clone();
                                aircraft_info.title = actual_title;
                                aircraft_info.atc_model = actual_atc_model;
                                aircraft_info.atc_id = actual_atc_id;
                                aircraft_info.object_class = object_class;
                                aircraft_info.category = category;
                                aircraft_info.num_engines = num_engines;
                                aircraft_info.engine_type = engine_type;
                                crate::append_log(&app, format!("[X-Plane] Identified aircraft: {} [Model: {}, ID: {}, Engines: {} {}]", last_known_title, aircraft_info.atc_model, aircraft_info.atc_id, aircraft_info.num_engines, aircraft_info.engine_type));
                            }

                            // Resumption check
                            let mut resumed_path = None;
                            if !aircraft_info.title.is_empty() {
                                resumed_path = crate::flight_log_manager::try_find_resume_flight(&app, &m, &aircraft_info.title);
                            }

                            let app_data_dir = app.path().app_data_dir().unwrap();
                            let internal_log_dir = app_data_dir.join("flightlogs");
                            let _ = create_dir_all(&internal_log_dir);
                            
                            let (path, filename) = if let Some(ref p) = resumed_path {
                                let f = p.file_name().unwrap().to_string_lossy().to_string();
                                crate::append_log(&app, format!("[X-Plane] Resuming existing flight log: {}", f));
                                (p.clone(), f)
                            } else {
                                let f = format!("butterlog_xp_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
                                let p = internal_log_dir.join(&f);
                                (p, f)
                            };

                            current_log_path = Some(path.clone());
                            { let mut fid = current_flight_id_mutex.lock().unwrap(); *fid = filename.replace(".db", ""); }

                            if let Ok(conn) = Connection::open(&path) {
                                if let Err(e) = init_sqlite_db(&conn) {
                                    crate::append_log(&app, format!("[X-Plane] Error initializing DB: {}", e));
                                }

                                // Restore analyzer state if resuming
                                if resumed_path.is_some() {
                                    if let Err(e) = analyzer.restore(&conn) {
                                        crate::append_log(&app, format!("[X-Plane] Error restoring analyzer: {}", e));
                                    } else {
                                        start_time = analyzer.first_timestamp.clone();
                                        takeoff_time = analyzer.takeoff_timestamp.clone();
                                    }
                                }

                                // Set initial departure if on ground
                                if m.is_on_ground > 0.5 {
                                    if let Some(db) = app.try_state::<AirportsDatabase>() {
                                        if let Some(nearest) = db.find_nearest(m.latitude, m.longitude, 1).first() {
                                            crate::append_log(&app, format!("[X-Plane] Identified departure: {} ({})", nearest.ident, nearest.name));
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_icao', ?1)", params![nearest.ident]);
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('departure_name', ?1)", params![nearest.name]);
                                        }
                                    }
                                }

                                db_conn = Some(conn);
                                let _ = app.emit("flight-logs-updated", ());
                            }

                            if !aircraft_info.title.is_empty() {
                                 let title_str = aircraft_info.title.clone();
                                 let atc_model_str = aircraft_info.atc_model.clone();
                                 let atc_id_str = aircraft_info.atc_id.clone();
                                 if let Some(ref conn) = db_conn {
                                     let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('aircraft_title', ?1)", params![title_str.clone()]);
                                     let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_model', ?1)", params![atc_model_str.clone()]);
                                     let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('atc_id', ?1)", params![atc_id_str.clone()]);
                                 }
                                 let mut info = aircraft_info_mutex.lock().unwrap();
                                 info.title = title_str;
                                 info.atc_model = atc_model_str;
                                 info.atc_id = atc_id_str;
                                 info.object_class = aircraft_info.object_class.clone();
                                 info.category = aircraft_info.category.clone();
                                 info.num_engines = aircraft_info.num_engines;
                                 info.engine_type = aircraft_info.engine_type.clone();
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
                            let now_instant = std::time::Instant::now();

                            let mut force_update = false;
                            if (last_agl < 100.0 && m.altitude_agl >= 100.0) || (last_agl > 100.0 && m.altitude_agl <= 100.0) {
                                force_update = true;
                            }
                            last_agl = m.altitude_agl;

                            if m.is_on_ground > 0.5 && analyzer.current_phase == crate::models::FlightPhase::Landing {
                                if touchdown_time.is_none() {
                                    touchdown_time = Some(now_instant);
                                } else if touchdown_time.unwrap().elapsed().as_secs() >= 5 && !touchdown_update_done {
                                    force_update = true;
                                    touchdown_update_done = true;
                                }
                            } else {
                                touchdown_time = None;
                                touchdown_update_done = false;
                            }

                            if force_update || now.signed_duration_since(last_log_time) >= chrono::Duration::seconds(1) {
                                last_log_time = now;
                                let now_str = now.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

                                let mut force_sync = false;
                                if force_update || m.ground_speed.abs() > 0.1 || m.vertical_speed.abs() > 10.0 {
                                    if let Some(new_phase) = analyzer.update(&m, &now_str) {
                                        let _ = app.emit("flight-phase-change", new_phase);
                                        if new_phase == crate::models::FlightPhase::Takeoff {
                                            takeoff_snapshot = Some(m);
                                            takeoff_time = Some(now_str.clone());
                                            force_sync = true;
                                            auto_finalized = false;

                                            // Immediate takeoff event in summary
                                            if let Some(ref conn) = db_conn {
                                                let takeoff_event = crate::models::FlightEvent {
                                                    timestamp: now_str.clone(),
                                                    event_type: "takeoff".to_string(),
                                                    latitude: m.latitude,
                                                    longitude: m.longitude,
                                                    touchdown_fpm: None,
                                                    landing_g: None,
                                                    offset_percent: None,
                                                    threshold_dist_ft: None,
                                                    vs_variance: None,
                                                    ias_variance: None,
                                                    heading: None,
                                                };
                                                if let Ok(event_json) = serde_json::to_string(&vec![takeoff_event]) {
                                                    let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('takeoff_event', ?1)", params![event_json]);
                                                }
                                            }
                                            let _ = app.emit("flight-logs-updated", ());
                                        } else if new_phase == crate::models::FlightPhase::Landing {
                                            landing_snapshot = Some(m);
                                            landing_time = Some(now_str.clone());
                                            force_sync = true;
                                            let _ = app.emit("flight-logs-updated", ());
                                        }
                                    }
                                    if let Some(ref conn) = db_conn { 
                                        if let Err(e) = insert_sqlite_row(conn, &now_str, &m) {
                                            crate::append_log(&app, format!("[X-Plane] Error writing to DB: {}", e));
                                        }

                                        // Update summary with live metrics
                                        let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
                                        let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('max_altitude', ?1)", params![analyzer.max_alt.to_string()]);
                                        let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('max_ground_speed', ?1)", params![analyzer.max_gs.to_string()]);
                                        let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('fuel_consumed', ?1)", params![fuel_consumed.to_string()]);

                                        if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('flight_events', ?1)", params![events_json]);
                                        }

                                        if let Some(db) = app.try_state::<AirportsDatabase>() {
                                            let arrival_icao = analyzer.find_end_icao(&db);
                                            let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_icao', ?1)", params![arrival_icao]);
                                            if let Some(airport) = db.get_by_ident(&arrival_icao) {
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES ('arrival_name', ?1)", params![airport.name]);
                                            }
                                        }
                                    }
                                }

                                if takeoff_time.is_some() {
                                    if let Some(db) = app.try_state::<AirportsDatabase>() {
                                         let closest_airport = {
                                             let lat = m.latitude;
                                             let lon = m.longitude;
                                             let nearest = db.find_nearest(lat, lon, 1);
                                             nearest.first().map(|airport| {
                                                 let dist = crate::sim_monitor::calculate_distance(lat, lon, airport.latitude_deg.unwrap_or(0.0), airport.longitude_deg.unwrap_or(0.0));
                                                 ClosestAirportInfo {
                                                     icao: airport.ident.clone(),
                                                     name: airport.name.clone(),
                                                     distance: dist,
                                                 }
                                             })
                                         };

                                         let summary = WebhookFlightSummary {
                                             log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                             airframe_name: aircraft_info.title.clone(),
                                             atc_model: aircraft_info.atc_model.clone(),
                                             atc_id: aircraft_info.atc_id.clone(),
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
                                             closest_airport,
                                             takeoff_time: takeoff_time.clone(),
                                             landing_time: landing_time.clone(),
                                             start_time: start_time.clone(),
                                             end_time: Some(now_str.clone()),
                                             takeoff_snapshot,
                                             landing_snapshot,
                                             current_snapshot: Some(m),
                                             max_entries: max_metrics,
                                             vs_variance: None,
                                             ias_variance: None,
                                             landing_score: None,
                                             landing_offset_percent: None,
                                             landing_threshold_dist_ft: None,
                                         };
                                        let app_c = app.clone();
                                        let sum_c = summary.clone();
                                        tauri::async_runtime::spawn(async move {
                                            app_c.state::<WebhookManager>().sync_flight(&app_c, &sum_c, force_sync).await;
                                        });
                                    }
                                }

                                // Auto-close logic: on ground > 30s or stationary > 10s
                                let now_instant = std::time::Instant::now();
                                if m.is_on_ground > 0.5 {
                                    if on_ground_since.is_none() { on_ground_since = Some(now_instant); }
                                } else {
                                    on_ground_since = None;
                                }

                                if m.ground_speed.abs() < 10.0 {
                                    if stationary_since.is_none() { stationary_since = Some(now_instant); }
                                } else {
                                    stationary_since = None;
                                }

                                let should_close = if takeoff_time.is_some() && landing_time.is_some() {
                                    (if let Some(t) = on_ground_since {
                                        t.elapsed().as_secs() > 30
                                    } else { false }) || (if let Some(t) = stationary_since {
                                        t.elapsed().as_secs() > 10
                                    } else { false })
                                } else { false };

                                if should_close && !auto_finalized {
                                    crate::append_log(&app, "[X-Plane] Aircraft stationary. Updating flight summary and stats.".to_string());
                                    auto_finalized = true;

                                    if let Some(db) = app.try_state::<AirportsDatabase>() {
                                        if let Some(r_db) = app.try_state::<RunwaysDatabase>() {
                                            analyzer.finalize_landing_performance(&db, &r_db, db_conn.as_ref());
                                        }

                                        let start_icao = analyzer.find_start_icao(&db);
                                        let end_icao = analyzer.find_end_icao(&db);
                                        
                                        if takeoff_time.is_some() {
                                            let landing_event = analyzer.events.iter().find(|e| e.event_type == "landing");
                                             let start_name = db.get_by_ident(&start_icao).map(|a| a.name.clone()).unwrap_or_default();
                                             let end_name = db.get_by_ident(&end_icao).map(|a| a.name.clone()).unwrap_or_default();
                                             let closest_airport = {
                                                 let lat = m.latitude;
                                                 let lon = m.longitude;
                                                 let nearest = db.find_nearest(lat, lon, 1);
                                                 nearest.first().map(|airport| {
                                                     let dist = crate::sim_monitor::calculate_distance(lat, lon, airport.latitude_deg.unwrap_or(0.0), airport.longitude_deg.unwrap_or(0.0));
                                                     ClosestAirportInfo {
                                                         icao: airport.ident.clone(),
                                                         name: airport.name.clone(),
                                                         distance: dist,
                                                     }
                                                 })
                                             };

                                             let summary = WebhookFlightSummary {
                                                 log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                                                 airframe_name: aircraft_info.title.clone(),
                                                 atc_model: aircraft_info.atc_model.clone(),
                                                 atc_id: aircraft_info.atc_id.clone(),
                                                 simulator: "X-Plane".to_string(),
                                                 simulator_version: "12".to_string(),
                                                 departure: AirportInfo { icao: start_icao.clone(), name: start_name },
                                                 arrival: AirportInfo { icao: end_icao.clone(), name: end_name },
                                                 closest_airport,
                                                 takeoff_time: takeoff_time.clone(),
                                                 landing_time: landing_time.clone(),
                                                 start_time: start_time.clone(),
                                                 end_time: Some(now_str.clone()),
                                                 takeoff_snapshot: takeoff_snapshot.clone(),
                                                 landing_snapshot: landing_snapshot.clone(),
                                                 current_snapshot: Some(m),
                                                 max_entries: max_metrics.clone(),
                                                 vs_variance: landing_event.and_then(|e| e.vs_variance),
                                                 ias_variance: landing_event.and_then(|e| e.ias_variance),
                                                 landing_score: landing_event.and_then(|e| e.calculate_landing_score()),
                                                 landing_offset_percent: landing_event.and_then(|e| e.offset_percent),
                                                 landing_threshold_dist_ft: landing_event.and_then(|e| e.threshold_dist_ft),
                                             };
                                            let app_c = app.clone();
                                            let sum_c = summary.clone();
                                            tauri::async_runtime::spawn(async move {
                                                app_c.state::<WebhookManager>().sync_flight(&app_c, &sum_c, true).await;
                                            });
                                        }

                                        if let Some(ref conn) = db_conn {
                                            let mut summary_data = vec![
                                                ("departure_icao", start_icao.clone()),
                                                ("arrival_icao", end_icao.clone()),
                                                ("aircraft_title", aircraft_info.title.clone()),
                                                ("atc_model", aircraft_info.atc_model.clone()),
                                                ("atc_id", aircraft_info.atc_id.clone()),
                                                ("max_altitude", analyzer.max_alt.to_string()),
                                                ("max_ground_speed", analyzer.max_gs.to_string()),
                                                ("fuel_consumed", (analyzer.initial_fuel - analyzer.final_fuel).to_string()),
                                            ];

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
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params![k, v]);
                                            }

                                            if let Ok(events_json) = serde_json::to_string(&analyzer.events) {
                                                let _ = conn.execute("INSERT OR REPLACE INTO summary (key, value) VALUES (?1, ?2)", params!["flight_events", events_json]);
                                            }

                                            let fuel_consumed = analyzer.initial_fuel - analyzer.final_fuel;
                                            let duration_mins = analyzer.get_duration_minutes();
                                            let _ = crate::flight_log_manager::update_aircraft_stats(&app, &aircraft_info.title, duration_mins as f64, fuel_consumed, &end_icao, true);
                                        }

                                        let _ = app.emit("flight-logs-updated", ());
                                    }

                                    on_ground_since = None;
                                    stationary_since = None;
                                }
                            }
                        }
                    } else if let Err(e) = serde_json::from_str::<Value>(&text) {
                        if last_debug_log.elapsed().as_secs() >= 10 {
                            crate::append_log(&app, format!("[X-Plane] JSON Parse Error: {}. Text sample: {}", e, text.chars().take(50).collect::<String>()));
                            last_debug_log = std::time::Instant::now();
                        }
                    }
                }
                Ok(other) => {
                    if last_debug_log.elapsed().as_secs() >= 10 {
                        crate::append_log(&app, format!("[X-Plane] Received non-text message: {:?}", other));
                        last_debug_log = std::time::Instant::now();
                    }
                }
                Err(e) => {
                    crate::append_log(&app, format!("[X-Plane] WebSocket Error: {}", e));
                    break;
                }
            }
        }

        if flight_ongoing { // Finalization logic
            {
                let mut fid = current_flight_id_mutex.lock().unwrap();
                fid.clear();
            }
            if let Some(db) = app.try_state::<AirportsDatabase>() {
                // Advanced Landing Analysis
                if let Some(r_db) = app.try_state::<RunwaysDatabase>() {
                    analyzer.finalize_landing_performance(&db, &r_db, db_conn.as_ref());
                }

                let now_str = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                let landing_event = analyzer.events.iter().find(|e| e.event_type == "landing");
                let closest_airport = {
                    let lat = m.latitude;
                    let lon = m.longitude;
                    let nearest = db.find_nearest(lat, lon, 1);
                    nearest.first().map(|airport| {
                        let dist = crate::sim_monitor::calculate_distance(lat, lon, airport.latitude_deg.unwrap_or(0.0), airport.longitude_deg.unwrap_or(0.0));
                        ClosestAirportInfo {
                            icao: airport.ident.clone(),
                            name: airport.name.clone(),
                            distance: dist,
                        }
                    })
                };

                let summary = WebhookFlightSummary {
                    log_path: current_log_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                    airframe_name: aircraft_info.title.clone(),
                    atc_model: aircraft_info.atc_model.clone(),
                    atc_id: aircraft_info.atc_id.clone(),
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
                    closest_airport,
                    takeoff_time: takeoff_time.clone(),
                    landing_time: landing_time.clone(),
                    start_time: start_time.clone(),
                    end_time: Some(now_str.clone()),

                    takeoff_snapshot: takeoff_snapshot.clone(),
                    landing_snapshot: landing_snapshot.clone(),
                    current_snapshot: Some(m),
                    max_entries: max_metrics.clone(),
                    vs_variance: landing_event.and_then(|e| e.vs_variance),
                    ias_variance: landing_event.and_then(|e| e.ias_variance),
                    landing_score: landing_event.and_then(|e| e.calculate_landing_score()),
                    landing_offset_percent: landing_event.and_then(|e| e.offset_percent),
                    landing_threshold_dist_ft: landing_event.and_then(|e| e.threshold_dist_ft),
                };
                if takeoff_time.is_some() {
                    let app_c = app.clone();
                    let sum_c = summary.clone();
                    tauri::async_runtime::spawn(async move {
                        app_c.state::<WebhookManager>().sync_flight(&app_c, &sum_c, true).await;
                    });
                }
                crate::append_log(&app, "[X-Plane] Finalized flight sync.".to_string());

                // Final update to analyzer
                let final_ts = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                analyzer.update(&m, &final_ts);

                drop(db_conn.take());

                let duration_mins = analyzer.get_duration_minutes();
                let has_movement = analyzer.max_gs > 5.0 || analyzer.max_alt > 50.0;
                let is_very_short = duration_mins < 2;
                
                if is_very_short || !has_movement {
                    if let Some(path) = current_log_path.take() {
                        let _ = std::fs::remove_file(&path);
                        crate::append_log(&app, format!("[X-Plane] Deleted short/empty flight log: {}", path.display()));
                    }
                }

                let _ = app.emit("flight-logs-updated", ());
            }
        }

        { let mut c = connected.lock().unwrap(); *c = false; }
        webhook_manager.reset();
        Ok(())
    }
}

impl SimMonitor for XPlaneMonitor {
    fn id(&self) -> &'static str { "xplane" }
    fn start(&self, app: AppHandle, _log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock().unwrap();
        if *running { return Ok(()); }
        *running = true;
        let metrics = self.metrics.clone();
        let aircraft_info = self.aircraft_info.clone();
        let current_flight_id = self.current_flight_id.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();
        let monitoring_clone = self.monitoring.clone();
        
        tauri::async_runtime::spawn(async move {
            loop {
                if !*running_clone.lock().unwrap() { break; }
                let _ = Self::run_monitor_async(
                    app.clone(),
                    metrics.clone(),
                    aircraft_info.clone(),
                    current_flight_id.clone(),
                    running_clone.clone(),
                    connected_clone.clone(),
                    monitoring_clone.clone(),
                ).await;
                { let mut m = metrics.lock().unwrap(); *m = FlightMetrics::default(); }
                { let mut info = aircraft_info.lock().unwrap(); *info = crate::models::AircraftInfo::default(); }
                { let mut fid = current_flight_id.lock().unwrap(); *fid = "".to_string(); }
                { let mut c = connected_clone.lock().unwrap(); *c = false; }
                { let mut m = monitoring_clone.lock().unwrap(); *m = false; }
                // Retry after 5 seconds if disconnected
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });
        Ok(())
    }
    fn stop(&self) { let mut running = self.running.lock().unwrap(); *running = false; }
    fn get_metrics(&self) -> FlightMetrics { *self.metrics.lock().unwrap() }
    fn get_aircraft_info(&self) -> crate::models::AircraftInfo { self.aircraft_info.lock().unwrap().clone() }
    fn get_current_flight_id(&self) -> String { self.current_flight_id.lock().unwrap().clone() }
    fn is_connected(&self) -> bool { *self.connected.lock().unwrap() }
    fn is_monitoring(&self) -> bool { *self.monitoring.lock().unwrap() }
    fn update_remote_aircraft(
        &self,
        _id: &str,
        _title: &str,
        _atc_model: &str,
        _object_class: &str,
        _category: &str,
        _num_engines: i32,
        _engine_type: &str,
        _metrics: &FlightMetrics,
    ) {}
}
