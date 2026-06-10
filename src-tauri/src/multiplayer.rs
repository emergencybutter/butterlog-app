use std::net::{SocketAddr, UdpSocket, ToSocketAddrs};
use std::sync::Arc;
use parking_lot::Mutex;
use std::time::Duration;
use std::collections::HashMap;
use tauri::{AppHandle, Manager};
use crate::models::FlightMetrics;
use crate::config::ConfigManager;
use crate::UnifiedMonitor;

const STUN_TX_ID: [u8; 12] = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];

fn lerp(start: f64, target: f64, t: f64) -> f64 {
    start + (target - start) * t
}

fn interpolate_angle_0_360(start: f64, target: f64, t: f64) -> f64 {
    let mut diff = (target - start) % 360.0;
    if diff > 180.0 {
        diff -= 360.0;
    } else if diff < -180.0 {
        diff += 360.0;
    }
    let res = start + diff * t;
    (res + 360.0) % 360.0
}

fn interpolate_angle_signed(start: f64, target: f64, t: f64) -> f64 {
    let mut diff = (target - start) % 360.0;
    if diff > 180.0 {
        diff -= 360.0;
    } else if diff < -180.0 {
        diff += 360.0;
    }
    let mut res = start + diff * t;
    res = (res + 180.0) % 360.0;
    if res < 0.0 {
        res += 360.0;
    }
    res - 180.0
}

