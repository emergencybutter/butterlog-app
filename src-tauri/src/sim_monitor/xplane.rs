use crate::models::{AircraftInfo, FlightMetrics};
use crate::sim_monitor::SimMonitor;
use futures_util::{StreamExt, SinkExt};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};
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
    let item = json["data"].as_array()?.first()?;
    let id = item.get("id")?;
    let id_str = if id.is_string() {
        id.as_str()?.to_string()
    } else {
        id.as_i64()?.to_string()
    };
    let val_url = format!("{}/{}/value", rest_url, id_str);
    let val_resp = client.get(&val_url).send().await.ok()?;
    let val_json = val_resp.json::<Value>().await.ok()?;
    let val_ref = val_json.get("data")?;
    extract_xplane_string(val_ref)
}

async fn fetch_xplane_dataref_double(client: &reqwest::Client, rest_url: &str, name: &str) -> Option<f64> {
    let resp = client.get(rest_url).query(&[("filter[name]", name)]).send().await.ok()?;
    let json = resp.json::<Value>().await.ok()?;
    let item = json["data"].as_array()?.first()?;
    let id = item.get("id")?;
    let id_str = if id.is_string() {
        id.as_str()?.to_string()
    } else {
        id.as_i64()?.to_string()
    };
    let val_url = format!("{}/{}/value", rest_url, id_str);
    let val_resp = client.get(&val_url).send().await.ok()?;
    let val_json = val_resp.json::<Value>().await.ok()?;
    let val_ref = val_json.get("data")?;
    if let Some(arr) = val_ref.as_array() {
        arr.first()?.as_f64()
    } else {
        val_ref.as_f64()
    }
}

/// `sim/aircraft/prop/acf_en_type` in the words the MSFS side uses ("piston", "turboprop",
/// "jet"). X-Plane's values: 0 carburetted piston, 1 fuel-injected piston, 2 free turbine,
/// 3 electric, 4 low-bypass jet, 5 high-bypass jet, 6 rocket, 7 tip rockets, 8 fixed turbine.
fn engine_type_name(acf_en_type: i32) -> &'static str {
    match acf_en_type {
        0 | 1 => "piston",
        2 | 8 => "turboprop",
        4 | 5 => "jet",
        3 => "electric",
        _ => "unknown",
    }
}

/// Fetch the aircraft identity (name, livery, type, engines) from X-Plane via REST.
/// These string/config datarefs aren't subscribable, so each call is a one-shot read.
/// `title` is empty when no aircraft is loaded yet.
async fn fetch_xplane_aircraft_identity(client: &reqwest::Client, rest_url: &str) -> AircraftInfo {
    let title = fetch_xplane_dataref_string(client, rest_url, "sim/aircraft/view/acf_ui_name").await.unwrap_or_default();
    let atc_model = fetch_xplane_dataref_string(client, rest_url, "sim/aircraft/view/acf_ICAO").await.unwrap_or_default();
    let atc_id = fetch_xplane_dataref_string(client, rest_url, "sim/aircraft/view/acf_tailnum").await.unwrap_or_default();
    // X-Plane's livery identifier; may be empty if the aircraft has no liveries.
    let livery = fetch_xplane_dataref_string(client, rest_url, "sim/aircraft/view/acf_livery_path").await.unwrap_or_default();

    // X-Plane 12 has no `acf_class`; these flags replace it.
    let flag = |v: Option<f64>| v.is_some_and(|v| v > 0.5);
    let helicopter = flag(fetch_xplane_dataref_double(client, rest_url, "sim/aircraft2/metadata/is_helicopter").await);
    let glider = flag(fetch_xplane_dataref_double(client, rest_url, "sim/aircraft2/metadata/is_glider").await);
    let class = if helicopter { "helicopter" } else if glider { "glider" } else { "airplane" };
    let (object_class, category) = (class.to_string(), class.to_string());

    let num_engines = fetch_xplane_dataref_double(client, rest_url, "sim/aircraft/engine/acf_num_engines").await.unwrap_or(1.0) as i32;

    let en_type_val = fetch_xplane_dataref_double(client, rest_url, "sim/aircraft/prop/acf_en_type").await.unwrap_or(0.0) as i32;
    let engine_type = engine_type_name(en_type_val).to_string();

    AircraftInfo { title, livery, resolved_icao: String::new(), resolved_airline: String::new(), resolved_airline_icao: String::new(), atc_model, atc_id, object_class, category, num_engines, engine_type }
}

