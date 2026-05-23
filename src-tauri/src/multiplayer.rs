use std::net::{SocketAddr, UdpSocket, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::collections::HashMap;
use tauri::{AppHandle, Manager};
use crate::models::FlightMetrics;
use crate::config::ConfigManager;
use crate::UnifiedMonitor;

const STUN_TX_ID: [u8; 12] = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];

struct TrackedAircraft {
    last_seen: std::time::Instant,
    aircraft: String,
    metrics: FlightMetrics,
}

pub struct MultiplayerManager {
    peers: Mutex<Vec<SocketAddr>>,
    public_address: Mutex<Option<SocketAddr>>,
    tracked_aircrafts: Mutex<HashMap<String, TrackedAircraft>>,
}

impl MultiplayerManager {
    pub fn new() -> Self {
        Self {
            peers: Mutex::new(Vec::new()),
            public_address: Mutex::new(None),
            tracked_aircrafts: Mutex::new(HashMap::new()),
        }
    }

    pub fn update_peers(&self, peer_strings: Vec<String>) {
        let mut peers = self.peers.lock().unwrap();
        let new_peers: Vec<SocketAddr> = peer_strings
            .into_iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        *peers = new_peers;
    }

    pub fn get_public_address(&self) -> Option<SocketAddr> {
        *self.public_address.lock().unwrap()
    }

    pub fn start(&self, app: AppHandle) {
        let multiplayer = app.state::<Arc<MultiplayerManager>>().inner().clone();
        
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
                                            
                                            let mut addr_lock = recv_multiplayer.public_address.lock().unwrap();
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
                                            
                                            let mut addr_lock = recv_multiplayer.public_address.lock().unwrap();
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
                                // Parse as JSON payload
                                if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(data) {
                                    if let (Some(aircraft), Some(metrics_val)) = (payload["aircraft"].as_str(), payload.get("metrics")) {
                                        if let Ok(metrics) = serde_json::from_value::<FlightMetrics>(metrics_val.clone()) {
                                            let config = recv_app.state::<ConfigManager>().get_config();
                                            let monitor = recv_app.state::<UnifiedMonitor>();
                                            
                                            if config.inject_butterlog_traffic {
                                                if let Some(m) = monitor.get_connected_monitor() {
                                                    if m.is_connected() {
                                                        let self_metrics = m.get_metrics();
                                                        // Validate self coordinates
                                                        if self_metrics.latitude != 0.0 || self_metrics.longitude != 0.0 {
                                                            let dist = crate::sim_monitor::calculate_distance(
                                                                self_metrics.latitude,
                                                                self_metrics.longitude,
                                                                metrics.latitude,
                                                                metrics.longitude,
                                                            );
                                                            
                                                            if dist <= 20.0 {
                                                                // Insert/update into tracked aircrafts list
                                                                let mut tracked = recv_multiplayer.tracked_aircrafts.lock().unwrap();
                                                                tracked.insert(addr.to_string(), TrackedAircraft {
                                                                    last_seen: std::time::Instant::now(),
                                                                    aircraft: aircraft.to_string(),
                                                                    metrics,
                                                                });
                                                                
                                                                // Instantly feed update to simulator
                                                                m.update_remote_aircraft(&addr.to_string(), aircraft, &metrics);
                                                            }
                                                        }
                                                    }
                                                }
                                            } else if config.enable_multiplayer_hubs {
                                                if let Some(m) = monitor.get_connected_monitor() {
                                                    m.update_remote_aircraft(&addr.to_string(), aircraft, &metrics);
                                                }
                                            }
                                        }
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
                        let mut tracked = multiplayer.tracked_aircrafts.lock().unwrap();
                        let mut to_remove = Vec::new();
                        
                        for (id, ac) in tracked.iter() {
                            let age = now.duration_since(ac.last_seen);
                            let dist = if self_metrics.latitude != 0.0 || self_metrics.longitude != 0.0 {
                                crate::sim_monitor::calculate_distance(
                                    self_metrics.latitude,
                                    self_metrics.longitude,
                                    ac.metrics.latitude,
                                    ac.metrics.longitude,
                                )
                            } else {
                                999.0 // force remove if we don't have our own coordinates
                            };
                            
                            if age > Duration::from_secs(60) || dist > 20.0 {
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
                            "metrics": metrics
                        });

                        if let Ok(data) = serde_json::to_vec(&payload) {
                            let peers = multiplayer.peers.lock().unwrap();
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
        // IP = 192.168.1.100 (0xC0A80164) XOR 0x2112A442 = 0xE1BAB526
        buf[28..32].copy_from_slice(&0xE1BAB526u32.to_be_bytes());

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
}