struct TrackedAircraft {
    last_seen: std::time::Instant,
    aircraft: String,
    atc_model: String,
    object_class: String,
    category: String,
    num_engines: i32,
    engine_type: String,
    start_time: std::time::Instant,
    expected_duration: std::time::Duration,
    start_metrics: FlightMetrics,
    target_metrics: FlightMetrics,
    current_metrics: FlightMetrics,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackedAircraftDebugInfo {
    pub id: String,
    pub aircraft: String,
    pub atc_model: String,
    pub object_class: String,
    pub category: String,
    pub num_engines: i32,
    pub engine_type: String,
    pub last_seen_seconds_ago: u64,
    pub latitude: f64,
    pub longitude: f64,
    pub gps_altitude_msl: f64,
    pub indicated_altitude: f64,
    pub ground_speed: f64,
    pub heading: f64,
    pub track: f64,
    pub pitch_angle: f64,
    pub roll_angle: f64,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MultiplayerDebugInfo {
    pub public_address: Option<String>,
    pub peers: Vec<String>,
    pub tracked_aircrafts: Vec<TrackedAircraftDebugInfo>,
}

pub struct MultiplayerManager {
    peers: Mutex<Vec<SocketAddr>>,
    public_address: Mutex<Option<SocketAddr>>,
    tracked_aircrafts: Mutex<HashMap<String, TrackedAircraft>>,
    last_received_log: Mutex<Option<std::time::Instant>>,
    last_emitted_log: Mutex<Option<std::time::Instant>>,
}

impl MultiplayerManager {
    pub fn new() -> Self {
        Self {
            peers: Mutex::new(Vec::new()),
            public_address: Mutex::new(None),
            tracked_aircrafts: Mutex::new(HashMap::new()),
            last_received_log: Mutex::new(None),
            last_emitted_log: Mutex::new(None),
        }
    }

    pub fn update_peers(&self, peer_strings: Vec<String>) {
        let mut peers = self.peers.lock();
        let new_peers: Vec<SocketAddr> = peer_strings
            .into_iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        *peers = new_peers;
    }

    pub fn get_public_address(&self) -> Option<SocketAddr> {
        *self.public_address.lock()
    }

    pub fn get_debug_info(&self) -> MultiplayerDebugInfo {
        let public_address = self.public_address.lock().map(|addr| addr.to_string());
        
        let peers = self.peers.lock()
            .iter()
            .map(|addr| addr.to_string())
            .collect();
            
        let now = std::time::Instant::now();
        let tracked_aircrafts = self.tracked_aircrafts.lock()
            .iter()
            .map(|(id, ac)| {
                let last_seen_seconds_ago = now.duration_since(ac.last_seen).as_secs();
                TrackedAircraftDebugInfo {
                    id: id.clone(),
                    aircraft: ac.aircraft.clone(),
                    atc_model: ac.atc_model.clone(),
                    object_class: ac.object_class.clone(),
                    category: ac.category.clone(),
                    num_engines: ac.num_engines,
                    engine_type: ac.engine_type.clone(),
                    last_seen_seconds_ago,
                    latitude: ac.current_metrics.latitude,
                    longitude: ac.current_metrics.longitude,
                    gps_altitude_msl: ac.current_metrics.gps_altitude_msl,
                    indicated_altitude: ac.current_metrics.indicated_altitude,
                    ground_speed: ac.current_metrics.ground_speed,
                    heading: ac.current_metrics.heading,
                    track: ac.current_metrics.track,
                    pitch_angle: ac.current_metrics.pitch_angle,
                    roll_angle: ac.current_metrics.roll_angle,
                }
            })
            .collect();
            
        MultiplayerDebugInfo {
            public_address,
            peers,
            tracked_aircrafts,
        }
    }

    pub fn start(&self, app: AppHandle) {
        let multiplayer = app.state::<Arc<MultiplayerManager>>().inner().clone();
        
        // Dedicated thread for 50ms interpolation and simulator injection
        let interp_app = app.clone();
        let interp_multiplayer = multiplayer.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(50));
                
                let config = interp_app.state::<ConfigManager>().get_config();
                if !config.inject_butterlog_traffic && !config.enable_multiplayer_hubs {
                    continue;
                }
                
                let monitor = interp_app.state::<UnifiedMonitor>();
                if let Some(m) = monitor.get_connected_monitor() {
                    if m.is_connected() {
                        let now = std::time::Instant::now();
                        let mut tracked = interp_multiplayer.tracked_aircrafts.lock();
                        
                        for (id, ac) in tracked.iter_mut() {
                            let elapsed = now.duration_since(ac.start_time);
                            let duration_secs = ac.expected_duration.as_secs_f64();
                            let t = if duration_secs > 0.0 {
                                (elapsed.as_secs_f64() / duration_secs).clamp(0.0, 1.0)
                            } else {
                                1.0
                            };
                            
                            let lat = lerp(ac.start_metrics.latitude, ac.target_metrics.latitude, t);
                            let lon = lerp(ac.start_metrics.longitude, ac.target_metrics.longitude, t);
                            let gps_alt = lerp(ac.start_metrics.gps_altitude_msl, ac.target_metrics.gps_altitude_msl, t);
                            let ind_alt = lerp(ac.start_metrics.indicated_altitude, ac.target_metrics.indicated_altitude, t);
                            let gnd_spd = lerp(ac.start_metrics.ground_speed, ac.target_metrics.ground_speed, t);
                            
                            let hdg = interpolate_angle_0_360(ac.start_metrics.heading, ac.target_metrics.heading, t);
                            let trk = interpolate_angle_0_360(ac.start_metrics.track, ac.target_metrics.track, t);
                            let pitch = interpolate_angle_signed(ac.start_metrics.pitch_angle, ac.target_metrics.pitch_angle, t);
                            let roll = interpolate_angle_signed(ac.start_metrics.roll_angle, ac.target_metrics.roll_angle, t);
                            
                            ac.current_metrics = ac.target_metrics;
                            
                            ac.current_metrics.latitude = lat;
                            ac.current_metrics.longitude = lon;
                            ac.current_metrics.gps_altitude_msl = gps_alt;
                            ac.current_metrics.indicated_altitude = ind_alt;
                            ac.current_metrics.ground_speed = gnd_spd;
                            ac.current_metrics.heading = hdg;
                            ac.current_metrics.track = trk;
                            ac.current_metrics.pitch_angle = pitch;
                            ac.current_metrics.roll_angle = roll;
                            
                            m.update_remote_aircraft(
                                id,
                                &ac.aircraft,
                                &ac.atc_model,
                                &ac.object_class,
                                &ac.category,
                                ac.num_engines,
                                &ac.engine_type,
                                &ac.current_metrics,
                            );
                        }
                    }
                }
            }
        });

        let ping_app = app.clone();
        let ping_multiplayer = multiplayer.clone();
        tauri::async_runtime::spawn(async move {
            let client = reqwest::Client::new();
            let mut last_log_time: Option<std::time::Instant> = None;
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                
                let config = ping_app.state::<ConfigManager>().get_config();
                
                // Only ping if features are enabled and logged in
                let features_enabled = config.inject_butterlog_traffic || config.enable_multiplayer_hubs;
                let api_auth = config.api_auth();
                let (api_base, api_token) = match api_auth {
                    Some(auth) if features_enabled => auth,
                    _ => continue,
                };
                
                // Only ping if connected to a simulator
                let monitor = ping_app.state::<UnifiedMonitor>();
                let sim_connected = monitor.get_all_monitors().iter().any(|m| m.is_connected());
                if !sim_connected {
                    continue;
                }
                
                // Get public address discovered by STUN
                let public_addr = ping_multiplayer.get_public_address();
                let public_addr_str = public_addr.map(|a| a.to_string());
                
                let url = format!("{}/multiplayer/ping", api_base);
                let body = serde_json::json!({
                    "udp_address": public_addr_str
                });

                match client.post(&url).bearer_auth(&api_token).json(&body).send().await {
                    Ok(res) => {
                        if res.status().is_success() {
                            #[derive(serde::Deserialize)]
                            struct PingResponse {
                                peers: Vec<String>,
                            }
                            if let Ok(data) = res.json::<PingResponse>().await {
                                let now = std::time::Instant::now();
                                let should_log = match last_log_time {
                                    Some(t) if now.duration_since(t).as_secs() < 30 => false,
                                    _ => {
                                        last_log_time = Some(now);
                                        true
                                    }
                                };
                                if should_log {
                                    crate::append_log(&ping_app, format!("[Multiplayer Ping] Active. Received {} peers from service", data.peers.len()));
                                }
                                ping_multiplayer.update_peers(data.peers);
                            }
                        } else if res.status().as_u16() == 401 {
                            crate::force_logout(&ping_app, "service rejected token during multiplayer ping");
                        } else {
                            crate::append_log(&ping_app, format!("[Multiplayer Ping] Failed with status: {}", res.status()));
                        }
                    }
                    Err(e) => {
                        crate::append_log(&ping_app, format!("[Multiplayer Ping] Error: {}", e));
                    }
                }
            }
        });

        // Background thread for sending data
        std::thread::spawn(move || {
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => {
                    crate::append_log(&app, format!("[Multiplayer] Failed to bind UDP socket: {}", e));
                    return;
                }
            };
            
            let _ = socket.set_nonblocking(true);
            
            crate::append_log(&app, format!("[Multiplayer] UDP socket bound to: {:?}", socket.local_addr()));

            // Background thread for receiving data (UDP hole punching needs this to "open" the port)
            let recv_app = app.clone();
            let recv_socket = socket.try_clone().expect("Failed to clone socket");
            let recv_multiplayer = multiplayer.clone();
            
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match recv_socket.recv_from(&mut buf) {
                        Ok((size, addr)) => {
                            let data = &buf[..size];
                            
                            // Check if this is a STUN Binding Success Response
                            if size >= 20 && data[0] == 0x01 && data[1] == 0x01 && data[8..20] == STUN_TX_ID {
                                let mut pos = 20;
                                while pos + 4 <= size {
                                    let attr_type = u16::from_be_bytes([data[pos], data[pos+1]]);
                                    let attr_len = u16::from_be_bytes([data[pos+2], data[pos+3]]) as usize;
                                    pos += 4;
                                    
                                    if pos + attr_len > size {
                                        break;
                                    }
                                    
                                    let attr_val = &data[pos..pos + attr_len];
                                    if attr_type == 0x0020 { // XOR-MAPPED-ADDRESS
                                        if attr_len >= 8 && attr_val[1] == 0x01 { // IPv4
                                            let xor_port = u16::from_be_bytes([attr_val[2], attr_val[3]]);
                                            let port = xor_port ^ 0x2112;
                                            
                                            let xor_ip = u32::from_be_bytes([attr_val[4], attr_val[5], attr_val[6], attr_val[7]]);
                                            let ip = xor_ip ^ 0x2112A442;
                                            
                                            let public_addr = SocketAddr::new(
                                                std::net::IpAddr::V4(std::net::Ipv4Addr::from(ip)),
                                                port
                                            );
                                            
                                            let mut addr_lock = recv_multiplayer.public_address.lock();
                                            if *addr_lock != Some(public_addr) {
                                                *addr_lock = Some(public_addr);
                                                crate::append_log(&recv_app, format!("[Multiplayer] Discovered public UDP address: {}", public_addr));
                                            }
                                        }
                                    } else if attr_type == 0x0001 { // MAPPED-ADDRESS
                                        if attr_len >= 8 && attr_val[1] == 0x01 { // IPv4
                                            let port = u16::from_be_bytes([attr_val[2], attr_val[3]]);
                                            let ip = u32::from_be_bytes([attr_val[4], attr_val[5], attr_val[6], attr_val[7]]);
                                            
                                            let public_addr = SocketAddr::new(
                                                std::net::IpAddr::V4(std::net::Ipv4Addr::from(ip)),
                                                port
                                            );
                                            
                                            let mut addr_lock = recv_multiplayer.public_address.lock();
                                            if *addr_lock != Some(public_addr) {
                                                *addr_lock = Some(public_addr);
                                                crate::append_log(&recv_app, format!("[Multiplayer] Discovered public UDP address: {}", public_addr));
                                            }
                                        }
                                    }
                                    
                                    let pad = (4 - (attr_len % 4)) % 4;
                                    pos += attr_len + pad;
                                }
                            } else {
                                // Telemetry is only accepted from peers the service
                                // told us about: our public address is discoverable,
                                // so unsolicited senders must not be able to inject
                                // fake traffic into the simulator.
                                let is_known_peer = recv_multiplayer.peers.lock().contains(&addr);
                                if !is_known_peer {
                                    let now = std::time::Instant::now();
                                    let mut last_log = recv_multiplayer.last_received_log.lock();
                                    let should_log = match *last_log {
                                        Some(t) if now.duration_since(t).as_secs() < 60 => false,
                                        _ => {
                                            *last_log = Some(now);
                                            true
                                        }
                                    };
                                    if should_log {
                                        crate::append_log(&recv_app, format!("[Multiplayer RECV] Dropped packet from unknown sender {}", addr));
                                    }
                                    continue;
                                }

                                // Parse as JSON payload
                                if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(data) {
                                    if let (Some(aircraft), Some(metrics_val)) = (payload["aircraft"].as_str(), payload.get("metrics")) {
                                        match serde_json::from_value::<FlightMetrics>(metrics_val.clone()) {
                                            Err(e) => {
                                                crate::append_log(&recv_app, format!("[Multiplayer RECV] Failed to deserialize FlightMetrics from {}: {}", addr, e));
                                            }
                                            Ok(metrics) => {
                                            let atc_model = payload.get("atc_model").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let object_class = payload.get("object_class").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let category = payload.get("category").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                            let num_engines = payload.get("num_engines").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                            let engine_type = payload.get("engine_type").and_then(|v| v.as_str()).unwrap_or("").to_string();

                                            let config = recv_app.state::<ConfigManager>().get_config();
                                            let monitor = recv_app.state::<UnifiedMonitor>();
                                            
                                            // Throttled logging of received peer telemetry (every 10s)
                                            if config.inject_butterlog_traffic || config.enable_multiplayer_hubs {
                                                let now = std::time::Instant::now();
                                                let should_log = {
                                                    let mut last_log = recv_multiplayer.last_received_log.lock();
                                                    match *last_log {
                                                        Some(t) if now.duration_since(t).as_secs() < 60 => false,
                                                        _ => {
                                                            *last_log = Some(now);
                                                            true
                                                        }
                                                    }
                                                };
                                                if should_log {
                                                    crate::append_log(
                                                        &recv_app,
                                                        format!(
                                                            "[Multiplayer RECV] Update from {}: {} [{}] at Lat={:.6}, Lon={:.6}, Alt={:.1} ft MSL, Hdg={:.1}",
                                                            addr, aircraft, atc_model, metrics.latitude, metrics.longitude, metrics.gps_altitude_msl, metrics.heading
                                                        )
                                                    );
                                                }
                                            }
                                            
                                            // Determine if we should process this aircraft
                                            let mut should_process = false;
                                            if let Some(m) = monitor.get_connected_monitor() {
                                                if m.is_connected() {
                                                    if config.inject_butterlog_traffic {
                                                        let self_metrics = m.get_metrics();
                                                        if self_metrics.latitude != 0.0 || self_metrics.longitude != 0.0 {
                                                            let dist = crate::sim_monitor::calculate_distance(
                                                                self_metrics.latitude,
                                                                self_metrics.longitude,
                                                                metrics.latitude,
                                                                metrics.longitude,
                                                            );
                                                            if dist <= 20.0 {
                                                                should_process = true;
                                                            }
                                                        }
                                                    } else if config.enable_multiplayer_hubs {
                                                        should_process = true;
                                                    }
                                                }
                                            }

                                            if should_process {
                                                let now = std::time::Instant::now();
                                                let mut tracked = recv_multiplayer.tracked_aircrafts.lock();
                                                if let Some(ac) = tracked.get_mut(&addr.to_string()) {
                                                    let elapsed = now.duration_since(ac.last_seen);
                                                    let expected_duration = elapsed.clamp(Duration::from_millis(50), Duration::from_secs(2));
                                                    
                                                    ac.start_metrics = ac.current_metrics;
                                                    ac.target_metrics = metrics;
                                                    ac.start_time = now;
                                                    ac.expected_duration = expected_duration;
                                                    ac.last_seen = now;
                                                    
                                                    ac.aircraft = aircraft.to_string();
                                                    ac.atc_model = atc_model.clone();
                                                    ac.object_class = object_class.clone();
                                                    ac.category = category.clone();
                                                    ac.num_engines = num_engines;
                                                    ac.engine_type = engine_type.clone();
                                                } else {
                                                    tracked.insert(addr.to_string(), TrackedAircraft {
                                                        last_seen: now,
                                                        aircraft: aircraft.to_string(),
                                                        atc_model: atc_model.clone(),
                                                        object_class: object_class.clone(),
                                                        category: category.clone(),
                                                        num_engines,
                                                        engine_type: engine_type.clone(),
                                                        start_time: now,
                                                        expected_duration: Duration::from_millis(250),
                                                        start_metrics: metrics,
                                                        target_metrics: metrics,
                                                        current_metrics: metrics,
                                                    });
                                                }
                                            }
                                            }
                                        } // match
                                    }
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => {
                            crate::append_log(&recv_app, format!("[Multiplayer] Receive error: {}", e));
                            break;
                        }
                    }
                }
            });

            let mut last_stun_query = std::time::Instant::now() - Duration::from_secs(300);
            let mut cached_stun_addr = None;
            let mut last_prune = std::time::Instant::now();
            let mut last_publish_hubs = std::time::Instant::now();
            let mut last_publish_inject = std::time::Instant::now();

            loop {
                let now = std::time::Instant::now();
                let config = app.state::<ConfigManager>().get_config();

                // 1. Periodically query STUN to discover / refresh our public address
                if now.duration_since(last_stun_query) >= Duration::from_secs(60) {
                    let mut request = [0u8; 20];
                    request[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // Binding Request
                    request[2..4].copy_from_slice(&0x0000u16.to_be_bytes()); // Length
                    request[4..8].copy_from_slice(&0x2112A442u32.to_be_bytes()); // Magic Cookie
                    request[8..20].copy_from_slice(&STUN_TX_ID);
                    
                    if cached_stun_addr.is_none() {
                        if let Ok(mut addrs) = ("stun.l.google.com", 19302).to_socket_addrs() {
                            cached_stun_addr = addrs.next();
                        }
                    }
                    if let Some(stun_addr) = cached_stun_addr {
                        let _ = socket.send_to(&request, stun_addr);
                    }
                    last_stun_query = now;
                }

                // 2. Prune tracked aircrafts (every 1 second)
                if now.duration_since(last_prune) >= Duration::from_secs(1) {
                    let monitor = app.state::<UnifiedMonitor>();
                    if let Some(m) = monitor.get_connected_monitor() {
                        let self_metrics = m.get_metrics();
                        let mut tracked = multiplayer.tracked_aircrafts.lock();
                        let mut to_remove = Vec::new();
                        
                        for (id, ac) in tracked.iter() {
                            let age = now.duration_since(ac.last_seen);
                            let dist = if self_metrics.latitude != 0.0 || self_metrics.longitude != 0.0 {
                                crate::sim_monitor::calculate_distance(
                                    self_metrics.latitude,
                                    self_metrics.longitude,
                                    ac.current_metrics.latitude,
                                    ac.current_metrics.longitude,
                                )
                            } else {
                                999.0 // force remove if we don't have our own coordinates
                            };
                            
                            let prune_dist = if config.enable_multiplayer_hubs {
                                false
                            } else {
                                dist > 20.0
                            };
                            
                            if age > Duration::from_secs(60) || prune_dist {
                                to_remove.push(id.clone());
                            }
                        }
                        
                        for id in to_remove {
                            tracked.remove(&id);
                        }
                    }
                    last_prune = now;
                }

                // 3. Publish our position to peers
                let mut should_publish = false;
                
                if config.enable_multiplayer_hubs {
                    if now.duration_since(last_publish_hubs) >= Duration::from_millis(200) {
                        should_publish = true;
                        last_publish_hubs = now;
                    }
                }
                
                if config.inject_butterlog_traffic {
                    if now.duration_since(last_publish_inject) >= Duration::from_millis(250) {
                        should_publish = true;
                        last_publish_inject = now;
                    }
                }
                
                if should_publish {
                    let monitor = app.state::<UnifiedMonitor>();
                    if let Some(m) = monitor.get_connected_monitor() {
                        let metrics = m.get_metrics();
                        let aircraft = m.get_aircraft_info();

                        let payload = serde_json::json!({
                            "aircraft": aircraft.title,
                            "atc_model": aircraft.atc_model,
                            "object_class": aircraft.object_class,
                            "category": aircraft.category,
                            "num_engines": aircraft.num_engines,
                            "engine_type": aircraft.engine_type,
                            "metrics": metrics
                        });

                        if let Ok(data) = serde_json::to_vec(&payload) {
                            let peers = multiplayer.peers.lock().clone();
                            for peer in peers.iter() {
                                let _ = socket.send_to(&data, peer);
                            }
                        }
                    }
                }

                std::thread::sleep(Duration::from_millis(50));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stun_parsing() {
        // Construct a mock STUN response
        // Message Type: 0x0101 (Binding Success Response)
        // Length: 12 bytes (one XOR-MAPPED-ADDRESS attribute of 8 bytes + 4 bytes header)
        // Magic Cookie: 0x2112a442
        // Transaction ID: STUN_TX_ID
        let mut buf = vec![0u8; 32];
        buf[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
        buf[2..4].copy_from_slice(&12u16.to_be_bytes()); // Attribute size
        buf[4..8].copy_from_slice(&0x2112A442u32.to_be_bytes());
        buf[8..20].copy_from_slice(&STUN_TX_ID);

        // XOR-MAPPED-ADDRESS attribute
        // Type: 0x0020
        // Length: 8 bytes
        buf[20..22].copy_from_slice(&0x0020u16.to_be_bytes());
        buf[22..24].copy_from_slice(&8u16.to_be_bytes());
        
        // Value: Family = 0x01 (IPv4), XOR-Port, XOR-IP
        buf[24] = 0x00;
        buf[25] = 0x01;
        // Port = 4902 (0x1326) XOR 0x2112 = 0x3234
        buf[26..28].copy_from_slice(&0x3234u16.to_be_bytes());
        // IP = 192.168.1.100 (0xC0A80164) XOR 0x2112A442 = 0xE1BAA526
        buf[28..32].copy_from_slice(&0xE1BAA526u32.to_be_bytes());

        // Parse attributes manually like in the receiver loop
        let size = buf.len();
        let mut pos = 20;
        let mut parsed_addr = None;

        while pos + 4 <= size {
            let attr_type = u16::from_be_bytes([buf[pos], buf[pos+1]]);
            let attr_len = u16::from_be_bytes([buf[pos+2], buf[pos+3]]) as usize;
            pos += 4;
            
            if pos + attr_len > size {
                break;
            }
            
            let attr_val = &buf[pos..pos + attr_len];
            if attr_type == 0x0020 {
                if attr_len >= 8 && attr_val[1] == 0x01 {
                    let xor_port = u16::from_be_bytes([attr_val[2], attr_val[3]]);
                    let port = xor_port ^ 0x2112;
                    let xor_ip = u32::from_be_bytes([attr_val[4], attr_val[5], attr_val[6], attr_val[7]]);
                    let ip = xor_ip ^ 0x2112A442;
                    
                    parsed_addr = Some(SocketAddr::new(
                        std::net::IpAddr::V4(std::net::Ipv4Addr::from(ip)),
                        port
                    ));
                }
            }
            
            let pad = (4 - (attr_len % 4)) % 4;
            pos += attr_len + pad;
        }

        assert!(parsed_addr.is_some());
        let addr = parsed_addr.unwrap();
        assert_eq!(addr.port(), 4902);
        assert_eq!(addr.ip().to_string(), "192.168.1.100");
    }

    #[test]
    fn test_interpolation_logic() {
        // Test basic lerp
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
        assert_eq!(lerp(10.0, 20.0, 0.25), 12.5);
        assert_eq!(lerp(5.0, 5.0, 0.5), 5.0);

        // Test 0-360 angle interpolation (headings)
        // Midpoint of 350 and 10 heading should be 0.0 (or 360.0 equivalent)
        let mid1 = interpolate_angle_0_360(350.0, 10.0, 0.5);
        assert!((mid1 - 0.0).abs() < 1e-5 || (mid1 - 360.0).abs() < 1e-5);

        // Midpoint of 10 and 350 heading should be 0.0 (or 360.0 equivalent)
        let mid2 = interpolate_angle_0_360(10.0, 350.0, 0.5);
        assert!((mid2 - 0.0).abs() < 1e-5 || (mid2 - 360.0).abs() < 1e-5);

        // Regular heading transition
        assert!((interpolate_angle_0_360(90.0, 180.0, 0.5) - 135.0).abs() < 1e-5);

        // Test signed angle interpolation (pitch/roll [-180, 180])
        // Midpoint of 170 and -170 should be 180 (or -180)
        let signed_mid1 = interpolate_angle_signed(170.0, -170.0, 0.5);
        assert!((signed_mid1 - 180.0).abs() < 1e-5 || (signed_mid1 - -180.0).abs() < 1e-5);

        // Midpoint of -170 and 170 should be 180 (or -180)
        let signed_mid2 = interpolate_angle_signed(-170.0, 170.0, 0.5);
        assert!((signed_mid2 - 180.0).abs() < 1e-5 || (signed_mid2 - -180.0).abs() < 1e-5);

        // Regular transition
        assert!((interpolate_angle_signed(-10.0, 10.0, 0.5) - 0.0).abs() < 1e-5);
    }
}