/// A pitch or roll between the MSFS signs traffic travels in (nose down, left wing down
/// positive) and X-Plane's (nose up, right wing down positive). The conversion is its own
/// inverse. Logs are not converted: they hold what the sim reports.
fn to_xplane_attitude(degrees: f64) -> f64 {
    -degrees
}

/// Our metrics with the attitude in the signs traffic travels in.
fn traffic_attitude(mut m: FlightMetrics) -> FlightMetrics {
    m.pitch_angle = to_xplane_attitude(m.pitch_angle);
    m.roll_angle = to_xplane_attitude(m.roll_angle);
    m
}

/// Resolve the ICAO type and operating airline from the title + livery, as the service
/// does (`butterlog-identity`), set them on `identity` and log them. The service's answer
/// replaces this one for traffic peers once the session opens (`crate::identity`).
fn resolve_and_log(app: &AppHandle, identity: &mut AircraftInfo) {
    let r = butterlog_identity::Resolver::embedded().resolve(&identity.title, &identity.livery);
    match &r.icao {
        Some(m) => crate::append_log(app, format!("[X-Plane Index] '{}' [{}] -> {} (score {:.2})", identity.title, identity.livery, m.icao, m.score)),
        None => crate::append_log(app, format!("[X-Plane Index] '{}' [{}] -> no ICAO match", identity.title, identity.livery)),
    }
    if let Some(m) = &r.icao {
        identity.resolved_icao = m.icao.clone();
    }
    match &r.airline {
        Some(m) => crate::append_log(app, format!("[X-Plane Airline] '{}' [{}] -> {} {} (score {:.2})", identity.title, identity.livery, m.icao, m.name, m.score)),
        None if r.airline_skipped => crate::append_log(app, format!("[X-Plane Airline] '{}' -> skipped, {} is a piston/light turboprop", identity.title, identity.resolved_icao)),
        None => crate::append_log(app, format!("[X-Plane Airline] '{}' [{}] -> no airline match", identity.title, identity.livery)),
    }
    if r.airline_skipped {
        identity.resolved_airline.clear();
        identity.resolved_airline_icao.clear();
    } else if let Some(m) = r.airline {
        identity.resolved_airline = m.name;
        identity.resolved_airline_icao = m.icao;
    }
}


pub struct XPlaneMonitor {
    metrics: Arc<Mutex<FlightMetrics>>,
    aircraft_info: Arc<Mutex<AircraftInfo>>,
    running: Arc<Mutex<bool>>,
    connected: Arc<Mutex<bool>>,
}

impl XPlaneMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(Mutex::new(FlightMetrics::default())),
            aircraft_info: Arc::new(Mutex::new(AircraftInfo::default())),
            running: Arc::new(Mutex::new(false)),
            connected: Arc::new(Mutex::new(false)),
        }
    }

    async fn run_monitor_async(
        app: AppHandle,
        metrics_mutex: Arc<Mutex<FlightMetrics>>,
        aircraft_info_mutex: Arc<Mutex<AircraftInfo>>,
        running: Arc<Mutex<bool>>,
        connected: Arc<Mutex<bool>>,
    ) -> anyhow::Result<()> {
        let base_host = "localhost:8086";
        let rest_url = format!("http://{}/api/v3/datarefs", base_host);
        let ws_url = format!("ws://{}/api/v3", base_host);
        let mut max_hp = 180.0;

        // Required dataref paths for flight tracking
        let mut paths = vec![
            "sim/flightmodel/position/latitude",
            "sim/flightmodel/position/longitude",
            "sim/flightmodel/position/elevation",
            "sim/flightmodel/position/indicated_airspeed",
            "sim/flightmodel/position/groundspeed",
            "sim/flightmodel/position/vh_ind",
            "sim/flightmodel/failures/onground_any",
            "sim/flightmodel/position/y_agl",
            "sim/flightmodel/position/psi",
            "sim/cockpit2/fuel/fuel_quantity",
            "sim/flightmodel/forces/g_nrml",
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
            "sim/cockpit2/engine/indicators/oil_temperature_deg_C",
            "sim/cockpit2/engine/indicators/oil_pressure_psi",
            "sim/cockpit2/engine/indicators/MPR_in_hg",
            "sim/cockpit2/engine/indicators/engine_speed_rpm",
            "sim/cockpit2/engine/indicators/CHT_deg_cel",
            "sim/cockpit2/engine/indicators/EGT_deg_cel",
            "sim/cockpit2/autopilot/autopilot_on",
            "sim/cockpit2/autopilot/sync_hold_pitch_deg",
            "sim/cockpit2/autopilot/sync_hold_roll_deg",
            "sim/cockpit2/autopilot/vvi_dial_fpm",
            "sim/flightmodel2/position/ellipsoid_height",
            "sim/flightmodel/position/true_airspeed",
            "sim/cockpit2/radios/actuators/HSI_source_select_pilot",
            "sim/cockpit2/radios/actuators/hsi_obs_deg_mag_pilot",
            "sim/cockpit2/radios/actuators/nav1_frequency_hz",
            "sim/cockpit2/radios/actuators/nav2_frequency_hz",
            "sim/cockpit2/radios/actuators/com1_frequency_hz_833",
            "sim/cockpit2/radios/actuators/com2_frequency_hz_833",
            "sim/flightmodel2/position/pressure_altitude",
            "sim/aircraft/parts/acf_gear_deploy",
            "sim/cockpit2/engine/indicators/power_watts",
            "sim/cockpit2/engine/indicators/ITT_deg_C",
            "sim/cockpit2/radios/indicators/nav1_hdef_dots_pilot",
            "sim/cockpit2/radios/indicators/nav1_vdef_dots_pilot",
            "sim/weather/wind_speed_kt",
            "sim/weather/wind_direction_degt",
            "sim/cockpit2/radios/indicators/gps_dme_distance_nm",
            "sim/cockpit2/radios/indicators/gps_bearing_deg_mag",
            "sim/flightmodel/position/magnetic_variation",
            "sim/cockpit2/autopilot/roll_status",
            "sim/cockpit2/autopilot/pitch_status",
            "sim/cockpit2/pressurization/indicators/cabin_altitude_ft",
            "sim/cockpit2/controls/parking_brake_ratio",
            "sim/cockpit2/switches/navigation_lights_on",
            "sim/cockpit2/switches/beacon_on",
            "sim/cockpit2/switches/strobe_lights_on",
            "sim/cockpit2/switches/taxi_light_on",
            "sim/cockpit2/switches/landing_lights_on",
        ];

        if crate::telemetry::recorder::enabled(&app) {
            for dataref in crate::telemetry::xplane::datarefs() {
                if !paths.contains(&dataref) {
                    paths.push(dataref);
                }
            }
        }

        // 1. Discovery Phase: Fetch session-specific IDs via REST discovery
        let client = reqwest::Client::new();
        let mut path_to_id = std::collections::HashMap::new();
        
        let resp = match client.get(&rest_url).send().await {
            Ok(r) => match r.json::<Value>().await {
                Ok(j) => j,
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to parse datarefs JSON: {}", e));
                }
            },
            Err(e) => {
                return Err(anyhow::anyhow!("Failed to send datarefs discovery request: {}", e));
            }
        };

        let paths_set: std::collections::HashSet<&str> = paths.iter().copied().collect();

        if let Some(data) = resp["data"].as_array() {
            for item in data {
                if let (Some(name), Some(id)) = (item["name"].as_str(), item["id"].as_i64()) {
                    if paths_set.contains(name) {
                        path_to_id.insert(name.to_string(), id.to_string());
                    }
                }
            }
        }

        // Validate that all critical datarefs were successfully mapped
        let critical_datarefs = vec![
            "sim/flightmodel/position/latitude",
            "sim/flightmodel/position/longitude",
            "sim/flightmodel/position/elevation",
            "sim/flightmodel/position/indicated_airspeed",
            "sim/flightmodel/position/groundspeed",
            "sim/flightmodel/failures/onground_any",
        ];

        let mut missing = Vec::new();
        for key in &critical_datarefs {
            if !path_to_id.contains_key(*key) {
                missing.push(*key);
            }
        }

        if !missing.is_empty() {
            crate::append_log(&app, format!("[X-Plane] ERROR: Missing critical datarefs: {:?}. Connection aborted.", missing));
            return Err(anyhow::anyhow!("Missing critical datarefs: {:?}", missing));
        }

        crate::append_log(&app, format!("[X-Plane] Discovered {}/{} dataref IDs.", path_to_id.len(), paths.len()));
        for path in &paths {
            if let Some(id) = path_to_id.get(*path) {
                if path.contains("groundspeed") || path.contains("onground") {
                    crate::append_log(&app, format!("[X-Plane] Map: {} -> {}", path, id));
                }
            }
        }

        let xplane_version = fetch_xplane_dataref_double(&client, &rest_url, "sim/version/xplane_internal_version").await;
        // Telemetry comes from the plugin's link when it's there (every sim frame), from
        // this connection's Web API updates otherwise.
        let plugin_link = if crate::telemetry::recorder::enabled(&app) {
            let link_app = app.clone();
            tokio::task::spawn_blocking(move || {
                crate::telemetry::xp_plugin::connect(&link_app, &crate::telemetry::xplane::plugin_entries())
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        let telemetry_sampler = crate::telemetry::xplane::XplaneSampler::register(
            &app,
            &path_to_id,
            xplane_version,
            plugin_link.is_some(),
        )
        .map(|s| std::sync::Arc::new(parking_lot::Mutex::new(s)));
        // Streams until dropped with this connection.
        let _plugin_link = match (plugin_link, &telemetry_sampler) {
            (Some(link), Some(sampler)) => link.run(app.clone(), sampler.clone()),
            _ => None,
        };

        // 2. Connection Phase: Open WebSocket
        let (mut ws_stream, _) = match connect_async(&ws_url).await {
            Ok(s) => s,
            Err(e) => {
                crate::append_log(&app, format!("[X-Plane] WebSocket connection failed: {}", e));
                return Err(anyhow::anyhow!("Connection failed"));
            }
        };

        { let mut c = connected.lock(); *c = true; }
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

        let mut aircraft_info = AircraftInfo::default();
        // Seen moving since this connection began: the aircraft is then identified again.
        let mut moving = false;

        let mut last_debug_log = std::time::Instant::now();
        let mut m = FlightMetrics::default();
        // (lat, lon, instant the fix was received) so a jump can be judged against
        // how much time actually elapsed, not just raw distance between samples.
        let mut last_pos: Option<(f64, f64, std::time::Instant)> = None;
        let mut last_known_title = String::new();
        let mut last_identify_attempt: Option<std::time::Instant> = None;


        while let Some(msg) = ws_stream.next().await {
            if !*running.lock() { break; }

            // Web-issued commands. X-Plane needs no plugin for these: the same
            // Web API this monitor reads through also writes.
            drain_sim_commands(&app).await;

            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(data) = serde_json::from_str::<Value>(&text) {
                        if data["type"] != "dataref_update_values" { continue; }
                        if let (Some(ts), Some(values)) = (&telemetry_sampler, data["data"].as_object()) {
                            ts.lock().on_update(values);
                        }
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
                        
                        // Teleport detection. A real teleport (loading a new situation or
                        // repositioning the aircraft) is a position discontinuity: the fix
                        // moves further than the aircraft could physically have travelled in
                        // the elapsed wall-clock time. A stalled data stream (network hiccup,
                        // frame stutter, brief pause) is NOT a teleport — the aircraft keeps
                        // flying, so a 30s gap at 420kt legitimately covers ~3.5nm. Judging by
                        // raw distance alone flagged those as jumps and reset the flight.
                        //
                        // Bound the plausible travel by the last known groundspeed and the
                        // elapsed time, times a generous factor that absorbs GS-reading lag,
                        // turns, and time acceleration (X-Plane's common 2x/4x/8x steps), plus
                        // a small absolute margin for GPS jitter near-stationary. m.ground_speed
                        // here is the reading from just before the gap (parsed later this loop).
                        const JUMP_SPEED_TOLERANCE: f64 = 8.0;   // covers up to ~8x time accel
                        const JUMP_BASE_MARGIN_NM: f64 = 3.0;    // floor: never flag sub-3nm moves
                        if updated && m.has_position() {
                            let now_pos = std::time::Instant::now();
                            if let Some((l_lat, l_lon, l_time)) = last_pos {
                                let d_lat = (m.latitude - l_lat).abs() * 60.0; // 1 deg lat = 60nm
                                let d_lon = (m.longitude - l_lon).abs() * 60.0 * l_lat.to_radians().cos();
                                let actual_nm = (d_lat * d_lat + d_lon * d_lon).sqrt();

                                let elapsed_s = now_pos.duration_since(l_time).as_secs_f64();
                                let max_plausible_nm =
                                    (m.ground_speed.max(0.0) / 3600.0) * elapsed_s * JUMP_SPEED_TOLERANCE
                                    + JUMP_BASE_MARGIN_NM;

                                if moving && actual_nm > max_plausible_nm {
                                    crate::append_log(&app, format!(
                                        "[X-Plane] Position jump detected: {:.2}nm in {:.1}s (max plausible {:.2}nm at GS {:.0}kt). Reconnecting.",
                                        actual_nm, elapsed_s, max_plausible_nm, m.ground_speed));
                                    if let Some(ts) = &telemetry_sampler {
                                        ts.lock().on_teleport();
                                    }
                                    ws_stream.close(None).await.ok();
                                    return Ok(()); // Loop in start() will restart discovery and new flight
                                }
                            }
                            last_pos = Some((m.latitude, m.longitude, now_pos));
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
                        // True heading, as MSFS reports it and runway headings are; `mag_psi` is magnetic.
                        if let Some(v) = get_path_double("sim/flightmodel/position/psi") { m.heading = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/fuel/fuel_quantity", 0) { m.fuel_quantity_left = v * 2.20462 * 0.1498; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/fuel/fuel_quantity", 1) { m.fuel_quantity_right = v * 2.20462 * 0.1498; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/forces/g_nrml") { m.normal_acceleration = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/gauges/indicators/altitude_ft_pilot") { m.indicated_altitude = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit/misc/barometer_setting") { m.altimeter_setting = v; updated = true; }
                        if let Some(v) = get_path_double("sim/weather/temperature_ambient_c") { m.outside_air_temp = v; updated = true; }
                        // As the sim reports them, nose up and right wing down positive: the service
                        // applies each sim's signs. See `traffic_metrics` for what peers are sent.
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
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/oil_temperature_deg_C", 0) { m.engine_1_oil_temp = v * 1.8 + 32.0; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/oil_pressure_psi", 0) { m.engine_1_oil_pressure = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/MPR_in_hg", 0) { m.engine_1_manifold_pressure = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/engine_speed_rpm", 0) { m.engine_1_rpm = v; updated = true; }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/power_watts", 0) { m.engine_1_percent_power = (v / 745.7 / max_hp) * 100.0; updated = true; }

                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/CHT_deg_cel", 0) {
                            let temp_f = v * 1.8 + 32.0;
                            m.engine_1_cht_1 = temp_f;
                            m.engine_1_cht_2 = temp_f;
                            m.engine_1_cht_3 = temp_f;
                            m.engine_1_cht_4 = temp_f;
                            m.engine_1_cht_5 = temp_f;
                            m.engine_1_cht_6 = temp_f;
                            updated = true;
                        }

                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/EGT_deg_cel", 0) {
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
                            // autopilot_on is a boolean dataref (0/1), so any truthy value means engaged.
                            m.autopilot_active = if v > 0.5 { 1.0 } else { 0.0 };
                            updated = true;
                        }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/sync_hold_pitch_deg") { m.pitch_command = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/sync_hold_roll_deg") { m.roll_command = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/vvi_dial_fpm") { m.vertical_speed_target = v; updated = true; }

                        // Frequencies
                        // In MHz, as MSFS reports them: X-Plane's COM datarefs are in kHz, NAV in tens of kHz.
                        if let Some(v) = get_path_double("sim/cockpit2/radios/actuators/com1_frequency_hz_833") { m.com_1_frequency = v / 1_000.0; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/actuators/com2_frequency_hz_833") { m.com_2_frequency = v / 1_000.0; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/actuators/nav1_frequency_hz") { m.nav_1_frequency = v / 100.0; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/actuators/nav2_frequency_hz") { m.nav_2_frequency = v / 100.0; updated = true; }

                        // OBS / HSI Source
                        if let Some(v) = get_path_double("sim/cockpit2/radios/actuators/HSI_source_select_pilot") { m.hsi_source = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/actuators/hsi_obs_deg_mag_pilot") { m.selected_course = v; updated = true; }

                        // GPS / TAS / G-force / Alt
                        if let Some(v) = get_path_double("sim/flightmodel2/position/ellipsoid_height") { m.gps_altitude_wgs84 = v * 3.28084; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/true_airspeed") { m.true_airspeed = v * 1.94384; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/forces/g_nrml") { m.gforce = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel2/position/pressure_altitude") {
                            m.pressure_altitude = v;
                            m.density_altitude = v + 120.0 * (m.outside_air_temp - (15.0 - 1.98 * (v / 1000.0)));
                            updated = true;
                        }
                        if let Some(v) = get_path_double_idx("sim/aircraft/parts/acf_gear_deploy", 0) { m.xp_gear_ratio = v; updated = true; }

                        // Exterior light switch states (0/1).
                        if let Some(v) = get_path_double("sim/cockpit2/switches/navigation_lights_on") { m.nav_lights = if v > 0.5 { 1.0 } else { 0.0 }; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/switches/beacon_on") { m.beacon_lights = if v > 0.5 { 1.0 } else { 0.0 }; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/switches/strobe_lights_on") { m.strobe_lights = if v > 0.5 { 1.0 } else { 0.0 }; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/switches/taxi_light_on") { m.taxi_lights = if v > 0.5 { 1.0 } else { 0.0 }; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/switches/landing_lights_on") { m.landing_lights = if v > 0.5 { 1.0 } else { 0.0 }; updated = true; }

                        // Added X-Plane parameter mapping
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/ITT_deg_C", 0) {
                            m.engine_1_tit_1 = v * 1.8 + 32.0;
                            updated = true;
                        }
                        if let Some(v) = get_path_double_idx("sim/cockpit2/engine/indicators/ITT_deg_C", 1) {
                            m.engine_1_tit_2 = v * 1.8 + 32.0;
                            updated = true;
                        }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/indicators/nav1_hdef_dots_pilot") { m.horizontal_cdi = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/indicators/nav1_vdef_dots_pilot") { m.vertical_cdi = v; updated = true; }
                        if let Some(v) = get_path_double("sim/weather/wind_speed_kt") { m.wind_speed = v; updated = true; }
                        if let Some(v) = get_path_double("sim/weather/wind_direction_degt") { m.wind_direction = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/indicators/gps_dme_distance_nm") { m.waypoint_distance = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/radios/indicators/gps_bearing_deg_mag") { m.waypoint_bearing = v; updated = true; }
                        if let Some(v) = get_path_double("sim/flightmodel/position/magnetic_variation") { m.magnetic_variation = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/roll_status") {
                            m.roll_mode = if (v - 2.0).abs() < 0.1 { 1.0 } else { 0.0 };
                            updated = true;
                        }
                        if let Some(v) = get_path_double("sim/cockpit2/autopilot/pitch_status") {
                            m.pitch_mode = if (v - 2.0).abs() < 0.1 { 1.0 } else { 0.0 };
                            updated = true;
                        }
                        if let Some(v) = get_path_double("sim/cockpit2/pressurization/indicators/cabin_altitude_ft") { m.pressurization_cabin_altitude = v; updated = true; }
                        if let Some(v) = get_path_double("sim/cockpit2/controls/parking_brake_ratio") { m.parking_brake = v; updated = true; }

                        if !updated { continue; }

                        if last_debug_log.elapsed().as_secs() >= 10 {
                            crate::append_log(&app, format!("[X-Plane] Debug: GS={:.1}, OnGround={}, Lat={:.4}, Lon={:.4}", 
                                m.ground_speed, m.is_on_ground > 0.5, m.latitude, m.longitude));
                            last_debug_log = std::time::Instant::now();
                        }

                        { let mut metrics_lock = metrics_mutex.lock(); *metrics_lock = m; }

                        // No position yet (e.g. still loading): nothing to log or analyse.
                        if !m.has_position() { continue; }

                        // While parked (no flight yet), try once a minute to identify the
                        // aircraft (name + livery) so it's known up front — the aircraft may
                        // still be loading right after connect. Stop once we have a title; the
                        // movement block below refreshes it when the flight actually starts.
                        if !moving
                            && aircraft_info.title.is_empty()
                            && last_identify_attempt.map_or(true, |t| t.elapsed().as_secs() >= 60)
                        {
                            last_identify_attempt = Some(std::time::Instant::now());
                            let mut identity = fetch_xplane_aircraft_identity(&client, &rest_url).await;
                            if !identity.title.is_empty() {
                                resolve_and_log(&app, &mut identity);
                                aircraft_info = identity.clone();
                                if let Some(ts) = &telemetry_sampler {
                                    ts.lock().aircraft_loaded(&aircraft_info);
                                }
                                { let mut info = aircraft_info_mutex.lock(); *info = identity; }
                                crate::append_log(&app, format!("[X-Plane] Identified aircraft (pre-flight): {} [Model: {}, ID: {}, Livery: {}]", aircraft_info.title, aircraft_info.atc_model, aircraft_info.atc_id, aircraft_info.livery));
                            }
                        }

                        // Moving under its own power: the aircraft is loaded, so identify it
                        // again (strings and config datarefs aren't subscribable, so these are
                        // one-shot REST reads). Another aircraft than before restarts the
                        // connection, and so the telemetry session.
                        if !moving && (m.is_on_ground < 0.5 || m.ground_speed > 10.0) {
                            moving = true;
                            let mut identity = fetch_xplane_aircraft_identity(&client, &rest_url).await;

                            if let Some(pmax) = fetch_xplane_dataref_double(&client, &rest_url, "sim/aircraft/engine/acf_pmax").await {
                                if pmax > 0.0 {
                                    max_hp = pmax / 745.7;
                                }
                            }

                            if !identity.title.is_empty() {
                                if !last_known_title.is_empty() && last_known_title != identity.title {
                                    crate::append_log(&app, format!("[X-Plane] Aircraft changed ({} -> {}). Reconnecting.", last_known_title, identity.title));
                                    ws_stream.close(None).await.ok();
                                    return Ok(());
                                }

                                last_known_title = identity.title.clone();
                                resolve_and_log(&app, &mut identity);
                                aircraft_info = identity.clone();
                                if let Some(ts) = &telemetry_sampler {
                                    ts.lock().aircraft_loaded(&aircraft_info);
                                }
                                crate::append_log(&app, format!("[X-Plane] Identified aircraft: {} [Model: {}, ID: {}, Livery: {}, Engines: {} {}]", last_known_title, aircraft_info.atc_model, aircraft_info.atc_id, aircraft_info.livery, aircraft_info.num_engines, aircraft_info.engine_type));
                                *aircraft_info_mutex.lock() = identity;
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


        { let mut c = connected.lock(); *c = false; }
        Ok(())
    }
}

impl SimMonitor for XPlaneMonitor {
    fn id(&self) -> &'static str { "xplane" }
    fn start(&self, app: AppHandle, _log_path: Option<PathBuf>) -> anyhow::Result<()> {
        let mut running = self.running.lock();
        if *running { return Ok(()); }
        *running = true;
        let metrics = self.metrics.clone();
        let aircraft_info = self.aircraft_info.clone();
        let running_clone = self.running.clone();
        let connected_clone = self.connected.clone();
        
        tauri::async_runtime::spawn(async move {
            loop {
                if !*running_clone.lock() { break; }
                let _ = Self::run_monitor_async(
                    app.clone(),
                    metrics.clone(),
                    aircraft_info.clone(),
                    running_clone.clone(),
                    connected_clone.clone(),
                ).await;
                { let mut m = metrics.lock(); *m = FlightMetrics::default(); }
                { let mut info = aircraft_info.lock(); *info = crate::models::AircraftInfo::default(); }
                { let mut c = connected_clone.lock(); *c = false; }
                // Retry after 5 seconds if disconnected
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });
        Ok(())
    }
    fn stop(&self) { let mut running = self.running.lock(); *running = false; }
    fn get_metrics(&self) -> FlightMetrics { *self.metrics.lock() }
    fn traffic_metrics(&self) -> FlightMetrics { traffic_attitude(self.get_metrics()) }
    fn get_aircraft_info(&self) -> crate::models::AircraftInfo { self.aircraft_info.lock().clone() }
    fn is_connected(&self) -> bool { *self.connected.lock() }
    fn update_remote_aircraft(
        &self,
        id: &str,
        identity: &crate::sim_monitor::RemoteAircraftIdentity,
        metrics: &FlightMetrics,
        // X-Plane's plugin selects the CSL model itself, so the receiver-side resolution
        // is ignored here.
        _chosen_model: Option<&str>,
    ) {
        static UDP_SOCKET: std::sync::OnceLock<std::net::UdpSocket> = std::sync::OnceLock::new();
        let socket = UDP_SOCKET.get_or_init(|| {
            std::net::UdpSocket::bind("0.0.0.0:0").expect("Failed to bind UDP socket")
        });

        // Operating airline as an ICAO code: prefer the sender's deduced airline (which the
        // plugin's livery/airline maps key on directly), falling back to the callsign prefix.
        let mut airline = if identity.resolved_airline_icao.is_empty() {
            None
        } else {
            Some(identity.resolved_airline_icao.to_uppercase())
        };
        if airline.is_none() && id.len() >= 4 {
            let first_3 = &id[..3];
            if first_3.chars().all(|c| c.is_ascii_alphabetic()) {
                airline = Some(first_3.to_uppercase());
            }
        }

        // Prefer the sender's deduced ICAO type over its raw ATC MODEL claim (often empty
        // or garbage on add-on/custom aircraft); fall back to a generic narrowbody.
        let icao = if !identity.resolved_icao.is_empty() {
            identity.resolved_icao.as_str()
        } else if !identity.atc_model.is_empty() {
            identity.atc_model.as_str()
        } else {
            "A320"
        };
        // The X-Plane plugin keys liveries on the CSL livery token, which won't line up
        // with a raw MSFS livery string; only forward a same-sim hint.
        let livery = if identity.livery.is_empty() { None } else { Some(identity.livery.as_str()) };

        let payload = serde_json::json!({
            "id": id,
            "icao": icao,
            "airline": airline,
            "livery": livery,
            "latitude": metrics.latitude,
            "longitude": metrics.longitude,
            "elevation": metrics.gps_altitude_msl * 0.3048, // convert feet to meters
            "pitch": to_xplane_attitude(metrics.pitch_angle) as f32,
            "heading": metrics.heading as f32,
            "roll": to_xplane_attitude(metrics.roll_angle) as f32,
            "on_ground": metrics.is_on_ground > 0.5,
            "gear_ratio": metrics.xp_gear_ratio,
            "nav_lights": metrics.nav_lights,
            "beacon_lights": metrics.beacon_lights,
            "strobe_lights": metrics.strobe_lights,
            "taxi_lights": metrics.taxi_lights,
            "landing_lights": metrics.landing_lights,
            "remove": false
        });

        if let Ok(data) = serde_json::to_vec(&payload) {
            let _ = socket.send_to(&data, "127.0.0.1:49020");
        }
    }
}

/// Execute any queued web-issued commands against X-Plane.
///
/// Unlike the MSFS path this has no thread affinity — the Web API is plain
/// HTTP — but it is drained from the monitor loop for symmetry, so both sims
/// stop executing commands the moment their monitor stops.
async fn drain_sim_commands(app: &AppHandle) {
    let pending = match app.try_state::<crate::sim_commands::CommandQueue>() {
        Some(queue) => queue.drain(),
        None => return,
    };
    if pending.is_empty() {
        return;
    }

    let config = app.state::<crate::config::ConfigManager>().get_config();
    let flight_id = crate::service::live_flight_id(app);

    for cmd in pending {
        let outcome = match crate::sim_commands::allowed_by_settings(
            &cmd.kind,
            config.is_logged_in(),
            config.is_logged_in(),
        ) {
            Err(why) => crate::sim_commands::Outcome::Rejected(why),
            Ok(()) => crate::sim_commands::execute_xplane(&cmd).await,
        };

        if let Some(flight_id) = flight_id {
            crate::sim_commands::ack(app, flight_id, &cmd.id, outcome).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuel_injected_pistons_are_pistons() {
        assert_eq!(engine_type_name(0), "piston");
        assert_eq!(engine_type_name(1), "piston"); // the C172 SP
        assert_eq!(engine_type_name(2), "turboprop");
        assert_eq!(engine_type_name(8), "turboprop");
        assert_eq!(engine_type_name(5), "jet");
        assert_eq!(engine_type_name(3), "electric");
    }

    #[test]
    fn attitude_round_trips_between_traffic_and_xplane_signs() {
        // Climbing at 7° nose up in X-Plane travels as -7 and is drawn at 7 again.
        let travelling = -7.0;
        assert_eq!(to_xplane_attitude(travelling), 7.0);
    }

    #[test]
    fn peers_get_our_attitude_in_traffic_signs() {
        // 7° nose up, 20° right wing down, as X-Plane reports and the log keeps them.
        let ours = FlightMetrics { pitch_angle: 7.0, roll_angle: 20.0, ..Default::default() };
        let sent = traffic_attitude(ours);
        assert_eq!((sent.pitch_angle, sent.roll_angle), (-7.0, -20.0));
    }
}
